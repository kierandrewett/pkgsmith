//!
//! `+COMPACT_MANIFEST` is the manifest without the file list, which `pkg search` and the repository catalogue read.
//! `+MANIFEST` is the full manifest, as UCL-compatible JSON, with `files`, `directories`, `config` and `scripts`. The
//! files follow with paths relative to `/`. Scripts are inside the manifest, not separate members.
//! The file checksum is the lowercase hex SHA-256 of the contents. A symlink's checksum is the SHA-256 of its target.
//! Dependencies are recorded with an origin (`category/port`), which pkg needs to resolve them. Directories are listed
//! only as the package declares them, as `pkg create` does.
//!
//! Reference: <https://github.com/freebsd/pkg> (`libpkg/pkg_manifest.c`, `libpkg/packing.c`, `libpkg/pkg_create.c`,
//! `libpkg/pkg_checksum.c`, `libpkg/pkg_abi.c`) and `pkg-create(8)` at <https://man.freebsd.org/cgi/man.cgi?pkg-create>.

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::entry::Data;
use pkgsmith_plugin::hash::digests;
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Entry, Host, Package};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// The `freebsd:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The FreeBSD major version the package is built for, for example `14`. Required: it sets the `abi` field.
    abi_version: Option<u32>,
    /// The port origin `category/port`. Default: `misc/NAME`.
    origin: Option<String>,
    /// The install prefix. Default: `/usr/local`.
    prefix: Option<String>,
    /// The port categories. Default: the category of the origin.
    categories: Option<Vec<String>>,
    /// `zstd` (default), `xz`, `gzip`, `bzip2` or `none`.
    compression: Option<String>,
    /// The origin of each dependency, keyed by the name in `depends`. Every dependency needs one.
    dep_origins: Option<BTreeMap<String, String>>,
}

fn compression(options: &Options) -> Result<Compression> {
    Ok(match options.compression.as_deref().unwrap_or("zstd") {
        "zstd" => Compression::Zstd,
        "xz" => Compression::Xz,
        "gzip" => Compression::Gzip,
        "bzip2" => Compression::Bzip2,
        "none" => Compression::None,
        other => bail!("freebsd.compression {other:?} must be zstd, xz, gzip, bzip2 or none"),
    })
}

/// The `pkg` architecture name: `amd64`, `aarch64`, or `*` for architecture-independent packages.
fn abi_arch(arch: &str) -> Result<&'static str> {
    Ok(match arch {
        "amd64" => "amd64",
        "arm64" => "aarch64",
        "all" => "*",
        other => bail!("architecture {other:?} has no FreeBSD name (amd64, arm64 or all)"),
    })
}

/// `NAME-VERSION_REVISION.pkg`. FreeBSD writes the packaging revision after an underscore, as ports do.
fn pkg_version(package: &Package) -> Result<String> {
    let core = Version::parse(&package.version)?.plain();
    let release = package.release.trim();
    if release.is_empty() {
        return Ok(core);
    }
    if !release.chars().all(|c| c.is_ascii_digit()) {
        bail!(
            "release {release:?} must be a number: pkg writes it as the _REVISION suffix of the version"
        );
    }
    Ok(format!("{core}_{release}"))
}

/// `name-version.pkg`.
pub fn file_name(package: &Package) -> Result<String> {
    Ok(format!("{}-{}.pkg", package.name, pkg_version(package)?))
}

/// The origin `category/port`. The port defaults to the package name in `misc`.
fn origin(package: &Package, options: &Options) -> Result<String> {
    let origin = options
        .origin
        .clone()
        .unwrap_or_else(|| format!("misc/{}", package.name));
    match origin.split_once('/') {
        Some((category, port))
            if !category.is_empty() && !port.is_empty() && !port.contains('/') =>
        {
            Ok(origin)
        }
        _ => bail!("freebsd.origin {origin:?} must be category/port"),
    }
}

