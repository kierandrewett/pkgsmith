//! RPM packages (`.rpm`), written to the version 4 file format that rpm 4.14 and newer read:
//! a lead, a signature header, the metadata header, and a compressed cpio archive of the files.
//!
//! The package is not signed with a key. The signature header holds only the digests that `rpm -K` checks.

use super::ensure_resolved;
use crate::archive::Compression;
use crate::hash::digests;
use crate::package::Package;
use crate::tree::{Data, Entry};
use crate::version::Version;
use anyhow::{Result, bail};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const HEADER_MAGIC: [u8; 8] = [0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0];

// Header tags.
const HEADERSIGNATURES: u32 = 62;
const HEADERIMMUTABLE: u32 = 63;
const HEADERI18NTABLE: u32 = 100;
const NAME: u32 = 1000;
const VERSION: u32 = 1001;
const RELEASE: u32 = 1002;
const SUMMARY: u32 = 1004;
const DESCRIPTION: u32 = 1005;
const BUILDTIME: u32 = 1006;
const BUILDHOST: u32 = 1007;
const SIZE: u32 = 1009;
const LICENSE: u32 = 1014;
const PACKAGER: u32 = 1015;
const GROUP: u32 = 1016;
const URL: u32 = 1020;
const OS: u32 = 1021;
const ARCH: u32 = 1022;
const PREIN: u32 = 1023;
const POSTIN: u32 = 1024;
const PREUN: u32 = 1025;
const POSTUN: u32 = 1026;
const FILESIZES: u32 = 1028;
const FILEMODES: u32 = 1030;
const FILERDEVS: u32 = 1033;
const FILEMTIMES: u32 = 1034;
const FILEDIGESTS: u32 = 1035;
const FILELINKTOS: u32 = 1036;
const FILEFLAGS: u32 = 1037;
const FILEUSERNAME: u32 = 1039;
const FILEGROUPNAME: u32 = 1040;
const SOURCERPM: u32 = 1044;
const FILEVERIFYFLAGS: u32 = 1045;
const PROVIDENAME: u32 = 1047;
const REQUIREFLAGS: u32 = 1048;
const REQUIRENAME: u32 = 1049;
const REQUIREVERSION: u32 = 1050;
const CONFLICTFLAGS: u32 = 1053;
const CONFLICTNAME: u32 = 1054;
const CONFLICTVERSION: u32 = 1055;
const RPMVERSION: u32 = 1064;
const PREINPROG: u32 = 1085;
const POSTINPROG: u32 = 1086;
const PREUNPROG: u32 = 1087;
const POSTUNPROG: u32 = 1088;
const OBSOLETENAME: u32 = 1090;
const FILEDEVICES: u32 = 1095;
const FILEINODES: u32 = 1096;
const FILELANGS: u32 = 1097;
const PROVIDEFLAGS: u32 = 1112;
const PROVIDEVERSION: u32 = 1113;
const OBSOLETEFLAGS: u32 = 1114;
const OBSOLETEVERSION: u32 = 1115;
const DIRINDEXES: u32 = 1116;
const BASENAMES: u32 = 1117;
const DIRNAMES: u32 = 1118;
const PAYLOADFORMAT: u32 = 1124;
const PAYLOADCOMPRESSOR: u32 = 1125;
const PAYLOADFLAGS: u32 = 1126;
const LONGSIZE: u32 = 5009;
const FILEDIGESTALGO: u32 = 5011;
const PAYLOADDIGEST: u32 = 5092;
const PAYLOADDIGESTALGO: u32 = 5093;
const RECOMMENDNAME: u32 = 5046;
const RECOMMENDVERSION: u32 = 5047;
const RECOMMENDFLAGS: u32 = 5048;
const SUGGESTNAME: u32 = 5049;
const SUGGESTVERSION: u32 = 5050;
const SUGGESTFLAGS: u32 = 5051;

