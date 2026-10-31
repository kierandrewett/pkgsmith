//! Void Linux packages (`.xbps`): a compressed tar with two Apple property lists, `props.plist` and `files.plist`, and
//! optional `INSTALL` and `REMOVE` scripts. `xbps-install` reads the lists and the scripts, and unpacks the files.
//!
//! The tar holds `./INSTALL`, `./REMOVE`, `./props.plist`, `./files.plist` and then the files with a `./` prefix, in
//! that order, as `xbps-create` writes them. The file name is `NAME-VERSION_REVISION.ARCH.xbps`. The revision is the
//! `release` of the manifest and must be a positive integer. The architecture is `x86_64`, `aarch64` or `noarch`, with
//! `-musl` added for a musl build.
//! Reference: <https://github.com/void-linux/xbps/blob/master/bin/xbps-create/main.c>, `xbps-create(1)` and the
//! package format described in <https://docs.voidlinux.org/xbps/index.html>.

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::archive::{Compression, Leading};
use pkgsmith_plugin::entry::{self, Data, Entry};
use pkgsmith_plugin::hash;
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::PathBuf;

/// The `xbps:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// `zstd` (default), `xz`, `gzip`, `bzip2` or `none`. The compression of the tar inside the package. `xbps-install`
    /// detects it when it reads the package.
    compression: Option<String>,
    /// `glibc` (default) or `musl`. A `musl` package gets `-musl` after the architecture, for example `x86_64-musl`,
    /// and `xbps-install` only installs it on a musl system. `noarch` packages have no suffix.
    libc: Option<String>,
}

/// The values of a package that the file name and the properties both use.
struct Naming {
    /// `name-version_revision`, the `pkgver` of the properties.
    pkgver: String,
    /// `version_revision`, the `version` of the properties.
    version: String,
    /// The architecture as xbps spells it, for example `x86_64-musl` or `noarch`.
    arch: String,
}

fn compression(options: &Options) -> Result<Compression> {
    Ok(match options.compression.as_deref().unwrap_or("zstd") {
        "zstd" => Compression::Zstd,
        "xz" => Compression::Xz,
        "gzip" => Compression::Gzip,
        "bzip2" => Compression::Bzip2,
        "none" => Compression::None,
        other => bail!("xbps compression {other:?} must be zstd, xz, gzip, bzip2 or none"),
    })
}

fn naming(package: &Package, options: &Options) -> Result<Naming> {
    let name = package.name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) || name.contains('/') {
        bail!("the package name {:?} is not valid for xbps", package.name);
    }
    let release: u64 = package.release.parse().with_context(|| {
        format!(
            "the release {:?} must be a positive integer for xbps",
            package.release
        )
    })?;
    if release == 0 {
        bail!(
            "the release {:?} must be a positive integer for xbps",
            package.release
        );
    }
    let version = Version::parse(&package.version)?.arch();
    let base = match package.arch.as_str() {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        "all" => {
            return Ok(Naming {
                pkgver: format!("{name}-{version}_{release}"),
                version: format!("{version}_{release}"),
                arch: "noarch".into(),
            });
        }
        other => bail!("arch {other:?} has no xbps architecture; use amd64, arm64 or all"),
    };
    let arch = match options.libc.as_deref().unwrap_or("glibc") {
        "glibc" => base.to_string(),
        "musl" => format!("{base}-musl"),
        other => bail!("xbps libc {other:?} must be glibc or musl"),
    };
    Ok(Naming {
        pkgver: format!("{name}-{version}_{release}"),
        version: format!("{version}_{release}"),
        arch,
    })
}

/// `NAME-VERSION_REVISION.ARCH.xbps`.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    let names = naming(package, &options)?;
    Ok(format!("{}.{}.xbps", names.pkgver, names.arch))
}

/// A `provides` value must be `name-version_revision`, which xbps checks. A bare name (one whose part after the last
/// dash is not a version) gets the version and the revision of this package, as Void does for `cmd:zstd-1.5.7_1`.
fn provide(text: &str, names: &Naming, release: &str) -> Result<String> {
    let valid = text.rsplit_once('-').is_some_and(|(name, version)| {
        !name.is_empty()
            && version.rsplit_once('_').is_some_and(|(v, rev)| {
                !v.is_empty() && !rev.is_empty() && rev.bytes().all(|b| b.is_ascii_digit())
            })
    });
    if valid {
        Ok(text.to_string())
    } else if !text.is_empty()
        && !text
            .rsplit_once('-')
            .is_some_and(|(_, tail)| starts_with_digit(tail))
    {
        Ok(format!("{text}-{}", names.version))
    } else {
        bail!(
            "provides {text:?} must be a bare name or name-version_revision for xbps (release {release})"
        )
    }
}