/// `pkg` percent-encodes every non-ASCII byte and `%`, and leaves the rest as it is (`urlencode` in `pkg_manifest.c`).
fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if !byte.is_ascii() || byte == b'%' {
            out.push_str(&format!("%{byte:02x}"));
        } else {
            out.push(byte as char);
        }
    }
    out
}

/// The `perm` of a file, as `pkg` prints it: `%#4.4o`, four octal digits with a leading zero.
fn perm(mode: u32) -> String {
    let digits = format!("{:04o}", mode & 0o7777);
    if digits.starts_with('0') {
        digits
    } else {
        format!("0{digits}")
    }
}

/// `pkg` reads `root` as the user and `wheel` as the group of the system, so `root` maps to `wheel` for the group.
fn group(name: &str) -> String {
    if name == "root" {
        "wheel".into()
    } else {
        name.into()
    }
}

/// The checksum of a symlink, as `pkg_checksum_symlink_readlink` computes it: `RELATIVE_PATH(target)` hashed for
/// `target.len()` bytes. With one leading slash that is the rest of the target plus the NUL that ends it. Two leading
/// slashes would make pkg hash the byte after the NUL, so such a target is refused.
fn symlink_sum(target: &str) -> Result<String> {
    if target.starts_with("//") {
        bail!("symlink target {target:?} starts with two slashes: pkg cannot checksum it");
    }
    let bytes = match target.strip_prefix('/') {
        Some(relative) => {
            let mut v = relative.as_bytes().to_vec();
            v.push(0);
            v
        }
        None => target.as_bytes().to_vec(),
    };
    Ok(digests(&mut bytes.as_slice())?.1)
}