// Signature header tags.
const SIG_SHA1: u32 = 269;
const SIG_SHA256: u32 = 273;
const SIG_SIZE: u32 = 1000;
const SIG_MD5: u32 = 1004;
const SIG_PAYLOADSIZE: u32 = 1007;

// Dependency flags.
const LESS: u32 = 2;
const GREATER: u32 = 4;
const EQUAL: u32 = 8;
const INTERP: u32 = 0x100;
const SCRIPT_PRE: u32 = 0x200;
const SCRIPT_POST: u32 = 0x400;
const SCRIPT_PREUN: u32 = 0x800;
const SCRIPT_POSTUN: u32 = 0x1000;
const RPMLIB: u32 = 0x100_0000;

// File flags.
const FILE_CONFIG: u32 = 1;
const FILE_NOREPLACE: u32 = 16;
const FILE_GHOST: u32 = 64;

#[derive(Debug, Clone)]
enum Value {
    Int16(Vec<u16>),
    Int32(Vec<u32>),
    Int64(Vec<u64>),
    Str(String),
    Bin(Vec<u8>),
    StrArray(Vec<String>),
    I18n(String),
}

impl Value {
    fn type_code(&self) -> u32 {
        match self {
            Value::Int16(_) => 3,
            Value::Int32(_) => 4,
            Value::Int64(_) => 5,
            Value::Str(_) => 6,
            Value::Bin(_) => 7,
            Value::StrArray(_) => 8,
            Value::I18n(_) => 9,
        }
    }

    fn alignment(&self) -> usize {
        match self {
            Value::Int16(_) => 2,
            Value::Int32(_) => 4,
            Value::Int64(_) => 8,
            _ => 1,
        }
    }

    fn count(&self) -> u32 {
        (match self {
            Value::Int16(v) => v.len(),
            Value::Int32(v) => v.len(),
            Value::Int64(v) => v.len(),
            Value::Str(_) | Value::I18n(_) => 1,
            Value::Bin(v) => v.len(),
            Value::StrArray(v) => v.len(),
        }) as u32
    }

    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Value::Int16(v) => v.iter().for_each(|n| out.extend(n.to_be_bytes())),
            Value::Int32(v) => v.iter().for_each(|n| out.extend(n.to_be_bytes())),
            Value::Int64(v) => v.iter().for_each(|n| out.extend(n.to_be_bytes())),
            Value::Str(s) | Value::I18n(s) => {
                out.extend(s.as_bytes());
                out.push(0);
            }
            Value::Bin(v) => out.extend(v),
            Value::StrArray(v) => v.iter().for_each(|s| {
                out.extend(s.as_bytes());
                out.push(0);
            }),
        }
    }
}

/// One header: the tags sorted, a region tag first, then the data store. `region` is 63 for the metadata header and 62
/// for the signature header. The region trailer lets rpm treat the header as one immutable block it can digest.
fn header(mut tags: Vec<(u32, Value)>, region: u32) -> Vec<u8> {
    tags.sort_by_key(|(tag, _)| *tag);
    let index_count = tags.len() as u32 + 1;
    let mut store = Vec::new();
    let mut index = Vec::new();
    for (tag, value) in &tags {
        while store.len() % value.alignment() != 0 {
            store.push(0);
        }
        index.push((*tag, value.type_code(), store.len() as u32, value.count()));
        value.encode(&mut store);
    }
    let trailer_at = store.len() as u32;
    let mut trailer = Vec::new();
    trailer.extend(region.to_be_bytes());
    trailer.extend(7u32.to_be_bytes());
    trailer.extend((-((index_count * 16) as i32)).to_be_bytes());
    trailer.extend(16u32.to_be_bytes());
    store.extend(&trailer);

    let mut out = HEADER_MAGIC.to_vec();
    out.extend(index_count.to_be_bytes());
    out.extend((store.len() as u32).to_be_bytes());
    for (tag, kind, offset, count) in
        std::iter::once((region, 7u32, trailer_at, 16u32)).chain(index)
    {
        out.extend(tag.to_be_bytes());
        out.extend(kind.to_be_bytes());
        out.extend(offset.to_be_bytes());
        out.extend(count.to_be_bytes());
    }
    out.extend(store);
    out
}