fn starts_with_digit(text: &str) -> bool {
    text.bytes().next().is_some_and(|b| b.is_ascii_digit())
}

/// The `props.plist` value types, as Apple property lists have them.
enum Value {
    String(String),
    Integer(u64),
    Array(Vec<Value>),
    Dict(Dict),
}

type Dict = BTreeMap<&'static str, Value>;

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

/// An array key, which xbps reads only when it holds items.
fn array(dict: &mut Dict, key: &'static str, items: Vec<Value>) {
    if !items.is_empty() {
        dict.insert(key, Value::Array(items));
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The Apple XML property list xbps reads. Keys are sorted, as `xbps-create` writes them.
fn plist(dict: &Dict) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n",
    );
    write_dict(&mut out, dict, 0);
    out.push_str("</plist>\n");
    out
}

fn write_dict(out: &mut String, dict: &Dict, depth: usize) {
    let pad = "\t".repeat(depth);
    out.push_str(&format!("{pad}<dict>\n"));
    for (key, value) in dict {
        out.push_str(&format!("{pad}\t<key>{}</key>\n", escape(key)));
        write_value(out, value, depth + 1);
    }
    out.push_str(&format!("{pad}</dict>\n"));
}

fn write_value(out: &mut String, value: &Value, depth: usize) {
    let pad = "\t".repeat(depth);
    match value {
        Value::String(s) => out.push_str(&format!("{pad}<string>{}</string>\n", escape(s))),
        Value::Integer(n) => out.push_str(&format!("{pad}<integer>{n}</integer>\n")),
        Value::Array(items) => {
            out.push_str(&format!("{pad}<array>\n"));
            for item in items {
                write_value(out, item, depth + 1);
            }
            out.push_str(&format!("{pad}</array>\n"));
        }
        Value::Dict(dict) => write_dict(out, dict, depth),
    }
}

/// `INSTALL` or `REMOVE`: xbps runs the one script with `pre` or `post` as the first argument. The script text of
/// pkgsmith loses its `#!` line, because it is written inside a `case` statement.
fn lifecycle_script(pre: Option<&str>, post: Option<&str>) -> Option<String> {
    if pre.is_none() && post.is_none() {
        return None;
    }
    let mut out = String::from("#!/bin/sh\ncase \"$1\" in\n");
    for (phase, body) in [("pre", pre), ("post", post)] {
        if let Some(body) = body {
            let lines: Vec<&str> = body.lines().skip_while(|l| l.starts_with("#!")).collect();
            out.push_str(&format!("{phase})\n{}\n;;\n", lines.join("\n")));
        }
    }
    out.push_str("esac\n");
    Some(out)
}

