//! conda packages: `.conda` (default) or the older `.tar.bz2`, for conda and micromamba.
//!
//! A `.conda` is an uncompressed zip holding `metadata.json` (`{"conda_pkg_format_version": 2}`), the files as
//! `pkg-NAME-VERSION-BUILD.tar.zst` and the `info/` folder as `info-NAME-VERSION-BUILD.tar.zst`. The files sit relative
//! to the conda prefix. `info/` holds `index.json`, `paths.json`, `files` and `about.json`. A `.tar.bz2` is one bzip2
//! tar with the files and `info/` in it.
//! Reference: <https://docs.conda.io/projects/conda-build/en/stable/resources/package-spec.html> and the `.conda` format
//! of CEP-0019 as conda-package-handling writes it. The layout was checked against a package built by this plugin and
//! installed with micromamba (see the report).

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::entry::{self, Data, Entry};
use pkgsmith_plugin::hash;
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use zip::write::SimpleFileOptions;

/// How many links a link may follow inside the package before its checksum is left out.
const MAX_LINKS: usize = 8;

/// The `conda:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// `conda` (default, a `.conda` zip) or `tar.bz2` (the older bzip2 tar).
    archive: Option<String>,
    /// The folder the files install into, which becomes the conda prefix. Default: `/usr`. Files and links outside it
    /// are an error.
    prefix: Option<String>,
    /// The build number. Default: the release, which must then be a number.
    build_number: Option<u64>,
    /// The build string, letters, digits, `_`, `.` and `+`. Default: the build number.
    build_string: Option<String>,
    /// The conda dependencies as match specs, for example `zlib >=1.2`.
    #[serde(default)]
    depends: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Archive {
    Conda,
    TarBz2,
}

impl Archive {
    fn parse(options: &Options) -> Result<Archive> {
        match options.archive.as_deref().unwrap_or("conda") {
            "conda" => Ok(Archive::Conda),
            "tar.bz2" => Ok(Archive::TarBz2),
            other => bail!("archive {other:?} must be conda or tar.bz2"),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Archive::Conda => "conda",
            Archive::TarBz2 => "tar.bz2",
        }
    }
}

/// The name, version and build that name every file of the package.
struct Identity {
    name: String,
    version: String,
    build: String,
    build_number: u64,
}

impl Identity {
    fn new(package: &Package, options: &Options) -> Result<Identity> {
        let build_number = match options.build_number {
            Some(number) => number,
            None => package.release.parse().with_context(|| {
                format!(
                    "release {:?} is not a number, so set build_number in the conda section",
                    package.release
                )
            })?,
        };
        let build = options
            .build_string
            .clone()
            .unwrap_or_else(|| build_number.to_string());
        if build.is_empty()
            || !build
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+'))
        {
            bail!("build string {build:?} may hold only letters, digits, _, . and +");
        }
        Ok(Identity {
            name: package.name.clone(),
            // conda splits the file name at its last two hyphens, so the version cannot hold one either.
            version: Version::parse(&package.version)?.arch(),
            build,
            build_number,
        })
    }

    /// `NAME-VERSION-BUILD`, the stem of the file name and of the inner archives.
    fn stem(&self) -> String {
        format!("{}-{}-{}", self.name, self.version, self.build)
    }
}

/// Where a package goes in a conda channel.
struct Platform {
    subdir: &'static str,
    arch: Option<&'static str>,
    platform: Option<&'static str>,
    noarch: Option<&'static str>,
}

fn platform(arch: &str) -> Result<Platform> {
    Ok(match arch {
        "amd64" => Platform {
            subdir: "linux-64",
            arch: Some("x86_64"),
            platform: Some("linux"),
            noarch: None,
        },
        "arm64" => Platform {
            subdir: "linux-aarch64",
            arch: Some("aarch64"),
            platform: Some("linux"),
            noarch: None,
        },
        "all" => Platform {
            subdir: "noarch",
            arch: None,
            platform: None,
            noarch: Some("generic"),
        },
        other => bail!("architecture {other:?} has no conda subdir"),
    })
}