fn lead(name: &str, arch: &str) -> Vec<u8> {
    let mut out = vec![0xed, 0xab, 0xee, 0xdb, 3, 0];
    out.extend(0u16.to_be_bytes()); // binary package
    out.extend((if arch == "aarch64" { 12u16 } else { 1u16 }).to_be_bytes());
    let mut field = [0u8; 66];
    let bytes = name.as_bytes();
    field[..bytes.len().min(65)].copy_from_slice(&bytes[..bytes.len().min(65)]);
    out.extend(field);
    out.extend(1u16.to_be_bytes()); // Linux
    out.extend(5u16.to_be_bytes()); // signature in a header
    out.extend([0u8; 16]);
    out
}

fn architecture(arch: &str) -> &'static str {
    match arch {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        _ => "noarch",
    }
}

/// `name`, `name >= 1.2`, `name = 1.2-3` -> (name, flags, version).
fn dependency(text: &str) -> (String, u32, String) {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if let [name, op, version] = parts[..] {
        let flags = match op {
            ">=" => GREATER | EQUAL,
            "<=" => LESS | EQUAL,
            ">" => GREATER,
            "<" => LESS,
            "=" | "==" => EQUAL,
            _ => 0,
        };
        return (name.to_string(), flags, version.to_string());
    }
    (text.trim().to_string(), 0, String::new())
}

struct Deps {
    names: Vec<String>,
    flags: Vec<u32>,
    versions: Vec<String>,
}

impl Deps {
    fn new() -> Deps {
        Deps {
            names: vec![],
            flags: vec![],
            versions: vec![],
        }
    }

    fn add(&mut self, name: &str, flags: u32, version: &str) {
        self.names.push(name.into());
        self.flags.push(flags);
        self.versions.push(version.into());
    }

    fn add_text(&mut self, text: &str) {
        let (name, flags, version) = dependency(text);
        self.add(&name, flags, &version);
    }

    fn push(&self, tags: &mut Vec<(u32, Value)>, name: u32, flags: u32, version: u32) {
        if self.names.is_empty() {
            return;
        }
        tags.push((name, Value::StrArray(self.names.clone())));
        tags.push((flags, Value::Int32(self.flags.clone())));
        tags.push((version, Value::StrArray(self.versions.clone())));
    }
}

fn file_mode(entry: &Entry) -> u32 {
    let kind = match entry.data {
        Data::Dir => 0o040000,
        Data::Symlink { .. } => 0o120000,
        _ => 0o100000,
    };
    kind | entry.mode
}

fn split_path(path: &str) -> (String, String) {
    let (dir, base) = path.rsplit_once('/').unwrap_or(("", path));
    (format!("{dir}/"), base.to_string())
}

/// One cpio "newc" member. Names, data and the header are padded to four bytes.
fn cpio_member(
    out: &mut Vec<u8>,
    inode: usize,
    mode: u32,
    nlink: u32,
    mtime: u64,
    data: &[u8],
    name: &str,
) -> Result<()> {
    if data.len() > u32::MAX as usize {
        bail!("{name} is larger than 4 GiB, which the cpio archive of an rpm cannot hold");
    }
    let name = format!("{name}\0");
    out.extend(
        format!(
            "070701{inode:08x}{mode:08x}{:08x}{:08x}{nlink:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
            0,
            0,
            mtime as u32,
            data.len(),
            0,
            0,
            0,
            0,
            name.len(),
            0
        )
        .bytes(),
    );
    out.extend(name.bytes());
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out.extend(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    Ok(())
}

/// The payload: every entry that is not a ghost, in header order, then the `TRAILER!!!` member.
fn payload(entries: &[Entry], mtime: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let name = format!(".{}", entry.path);
        let inode = index + 1;
        match &entry.data {
            Data::Ghost => {}
            Data::Dir => cpio_member(&mut out, inode, file_mode(entry), 2, mtime, &[], &name)?,
            Data::Symlink { target } => cpio_member(
                &mut out,
                inode,
                file_mode(entry),
                1,
                mtime,
                target.as_bytes(),
                &name,
            )?,
            Data::File { src, .. } => {
                cpio_member(
                    &mut out,
                    inode,
                    file_mode(entry),
                    1,
                    mtime,
                    &std::fs::read(src)?,
                    &name,
                )?;
            }
        }
    }
    cpio_member(&mut out, 0, 0, 1, 0, &[], "TRAILER!!!")?;
    Ok(out)
}