fn file_sum(src: &Path) -> Result<String> {
    let mut file = File::open(src).with_context(|| format!("opening {}", src.display()))?;
    Ok(digests(&mut file)?.1)
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let compression = compression(&options)?;
    let name = file_name(package)?;
    let abi_version = options
        .abi_version
        .context("freebsd.abi_version is required: the FreeBSD major version, for example 14")?;
    let arch = abi_arch(&package.arch)?;
    let origin = origin(package, &options)?;
    let prefix = options
        .prefix
        .clone()
        .unwrap_or_else(|| "/usr/local".into());
    let categories = options
        .categories
        .clone()
        .unwrap_or_else(|| vec![origin.split('/').next().unwrap_or("misc").to_string()]);
    let maintainer = package
        .maintainer
        .clone()
        .context("freebsd needs a maintainer: pkg requires one")?;
    let www = package
        .homepage
        .clone()
        .context("freebsd needs a homepage: pkg records it as www and requires it")?;

    if !package.recommends.is_empty() || !package.suggests.is_empty() {
        host.warn("FreeBSD pkg has no recommends or suggests, so they are ignored");
    }
    if !package.replaces.is_empty() {
        host.warn("FreeBSD pkg has no replaces, so replaces is ignored");
    }

    let mut deps = Map::new();
    for dep in &package.depends {
        let Some(dep_origin) = options.dep_origins.as_ref().and_then(|m| m.get(dep)) else {
            bail!(
                "dependency {dep:?} has no origin: pkg skips a dependency without one, so add it to freebsd.dep_origins"
            );
        };
        if dep_origin.split_once('/').is_none() {
            bail!("freebsd.dep_origins[{dep:?}] {dep_origin:?} must be category/port");
        }
        deps.insert(dep.clone(), json!({ "origin": dep_origin }));
    }

    let mut ghosts = 0usize;
    let entries: Vec<Entry> = package
        .entries
        .iter()
        .filter_map(|e| {
            if matches!(e.data, Data::Ghost) {
                ghosts += 1;
                return None;
            }
            let mut e = e.clone();
            e.group = group(&e.group);
            Some(e)
        })
        .collect();
    if ghosts > 0 {
        host.warn("FreeBSD pkg has no ghost files, so the entries marked ghost are left out");
    }

    let mut files = Map::new();
    let mut directories = Map::new();
    let mut config = Vec::new();
    let mut flatsize: u64 = 0;
    for entry in &entries {
        let path = urlencode(&entry.path);
        let attributes = |sum: Option<String>| {
            let mut m = Map::new();
            if let Some(sum) = sum {
                m.insert("sum".into(), Value::String(sum));
            }
            m.insert("uname".into(), Value::String(entry.owner.clone()));
            m.insert("gname".into(), Value::String(entry.group.clone()));
            m.insert("perm".into(), Value::String(perm(entry.mode)));
            m
        };
        match &entry.data {
            Data::File { src, size } => {
                flatsize += size;
                files.insert(
                    path.clone(),
                    Value::Object(attributes(Some(file_sum(src)?))),
                );
                if entry.config {
                    config.push(Value::String(path));
                }
            }
            Data::Symlink { target } => {
                flatsize += target.len() as u64;
                let mut m = attributes(Some(symlink_sum(target)?));
                m.insert("symlink_target".into(), Value::String(target.clone()));
                files.insert(path, Value::Object(m));
            }
            Data::Dir => {
                directories.insert(path, Value::Object(attributes(None)));
            }
            Data::Ghost => {}
        }
    }

    let mut top = Map::new();
    top.insert("name".into(), json!(package.name));
    top.insert("origin".into(), json!(origin));
    top.insert("version".into(), json!(pkg_version(package)?));
    top.insert("comment".into(), json!(package.summary()));
    top.insert("maintainer".into(), json!(maintainer));
    top.insert("www".into(), json!(www));
    top.insert("abi".into(), json!(format!("FreeBSD:{abi_version}:{arch}")));
    top.insert(
        "arch".into(),
        json!(format!("freebsd:{abi_version}:{arch}")),
    );
    top.insert("prefix".into(), json!(prefix));
    top.insert("flatsize".into(), json!(flatsize));
    top.insert("desc".into(), json!(urlencode(&package.description)));
    top.insert("categories".into(), json!(categories));
    if let Some(license) = &package.license {
        top.insert("licenses".into(), json!([license]));
        top.insert("licenselogic".into(), json!("single"));
    }
    if !deps.is_empty() {
        top.insert("deps".into(), Value::Object(deps));
    }
    if !package.conflicts.is_empty() {
        top.insert("conflicts".into(), json!(package.conflicts));
    }
    if !package.provides.is_empty() {
        top.insert("provides".into(), json!(package.provides));
    }

    // The compact manifest has no file list and no scripts.
    let compact = serde_json::to_vec(&Value::Object(top.clone()))?;

    let mut full = top;
    if !files.is_empty() {
        full.insert("files".into(), Value::Object(files));
    }
    if !config.is_empty() {
        full.insert("config".into(), Value::Array(config));
    }
    if !directories.is_empty() {
        full.insert("directories".into(), Value::Object(directories));
    }
    let mut scripts = Map::new();
    for (key, text) in [
        ("pre-install", &package.scripts.pre_install),
        ("post-install", &package.scripts.post_install),
        ("pre-deinstall", &package.scripts.pre_remove),
        ("post-deinstall", &package.scripts.post_remove),
    ] {
        if let Some(text) = text {
            scripts.insert(key.into(), json!(urlencode(text)));
        }
    }
    if !scripts.is_empty() {
        full.insert("scripts".into(), Value::Object(scripts));
    }
    let full = serde_json::to_vec(&Value::Object(full))?;

    let leading = [
        Leading {
            name: "+COMPACT_MANIFEST".into(),
            data: compact,
            mode: 0o644,
        },
        Leading {
            name: "+MANIFEST".into(),
            data: full,
            mode: 0o644,
        },
    ];
    let tar = archive::tar(&entries, "", package.mtime, &leading)?;
    let file = package.output(&name);
    std::fs::write(&file, compression.compress(&tar)?)?;
    host.info(&format!("wrote {}", file.display()));
    Ok(file)
}