/// The absolute path of a symbolic link target, as `xbps-create` records it. A relative target is relative to the
/// folder of the link.
fn link_target(path: &str, target: &str) -> String {
    let joined = if target.starts_with('/') {
        target.to_string()
    } else {
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        format!("{dir}/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let compression = compression(&options)?;
    let names = naming(package, &options)?;
    let file_name = format!("{}.{}.xbps", names.pkgver, names.arch);
    let mtime = package.mtime;

    let summary = package.summary();
    if summary.is_empty() {
        bail!("the description needs a first line, which xbps uses as short_desc");
    }
    if !package.recommends.is_empty() || !package.suggests.is_empty() {
        host.warn("xbps has no recommends or suggests fields, so they are ignored");
    }

    let ghosts = package
        .entries
        .iter()
        .filter(|e| e.data == Data::Ghost)
        .count();
    if ghosts > 0 {
        host.warn("xbps cannot record files that the package does not carry (ghost files), so they are ignored");
    }
    let carried: Vec<Entry> = package
        .entries
        .iter()
        .filter(|e| e.data != Data::Ghost)
        .cloned()
        .collect();
    for entry in &carried {
        if entry.config && !matches!(entry.data, Data::File { .. }) {
            host.warn(&format!(
                "xbps keeps configuration files only when they are regular files, so {} is not kept as one",
                entry.path
            ));
        }
    }
    let entries = entry::with_parents(&carried);

    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut conf_files = Vec::new();
    let mut links = Vec::new();
    let mut conf_paths = Vec::new();
    for entry in &entries {
        let path = entry.path.clone();
        match &entry.data {
            Data::Dir => {
                let mut dict = Dict::new();
                dict.insert("file", text(&path));
                dirs.push(Value::Dict(dict));
            }
            Data::File { src, size } => {
                let (_, sha256) = hash::digests(
                    &mut File::open(src).with_context(|| format!("reading {}", src.display()))?,
                )?;
                let mut dict = Dict::new();
                dict.insert("file", text(&path));
                dict.insert("mtime", Value::Integer(mtime));
                dict.insert("sha256", text(&sha256));
                dict.insert("size", Value::Integer(*size));
                if entry.config {
                    conf_paths.push(Value::String(path));
                    conf_files.push(Value::Dict(dict));
                } else {
                    files.push(Value::Dict(dict));
                }
            }
            Data::Symlink { target } => {
                let mut dict = Dict::new();
                dict.insert("file", text(&path));
                dict.insert("mtime", Value::Integer(mtime));
                dict.insert("target", text(&link_target(&path, target)));
                links.push(Value::Dict(dict));
            }
            Data::Ghost => {}
        }
    }

    let mut props = Dict::new();
    props.insert("architecture", text(&names.arch));
    props.insert("pkgname", text(&package.name));
    props.insert("version", text(&names.version));
    props.insert("pkgver", text(&names.pkgver));
    props.insert("short_desc", text(summary));
    let long: String = package
        .description
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    if !long.trim().is_empty() {
        props.insert("long_desc", text(long.trim()));
    }
    for (key, value) in [
        ("homepage", &package.homepage),
        ("license", &package.license),
        ("maintainer", &package.maintainer),
    ] {
        if let Some(value) = value.as_deref().filter(|v| !v.is_empty()) {
            props.insert(key, text(value));
        }
    }
    props.insert("installed_size", Value::Integer(package.installed_size()));
    array(
        &mut props,
        "run_depends",
        package.depends.iter().map(|d| pattern(d)).collect(),
    );
    array(&mut props, "conf_files", conf_paths);
    array(
        &mut props,
        "conflicts",
        package.conflicts.iter().map(|d| pattern(d)).collect(),
    );
    array(
        &mut props,
        "provides",
        package
            .provides
            .iter()
            .map(|p| provide(p, &names, &package.release).map(|v| text(&v)))
            .collect::<Result<Vec<_>>>()?,
    );
    array(
        &mut props,
        "replaces",
        package.replaces.iter().map(|d| pattern(d)).collect(),
    );

    let mut files_dict = Dict::new();
    array(&mut files_dict, "dirs", dirs);
    array(&mut files_dict, "files", files);
    array(&mut files_dict, "conf_files", conf_files);
    array(&mut files_dict, "links", links);

    let mut leading = Vec::new();
    if let Some(script) = lifecycle_script(
        package.scripts.pre_install.as_deref(),
        package.scripts.post_install.as_deref(),
    ) {
        leading.push(Leading {
            name: "./INSTALL".into(),
            data: script.into_bytes(),
            mode: 0o755,
        });
    }
    if let Some(script) = lifecycle_script(
        package.scripts.pre_remove.as_deref(),
        package.scripts.post_remove.as_deref(),
    ) {
        leading.push(Leading {
            name: "./REMOVE".into(),
            data: script.into_bytes(),
            mode: 0o755,
        });
    }
    leading.push(Leading {
        name: "./props.plist".into(),
        data: plist(&props).into_bytes(),
        mode: 0o644,
    });
    leading.push(Leading {
        name: "./files.plist".into(),
        data: plist(&files_dict).into_bytes(),
        mode: 0o644,
    });

    // The SDK's tar writer goes through the `tar` crate, which drops the `./` that xbps looks for in every member
    // name. So the metadata members and the package tree are written here as ustar members with exact names.
    let mut tar = Vec::new();
    for member in &leading {
        ustar_entry(
            &member.name,
            b'0',
            member.mode,
            "root",
            "root",
            "",
            &member.data,
            mtime,
            &mut tar,
        )?;
    }
    body_tar(&entries, mtime, &mut tar)?;
    tar.extend_from_slice(&[0u8; 1024]);
    let file = package.output(&file_name);
    std::fs::write(&file, compression.compress(&tar)?)?;
    Ok(file)
}

/// Appends one ustar member. `name` is kept exactly as given, `./` included. A name longer than the 100-byte name field
/// is split at a `/` into the ustar prefix field.
#[allow(clippy::too_many_arguments)]
fn ustar_entry(
    name: &str,
    kind: u8,
    mode: u32,
    owner: &str,
    group: &str,
    link: &str,
    data: &[u8],
    mtime: u64,
    out: &mut Vec<u8>,
) -> Result<()> {
    let (prefix, short) = split_name(name)?;
    if link.len() > 100 {
        bail!("the link target {link:?} is longer than a tar header holds");
    }
    if owner.len() > 31 || group.len() > 31 {
        bail!("the owner or group name of {name:?} is longer than a tar header holds");
    }
    let mut header = [0u8; 512];
    header[..short.len()].copy_from_slice(short.as_bytes());
    put_octal(&mut header[100..108], u64::from(mode & 0o7777))?;
    put_octal(&mut header[108..116], 0)?;
    put_octal(&mut header[116..124], 0)?;
    put_octal(&mut header[124..136], data.len() as u64)?;
    put_octal(&mut header[136..148], mtime)?;
    header[156] = kind;
    header[157..157 + link.len()].copy_from_slice(link.as_bytes());
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[265..265 + owner.len()].copy_from_slice(owner.as_bytes());
    header[297..297 + group.len()].copy_from_slice(group.as_bytes());
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    put_octal(&mut header[148..156], u64::from(sum))?;
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    out.resize(out.len() + (512 - data.len() % 512) % 512, 0);
    Ok(())
}

/// Splits a path into the ustar (prefix, name) fields. A path that fits the name field stays whole.
fn split_name(name: &str) -> Result<(&str, &str)> {
    if name.len() <= 100 {
        return Ok(("", name));
    }
    for (i, _) in name.match_indices('/').rev() {
        let (prefix, rest) = (&name[..i], &name[i + 1..]);
        if prefix.len() <= 155 && rest.len() <= 100 {
            return Ok((prefix, rest));
        }
    }
    bail!("the path {name:?} is too long for a tar header")
}

/// Writes the package tree as ustar members: directories, regular files and symlinks, each with the owner, group and
/// mode the entry carries. Paths get the `./` prefix that xbps expects.
fn body_tar(entries: &[Entry], mtime: u64, out: &mut Vec<u8>) -> Result<()> {
    for entry in entries {
        let name = format!("./{}", entry.path.trim_start_matches('/'));
        match &entry.data {
            Data::Ghost => {}
            Data::Dir => ustar_entry(
                &format!("{name}/"),
                b'5',
                entry.mode,
                &entry.owner,
                &entry.group,
                "",
                &[],
                mtime,
                out,
            )?,
            Data::File { src, .. } => {
                let data =
                    std::fs::read(src).with_context(|| format!("reading {}", src.display()))?;
                ustar_entry(
                    &name,
                    b'0',
                    entry.mode,
                    &entry.owner,
                    &entry.group,
                    "",
                    &data,
                    mtime,
                    out,
                )?;
            }
            Data::Symlink { target } => ustar_entry(
                &name,
                b'2',
                entry.mode,
                &entry.owner,
                &entry.group,
                target,
                &[],
                mtime,
                out,
            )?,
        }
    }
    Ok(())
}

/// Writes `value` as a zero-terminated octal number that fills `field`.
fn put_octal(field: &mut [u8], value: u64) -> Result<()> {
    let digits = format!("{value:o}");
    let width = field.len() - 1;
    if digits.len() > width {
        bail!("value {value} does not fit a tar header field");
    }
    let padded = format!("{digits:0>width$}");
    field[..width].copy_from_slice(padded.as_bytes());
    field[width] = 0;
    Ok(())
}

/// A dependency, conflict or replacement as xbps reads it. xbps refuses a bare name (`glibc` fails with "can't guess
/// pkgname"), so a name without a version is written `name>=0`. A value that already has an operator, or an exact
/// `name-version_revision`, is kept as it is.
fn pattern(name: &str) -> Value {
    let has_operator = name.contains(['<', '>', '=', '~']);
    let exact = name
        .rsplit_once('-')
        .is_some_and(|(_, tail)| starts_with_digit(tail));
    text(&if has_operator || exact {
        name.to_string()
    } else {
        format!("{name}>=0")
    })
}