/// The prefix without a trailing slash, `""` for the root.
fn prefix(options: &Options) -> Result<String> {
    let prefix = options.prefix.as_deref().unwrap_or("/usr");
    if !prefix.starts_with('/') {
        bail!("prefix {prefix:?} must be an absolute path");
    }
    Ok(prefix.trim_end_matches('/').to_string())
}

/// `path` relative to `prefix`. `None` for the prefix itself and for paths outside it.
fn relative(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// `path` without `.` and `..` parts. `None` when a `..` climbs above the start.
fn normalize(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// The target of a link at `link` (an absolute path) that points at `inside` (an absolute path below the prefix),
/// written relative to the folder of the link, the way conda-build keeps links inside a package.
fn relative_target(link: &str, inside: &str) -> String {
    let from: Vec<&str> = link
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let to: Vec<&str> = inside.split('/').filter(|part| !part.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

/// The checksum and size of the file that a link ends at, following links inside the package. `None` when it ends
/// outside the package.
fn resolve(
    link: &str,
    prefix: &str,
    files: &BTreeMap<String, (String, u64)>,
    links: &BTreeMap<String, String>,
) -> Option<(String, u64)> {
    let mut at = link.to_string();
    for _ in 0..MAX_LINKS {
        let target = links.get(&at)?;
        let next = if target.starts_with('/') {
            normalize(&relative(target, prefix)?)?
        } else {
            let dir = at.rsplit_once('/').map_or("", |(dir, _)| dir);
            normalize(&format!("{dir}/{target}"))?
        };
        if let Some(found) = files.get(&next) {
            return Some(found.clone());
        }
        at = next;
    }
    None
}

/// The entries under the prefix, with paths relative to it, and every folder above them. Folders above the prefix
/// are dropped. Ghost entries are dropped with a warning, as conda does not carry them.
fn payload(package: &Package, prefix: &str, host: &Host) -> Result<Vec<Entry>> {
    let mut kept = Vec::new();
    let mut ghosts = Vec::new();
    for entry in entry::with_parents(&package.entries) {
        if entry.data == Data::Ghost {
            ghosts.push(entry.path);
            continue;
        }
        let mut entry = entry;
        if let Data::Symlink { target } = &entry.data
            && target.starts_with('/')
        {
            match relative(target, prefix) {
                Some(_) => {
                    let target = relative_target(&entry.path, target);
                    entry.data = Data::Symlink { target };
                }
                None => host.warn(&format!(
                    "{} points at {target}, outside the prefix, so the link is kept as written and may not resolve after install",
                    entry.path
                )),
            }
        }
        match relative(&entry.path, prefix) {
            Some(path) => kept.push(Entry { path, ..entry }),
            None if entry.is_dir() => {}
            None => bail!(
                "{} is outside the prefix {}: conda installs files below the prefix, so set the prefix option",
                entry.path,
                if prefix.is_empty() { "/" } else { prefix }
            ),
        }
    }
    if !ghosts.is_empty() {
        host.warn(&format!(
            "conda packages do not carry ghost entries, so these are dropped: {}",
            ghosts.join(", ")
        ));
    }
    Ok(kept)
}

/// The install scripts as conda hooks: `bin/.NAME-pre-link.sh`, `bin/.NAME-post-link.sh` and `bin/.NAME-pre-unlink.sh`.
fn hook_scripts(package: &Package, host: &Host) -> Vec<Leading> {
    let hooks = [
        (&package.scripts.pre_install, "pre-link"),
        (&package.scripts.post_install, "post-link"),
        (&package.scripts.pre_remove, "pre-unlink"),
    ];
    let mut leading = Vec::new();
    for (script, hook) in hooks {
        if let Some(text) = script {
            leading.push(Leading {
                name: format!("bin/.{}-{hook}.sh", package.name),
                data: text.clone().into_bytes(),
                mode: 0o755,
            });
        }
    }
    if package.scripts.post_remove.is_some() {
        host.warn("conda runs no script after a package is removed, so post_remove is ignored");
    }
    leading
}

#[derive(Debug, Serialize)]
struct PathRecord {
    #[serde(rename = "_path")]
    path: String,
    path_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_in_bytes: Option<u64>,
}

#[derive(Serialize)]
struct PathsFile<'a> {
    paths: &'a [PathRecord],
    paths_version: u8,
}

/// Every regular file and link of the package, with the SHA-256 of its bytes. A link carries the checksum and size of
/// the file it ends at, as conda-build writes them.
fn path_records(
    entries: &[Entry],
    scripts: &[Leading],
    prefix: &str,
    host: &Host,
) -> Result<Vec<PathRecord>> {
    let mut files: BTreeMap<String, (String, u64)> = BTreeMap::new();
    let mut links: BTreeMap<String, String> = BTreeMap::new();
    for entry in entries {
        match &entry.data {
            Data::File { src, size } => {
                let file =
                    File::open(src).with_context(|| format!("cannot read {}", src.display()))?;
                let (_, sha256) = hash::digests(&mut std::io::BufReader::new(file))?;
                files.insert(entry.path.clone(), (sha256, *size));
            }
            Data::Symlink { target } => {
                links.insert(entry.path.clone(), target.clone());
            }
            Data::Dir | Data::Ghost => {}
        }
    }
    for script in scripts {
        let (_, sha256) = hash::digests(&mut script.data.as_slice())?;
        files.insert(script.name.clone(), (sha256, script.data.len() as u64));
    }

    let mut records: Vec<PathRecord> = files
        .iter()
        .map(|(path, (sha256, size))| PathRecord {
            path: path.clone(),
            path_type: "hardlink",
            sha256: Some(sha256.clone()),
            size_in_bytes: Some(*size),
        })
        .collect();
    for path in links.keys() {
        let target = resolve(path, prefix, &files, &links);
        if target.is_none() {
            host.warn(&format!(
                "{path} is a link that does not end at a file in the package, so conda records no checksum for it"
            ));
        }
        records.push(PathRecord {
            path: path.clone(),
            path_type: "softlink",
            sha256: target.as_ref().map(|(sha256, _)| sha256.clone()),
            size_in_bytes: target.map(|(_, size)| size),
        });
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(records)
}

#[derive(Serialize)]
struct Index<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    arch: Option<&'static str>,
    build: &'a str,
    build_number: u64,
    depends: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<&'a str>,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    noarch: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<&'static str>,
    subdir: &'static str,
    /// Milliseconds since the epoch.
    timestamp: u64,
    version: &'a str,
}

#[derive(Serialize)]
struct About<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    home: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<&'a str>,
    summary: &'a str,
    description: &'a str,
}

fn json_file(name: &str, value: &impl Serialize) -> Result<Leading> {
    Ok(Leading {
        name: name.into(),
        data: serde_json::to_vec_pretty(value)?,
        mode: 0o644,
    })
}

/// The files of `info/`: `index.json`, `about.json`, `paths.json` and `files`.
fn info_files(
    package: &Package,
    identity: &Identity,
    options: &Options,
    platform: &Platform,
    records: &[PathRecord],
) -> Result<Vec<Leading>> {
    let index = Index {
        arch: platform.arch,
        build: &identity.build,
        build_number: identity.build_number,
        depends: &options.depends,
        license: package.license.as_deref(),
        name: &identity.name,
        noarch: platform.noarch,
        platform: platform.platform,
        subdir: platform.subdir,
        timestamp: package.mtime * 1000,
        version: &identity.version,
    };
    let description = package
        .description
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    let about = About {
        home: package.homepage.as_deref(),
        license: package.license.as_deref(),
        summary: package.summary(),
        description: description.trim(),
    };
    let paths = PathsFile {
        paths: records,
        paths_version: 1,
    };
    let mut files_list: String = records.iter().map(|r| format!("{}\n", r.path)).collect();
    if files_list.is_empty() {
        files_list.push('\n');
    }
    Ok(vec![
        json_file("info/index.json", &index)?,
        json_file("info/about.json", &about)?,
        json_file("info/paths.json", &paths)?,
        Leading {
            name: "info/files".into(),
            data: files_list.into_bytes(),
            mode: 0o644,
        },
    ])
}

/// `NAME-VERSION-BUILD.conda`, or `NAME-VERSION-BUILD.tar.bz2`.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    let archive = Archive::parse(&options)?;
    let identity = Identity::new(package, &options)?;
    Ok(format!("{}.{}", identity.stem(), archive.extension()))
}

fn put(zip: &mut zip::ZipWriter<File>, name: &str, data: &[u8], time: zip::DateTime) -> Result<()> {
    // The inner archives are already compressed, so the zip stores them.
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(time);
    zip.start_file(name, options)?;
    zip.write_all(data)?;
    Ok(())
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let archive = Archive::parse(&options)?;
    let identity = Identity::new(package, &options)?;
    let prefix = prefix(&options)?;
    let platform = platform(&package.arch)?;
    let stem = identity.stem();

    warn_unmapped(package, &options, host);
    let entries = payload(package, &prefix, host)?;
    let hooks = hook_scripts(package, host);
    let records = path_records(&entries, &hooks, &prefix, host)?;
    let info = info_files(package, &identity, &options, &platform, &records)?;
    std::fs::create_dir_all(&package.out)?;
    let mtime = package.mtime;

    match archive {
        Archive::Conda => {
            let pkg = Compression::Zstd.compress(&archive::tar(&entries, "", mtime, &hooks)?)?;
            let info_tar = Compression::Zstd.compress(&archive::tar(&[], "", mtime, &info)?)?;
            let metadata =
                serde_json::to_vec(&serde_json::json!({ "conda_pkg_format_version": 2 }))?;
            // The zip holds fixed 1980 dates, so equal input gives equal bytes.
            let time = zip::DateTime::default();
            let path = package.output(&format!("{stem}.conda"));
            let mut zip = zip::ZipWriter::new(File::create(&path)?);
            put(&mut zip, "metadata.json", &metadata, time)?;
            put(&mut zip, &format!("pkg-{stem}.tar.zst"), &pkg, time)?;
            put(&mut zip, &format!("info-{stem}.tar.zst"), &info_tar, time)?;
            zip.finish()?;
            Ok(path)
        }
        Archive::TarBz2 => {
            let mut leading = info;
            leading.extend(hooks);
            let bytes =
                Compression::Bzip2.compress(&archive::tar(&entries, "", mtime, &leading)?)?;
            let path = package.output(&format!("{stem}.tar.bz2"));
            std::fs::write(&path, bytes)?;
            Ok(path)
        }
    }
}

/// Say what the manifest holds that a conda package cannot record.
fn warn_unmapped(package: &Package, options: &Options, host: &Host) {
    if !package.depends.is_empty() && options.depends.is_empty() {
        host.warn(
            "conda dependencies are match specs such as `zlib >=1.2`, set in the depends option of the conda section; \
             the manifest depends are not used",
        );
    }
    let lists = [
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, values) in lists {
        if !values.is_empty() {
            host.warn(&format!(
                "conda has no {field} field, so {field} is ignored: {}",
                values.join(", ")
            ));
        }
    }
    if package.maintainer.is_some() {
        host.warn("conda has no maintainer field, so the maintainer is not recorded");
    }
    if package.entries.iter().any(|e| e.config) {
        host.warn(
            "conda has no configuration-file flag, so config files are installed like the others",
        );
    }
    if package
        .entries
        .iter()
        .any(|e| e.owner != "root" || e.group != "root")
    {
        host.warn(
            "conda does not record file owners, so files are owned by the user who installs them",
        );
    }
}