/// The interpreter of a script: the `#!` line, or `/bin/sh`.
fn interpreter(script: &str) -> String {
    script
        .strip_prefix("#!")
        .and_then(|rest| rest.lines().next())
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("/bin/sh")
        .to_string()
}

pub fn build(package: &Package) -> Result<PathBuf> {
    let manifest = package.manifest;
    let effective = &package.effective;
    for (field, list) in [
        ("depends", &effective.depends),
        ("recommends", &effective.recommends),
        ("suggests", &effective.suggests),
        ("conflicts", &effective.conflicts),
        ("replaces", &effective.replaces),
        ("provides", &effective.provides),
    ] {
        ensure_resolved(list, field)?;
    }
    let mtime = package.options.mtime;
    let arch = architecture(&manifest.arch);
    let version = Version::parse(&manifest.version)?.tilde();
    let entries = &package.entries;
    let compression = Compression::parse(manifest.rpm.compression.as_deref().unwrap_or("zstd"))?;
    let (compressor, flags, rpmlib) = match compression {
        Compression::Zstd => ("zstd", "19", Some(("rpmlib(PayloadIsZstd)", "5.4.18-1"))),
        Compression::Xz => ("xz", "6", Some(("rpmlib(PayloadIsXz)", "5.2-1"))),
        Compression::Gzip => ("gzip", "9", None),
        Compression::None => bail!("an rpm payload is compressed: use zstd, xz or gzip"),
    };

    // Files.
    let (mut sizes, mut modes, mut mtimes, mut digest_list, mut links) =
        (vec![], vec![], vec![], vec![], vec![]);
    let (mut file_flags, mut users, mut groups) = (vec![], vec![], vec![]);
    let (mut dir_indexes, mut basenames, mut dirnames) = (vec![], vec![], Vec::<String>::new());
    for entry in entries {
        sizes.push(
            u32::try_from(entry.size())
                .map_err(|_| anyhow::anyhow!("{} is larger than 4 GiB", entry.path))?,
        );
        modes.push(file_mode(entry) as u16);
        mtimes.push(mtime as u32);
        digest_list.push(match &entry.data {
            Data::File { src, .. } => digests(&mut std::fs::File::open(src)?)?.1,
            _ => String::new(),
        });
        links.push(match &entry.data {
            Data::Symlink { target } => target.clone(),
            _ => String::new(),
        });
        file_flags.push(match &entry.data {
            Data::Ghost => FILE_GHOST,
            _ if entry.config => FILE_CONFIG | FILE_NOREPLACE,
            _ => 0,
        });
        users.push(entry.owner.clone());
        groups.push(entry.group.clone());
        let (dir, base) = split_path(&entry.path);
        let position = match dirnames.iter().position(|d| *d == dir) {
            Some(position) => position,
            None => {
                dirnames.push(dir);
                dirnames.len() - 1
            }
        };
        dir_indexes.push(position as u32);
        basenames.push(base);
    }
    let count = entries.len();

    // Dependencies.
    let mut requires = Deps::new();
    requires.add(
        "rpmlib(CompressedFileNames)",
        RPMLIB | LESS | EQUAL,
        "3.0.4-1",
    );
    requires.add("rpmlib(FileDigests)", RPMLIB | LESS | EQUAL, "4.6.0-1");
    requires.add(
        "rpmlib(PayloadFilesHavePrefix)",
        RPMLIB | LESS | EQUAL,
        "4.0-1",
    );
    if let Some((name, needed)) = rpmlib {
        requires.add(name, RPMLIB | LESS | EQUAL, needed);
    }
    effective.depends.iter().for_each(|d| requires.add_text(d));
    let scripts = &effective.scripts;
    let script_values = [
        (
            PREIN,
            PREINPROG,
            SCRIPT_PRE,
            package.script(&scripts.pre_install)?,
        ),
        (
            POSTIN,
            POSTINPROG,
            SCRIPT_POST,
            package.script(&scripts.post_install)?,
        ),
        (
            PREUN,
            PREUNPROG,
            SCRIPT_PREUN,
            package.script(&scripts.pre_remove)?,
        ),
        (
            POSTUN,
            POSTUNPROG,
            SCRIPT_POSTUN,
            package.script(&scripts.post_remove)?,
        ),
    ];
    let mut tags: Vec<(u32, Value)> = Vec::new();
    for (tag, program_tag, when, script) in &script_values {
        if let Some(text) = script {
            let program = interpreter(text);
            requires.add(&program, INTERP | when, "");
            tags.push((*tag, Value::Str(text.clone())));
            tags.push((*program_tag, Value::Str(program)));
        }
    }
    let nevr = format!("{version}-{}", manifest.release);
    let mut provides = Deps::new();
    provides.add(&manifest.name, EQUAL, &nevr);
    if arch != "noarch" {
        provides.add(
            &format!(
                "{}({})",
                manifest.name,
                if arch == "x86_64" { "x86-64" } else { arch }
            ),
            EQUAL,
            &nevr,
        );
    }
    effective.provides.iter().for_each(|d| provides.add_text(d));
    let mut conflicts = Deps::new();
    effective
        .conflicts
        .iter()
        .for_each(|d| conflicts.add_text(d));
    let mut obsoletes = Deps::new();
    effective
        .replaces
        .iter()
        .for_each(|d| obsoletes.add_text(d));
    let mut recommends = Deps::new();
    effective
        .recommends
        .iter()
        .for_each(|d| recommends.add_text(d));
    let mut suggests = Deps::new();
    effective.suggests.iter().for_each(|d| suggests.add_text(d));

    // Header.
    let installed = package.installed_size();
    let text = |value: &str| Value::Str(value.to_string());
    tags.extend([
        (HEADERI18NTABLE, Value::StrArray(vec!["C".into()])),
        (NAME, text(&manifest.name)),
        (VERSION, text(&version)),
        (RELEASE, text(&manifest.release)),
        (SUMMARY, Value::I18n(package.summary().to_string())),
        (
            DESCRIPTION,
            Value::I18n(manifest.description.trim().to_string()),
        ),
        (BUILDTIME, Value::Int32(vec![mtime as u32])),
        (BUILDHOST, text("pkgsmith")),
        (
            LICENSE,
            text(manifest.license.as_deref().unwrap_or("Unknown")),
        ),
        (
            GROUP,
            Value::I18n(
                manifest
                    .rpm
                    .group
                    .clone()
                    .unwrap_or_else(|| "Unspecified".into()),
            ),
        ),
        (OS, text("linux")),
        (ARCH, text(arch)),
        (SOURCERPM, text("(none)")),
        (RPMVERSION, text("4.19.0")),
        (PAYLOADFORMAT, text("cpio")),
        (PAYLOADCOMPRESSOR, text(compressor)),
        (PAYLOADFLAGS, text(flags)),
        (FILEDIGESTALGO, Value::Int32(vec![8])),
    ]);
    if let Ok(size) = u32::try_from(installed) {
        tags.push((SIZE, Value::Int32(vec![size])));
    } else {
        tags.push((LONGSIZE, Value::Int64(vec![installed])));
    }
    if let Some(packager) = &manifest.maintainer {
        tags.push((PACKAGER, text(packager)));
    }
    if let Some(url) = &manifest.homepage {
        tags.push((URL, text(url)));
    }
    if count > 0 {
        tags.extend([
            (FILESIZES, Value::Int32(sizes)),
            (FILEMODES, Value::Int16(modes)),
            (FILERDEVS, Value::Int16(vec![0; count])),
            (FILEMTIMES, Value::Int32(mtimes)),
            (FILEDIGESTS, Value::StrArray(digest_list)),
            (FILELINKTOS, Value::StrArray(links)),
            (FILEFLAGS, Value::Int32(file_flags)),
            (FILEUSERNAME, Value::StrArray(users)),
            (FILEGROUPNAME, Value::StrArray(groups)),
            (FILEVERIFYFLAGS, Value::Int32(vec![0xffff_ffff; count])),
            (FILEDEVICES, Value::Int32(vec![1; count])),
            (FILEINODES, Value::Int32((1..=count as u32).collect())),
            (FILELANGS, Value::StrArray(vec![String::new(); count])),
            (DIRINDEXES, Value::Int32(dir_indexes)),
            (BASENAMES, Value::StrArray(basenames)),
            (DIRNAMES, Value::StrArray(dirnames)),
        ]);
    }
    requires.push(&mut tags, REQUIRENAME, REQUIREFLAGS, REQUIREVERSION);
    provides.push(&mut tags, PROVIDENAME, PROVIDEFLAGS, PROVIDEVERSION);
    conflicts.push(&mut tags, CONFLICTNAME, CONFLICTFLAGS, CONFLICTVERSION);
    obsoletes.push(&mut tags, OBSOLETENAME, OBSOLETEFLAGS, OBSOLETEVERSION);
    recommends.push(&mut tags, RECOMMENDNAME, RECOMMENDFLAGS, RECOMMENDVERSION);
    suggests.push(&mut tags, SUGGESTNAME, SUGGESTFLAGS, SUGGESTVERSION);
    // The payload comes first because the header records the digest of the compressed bytes.
    let cpio = payload(entries, mtime)?;
    let compressed = compression.compress(&cpio)?;
    tags.push((
        PAYLOADDIGEST,
        Value::StrArray(vec![crate::hash::hex(&Sha256::digest(&compressed))]),
    ));
    tags.push((PAYLOADDIGESTALGO, Value::Int32(vec![8])));
    let header_bytes = header(tags, HEADERIMMUTABLE);

    // Signature header.
    let (mut md5, mut sha1, mut sha256) = (Md5::new(), Sha1::new(), Sha256::new());
    md5.update(&header_bytes);
    md5.update(&compressed);
    sha1.update(&header_bytes);
    sha256.update(&header_bytes);
    let signature = header(
        vec![
            (
                SIG_SIZE,
                Value::Int32(vec![(header_bytes.len() + compressed.len()) as u32]),
            ),
            (SIG_MD5, Value::Bin(md5.finalize().to_vec())),
            (SIG_SHA1, Value::Str(crate::hash::hex(&sha1.finalize()))),
            (SIG_SHA256, Value::Str(crate::hash::hex(&sha256.finalize()))),
            (SIG_PAYLOADSIZE, Value::Int32(vec![cpio.len() as u32])),
        ],
        HEADERSIGNATURES,
    );

    let mut file = lead(&format!("{}-{nevr}", manifest.name), arch);
    file.extend(&signature);
    while file.len() % 8 != 0 {
        file.push(0);
    }
    file.extend(&header_bytes);
    file.extend(&compressed);
    std::fs::create_dir_all(&package.options.out)?;
    let path = package.output(&format!("{}-{nevr}.{arch}.rpm", manifest.name));
    std::fs::write(&path, file)?;
    Ok(path)
}
