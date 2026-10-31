//! A Python wheel (`.whl`): a zip archive that pip installs into site-packages.
//!
//! Specifications used:
//! - Binary distribution format (wheel file name, root, `.dist-info` and `.data` folders, `WHEEL`):
//!   <https://packaging.python.org/en/latest/specifications/binary-distribution-format/>
//! - Recording installed projects (`RECORD`: `path,sha256=<urlsafe base64, no padding>,size`):
//!   <https://packaging.python.org/en/latest/specifications/recording-installed-packages/>
//! - Core metadata (`METADATA`: `License-Expression`, `Project-URL`, `Requires-Dist`, `Requires-Python`):
//!   <https://packaging.python.org/en/latest/specifications/core-metadata/>
//! - Platform compatibility tags (python, abi and platform tags): <https://packaging.python.org/en/latest/specifications/platform-compatibility-tags/>
//! - Version specifiers (PEP 440, the version in the file name and `METADATA`):
//!   <https://packaging.python.org/en/latest/specifications/version-specifiers/>
//! - Entry points (`entry_points.txt`): <https://packaging.python.org/en/latest/specifications/entry-points/>
//!
//! Version: a release `1.2.3` is `1.2.3`. A nightly `1.2.3-nightly.20261031.142.gabc1234` becomes the dev release
//! `1.2.3.dev202610310142`: a dev release sorts before `1.2.3` and after `1.2.2`, and the date and the build count order
//! the builds. The commit is dropped, because a `+local` label is not allowed in a file name. `1.2.3-rc.1`, `-beta2` and
//! `-a1` become `1.2.3rc1`, `1.2.3b2` and `1.2.3a1`.
//!
//! Layout: a file under `wheel: site_packages` goes to the root of the wheel, which pip installs into site-packages.
//! A file directly in `/usr/bin` or `/usr/local/bin` goes to `{dist}-{version}.data/scripts/`, where pip makes it
//! executable and rewrites `#!python`. Every other file goes to `{dist}-{version}.data/data/` with its path relative to
//! `/`; pip installs that folder under the Python prefix (`sys.prefix`), so the file is not at the root of the system.
//!
//! The release becomes the build tag, the counter of rebuilds of one version. `Root-Is-Purelib` is `true` only for
//! `any` wheels, whose root pip installs into purelib; platform wheels go to platlib.

use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Host, Package};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

/// The `wheel:` section of the manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct Options {
    /// The install path of site-packages on the target Python, for example `/usr/lib/python3.12/site-packages`. Required.
    site_packages: Option<String>,
    /// The Python tag: `py3`, `cp313` and so on, or several joined by dots. Default `py3`.
    python_tag: String,
    /// The ABI tag: `none`, `abi3`, `cp313t` and so on. Default `none`.
    abi_tag: String,
    /// The platform tag. Default `any` for arch `all`. Required for `amd64` and `arm64`, because the right tag
    /// depends on the libc and the glibc or macOS version, and only the author knows them (`manylinux_2_17_x86_64`).
    platform_tag: Option<String>,
    /// `Requires-Python`, a version specifier such as `>=3.9`.
    requires_python: Option<String>,
    /// `Classifier` lines, for example `Programming Language :: Python :: 3`.
    classifiers: Vec<String>,
    /// `Requires-Dist` lines: the Python requirements, for example `requests>=2`.
    requires_dist: Vec<String>,
    /// Entry points, grouped as in `entry_points.txt`: the group name, then the script name and `module:function`.
    /// `console_scripts` is the usual group. Empty groups are left out.
    entry_points: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            site_packages: None,
            python_tag: "py3".into(),
            abi_tag: "none".into(),
            platform_tag: None,
            requires_python: None,
            classifiers: Vec::new(),
            requires_dist: Vec::new(),
            entry_points: BTreeMap::new(),
        }
    }
}

/// The compatibility tags of the wheel, as written in the file name. A compressed set such as `py2.py3` is one value.
struct Tags {
    python: String,
    abi: String,
    platform: String,
}

impl Tags {
    /// One `python-abi-platform` triple for every combination of a compressed set, as the `Tag:` lines of `WHEEL` need.
    fn expanded(&self) -> Vec<String> {
        let mut tags = Vec::new();
        for python in self.python.split('.') {
            for abi in self.abi.split('.') {
                for platform in self.platform.split('.') {
                    tags.push(format!("{python}-{abi}-{platform}"));
                }
            }
        }
        tags
    }
}

/// Everything the names of the wheel and its folders are made of.
struct Naming {
    dist: String,
    version: String,
    build: Option<String>,
    tags: Tags,
}

impl Naming {
    /// `{dist}-{version}(-{build})?-{python}-{abi}-{platform}.whl`
    fn file_name(&self) -> String {
        let build = self
            .build
            .as_ref()
            .map(|b| format!("-{b}"))
            .unwrap_or_default();
        format!(
            "{}-{}{build}-{}-{}-{}.whl",
            self.dist, self.version, self.tags.python, self.tags.abi, self.tags.platform
        )
    }

    fn dist_info(&self) -> String {
        format!("{}-{}.dist-info", self.dist, self.version)
    }

    fn data(&self) -> String {
        format!("{}-{}.data", self.dist, self.version)
    }
}

/// Civil date and time of a Unix time, in UTC. A zip cannot hold a date before 1980, so earlier times become 1980.
fn civil(seconds: u64) -> (u16, u8, u8, u8, u8, u8) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u8;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u8;
    let year = (year_of_era + era * 400 + i64::from(month <= 2)) as u16;
    if year < 1980 {
        return (1980, 1, 1, 0, 0, 0);
    }
    (
        year,
        month,
        day,
        (rest / 3600) as u8,
        (rest % 3600 / 60) as u8,
        (rest % 60) as u8,
    )
}

/// `pkgname-version-py3-none-any.whl`.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    Ok(naming(package, &options)?.file_name())
}

fn naming(package: &Package, options: &Options) -> Result<Naming> {
    let tags = Tags {
        python: tag("python_tag", &options.python_tag)?,
        abi: tag("abi_tag", &options.abi_tag)?,
        platform: platform(options, &package.arch)?,
    };
    Ok(Naming {
        dist: normalize_name(&package.name)?,
        version: pep440(&package.version)?,
        build: build_tag(&package.release)?,
        tags,
    })
}

/// A distribution name as the file name spells it: lowercase, and each run of `-`, `_` or `.` becomes one `_`.
fn normalize_name(name: &str) -> Result<String> {
    let edge = |c: char| c.is_ascii_alphanumeric();
    ensure!(
        name.starts_with(edge)
            && name.ends_with(edge)
            && name
                .chars()
                .all(|c| edge(c) || matches!(c, '-' | '_' | '.')),
        "the package name {name:?} is not a Python distribution name: letters, digits, '-', '_' and '.', starting and ending with a letter or digit"
    );
    let mut out = String::new();
    let mut in_run = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !in_run {
                out.push('_');
            }
            in_run = true;
        } else {
            out.push(c.to_ascii_lowercase());
            in_run = false;
        }
    }
    Ok(out)
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// The version of the package in PEP 440. See the module comment for the mapping.
fn pep440(version: &str) -> Result<String> {
    let parsed = Version::parse(version)?;
    let Some(pre) = parsed.pre.as_deref() else {
        return Ok(parsed.core);
    };
    let parts: Vec<&str> = pre.split('.').collect();
    if let ["nightly", date, build, commit @ ..] = parts.as_slice() {
        let commit_ok = match commit {
            [] => true,
            [commit] => commit.strip_prefix('g').is_some_and(|hex| {
                (7..=40).contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit())
            }),
            _ => false,
        };
        ensure!(
            commit_ok && date.len() == 8 && is_digits(date) && is_digits(build) && build.len() <= 4,
            "version {version:?}: a nightly is MAJOR.MINOR.PATCH-nightly.YYYYMMDD.N[.gCOMMIT], with N of at most 4 digits"
        );
        let build: u32 = build.parse()?;
        return Ok(format!("{}.dev{date}{build:04}", parsed.core));
    }
    let compact: String = pre.chars().filter(|c| *c != '.').collect();
    let letters: String = compact
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let digits = &compact[letters.len()..];
    let label = match letters.as_str() {
        "a" | "alpha" => "a",
        "b" | "beta" => "b",
        "rc" => "rc",
        _ => "",
    };
    ensure!(
        !label.is_empty() && is_digits(digits),
        "version {version:?} has no PEP 440 form: use MAJOR.MINOR.PATCH, MAJOR.MINOR.PATCH-nightly.YYYYMMDD.N, or a pre-release such as -a1, -b1 or -rc1"
    );
    let number: u32 = digits.parse()?;
    Ok(format!("{}{label}{number}", parsed.core))
}

/// A compatibility tag. Letters, digits and `_`; dots join the tags of a compressed set.
fn tag(field: &str, value: &str) -> Result<String> {
    let valid = |part: &str| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    ensure!(
        value.split('.').all(valid),
        "wheel: {field} {value:?} must be letters, digits and '_', with dots between the tags of a set"
    );
    Ok(value.to_string())
}

fn platform(options: &Options, arch: &str) -> Result<String> {
    match (&options.platform_tag, arch) {
        (Some(tag_text), _) => tag("platform_tag", tag_text),
        (None, "all") => Ok("any".into()),
        (None, arch) => bail!(
            "wheel: set platform_tag for the {arch} package, for example manylinux_2_17_x86_64 for glibc Linux (the platform tag is not a default for {arch})"
        ),
    }
}

/// The release as the build tag. It counts the rebuilds of one version, so it orders like a build tag: it starts with a
/// digit.
fn build_tag(release: &str) -> Result<Option<String>> {
    if release.is_empty() {
        return Ok(None);
    }
    ensure!(
        release.starts_with(|c: char| c.is_ascii_digit())
            && release
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_')),
        "release {release:?} is not a wheel build tag: it must start with a digit"
    );
    Ok(Some(release.to_string()))
}

/// The install path of site-packages, without a trailing slash.
fn site_packages(options: &Options) -> Result<&str> {
    let site = options
        .site_packages
        .as_deref()
        .context("the wheel section needs site_packages: the install path of site-packages, for example /usr/lib/python3.12/site-packages")?
        .trim_end_matches('/');
    ensure!(
        site.starts_with('/') && site.len() > 1,
        "wheel: site_packages {site:?} must be an absolute path"
    );
    Ok(site)
}

/// Where a file of the package goes in the wheel.
#[derive(Debug, PartialEq, Eq)]
enum Place {
    SitePackages,
    Scripts,
    Data,
}

/// The name of a file in the wheel, from its path on the installed system.
fn archive_name(path: &str, site: &str, data_dir: &str) -> (Place, String) {
    if let Some(rest) = path
        .strip_prefix(site)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        return (Place::SitePackages, rest.to_string());
    }
    if let Some((dir, base)) = path.rsplit_once('/')
        && (dir == "/usr/bin" || dir == "/usr/local/bin")
    {
        return (Place::Scripts, format!("{data_dir}/scripts/{base}"));
    }
    (
        Place::Data,
        format!("{data_dir}/data/{}", path.trim_start_matches('/')),
    )
}

/// A file of the wheel: its name in the archive and the file it comes from.
struct Member<'a> {
    name: String,
    src: &'a Path,
    mode: u32,
}

/// Warn about the manifest fields that a wheel cannot carry.
fn warn_unmapped(package: &Package, host: &Host) {
    let relations = [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, names) in relations {
        if !names.is_empty() {
            host.warn(&format!(
                "wheel cannot express the distro {field} ({}): they are not in the wheel. A Python requirement goes in wheel: requires_dist",
                names.join(", ")
            ));
        }
    }
    let scripts = [
        ("pre_install", &package.scripts.pre_install),
        ("post_install", &package.scripts.post_install),
        ("pre_remove", &package.scripts.pre_remove),
        ("post_remove", &package.scripts.post_remove),
    ];
    let present: Vec<&str> = scripts
        .into_iter()
        .filter(|(_, script)| script.is_some())
        .map(|(name, _)| name)
        .collect();
    if !present.is_empty() {
        host.warn(&format!(
            "wheel cannot run install or remove scripts ({}): they are not in the wheel",
            present.join(", ")
        ));
    }
}

/// The files of the wheel, in package order. Warns about what the wheel cannot keep. Fails on a symbolic link, which
/// pip does not install, and on two files with one name in the wheel.
fn plan<'a>(
    package: &'a Package,
    site: &str,
    data_dir: &str,
    host: &Host,
) -> Result<Vec<Member<'a>>> {
    let mut members = Vec::new();
    let mut names = BTreeSet::new();
    // Every folder that holds an entry, so that an empty folder can be reported.
    let mut folders_with_files: BTreeSet<&str> = BTreeSet::new();
    let mut outside = Vec::new();
    let mut configs = Vec::new();
    for entry in &package.entries {
        let src = match &entry.data {
            Data::Ghost | Data::Dir => continue,
            Data::Symlink { .. } => bail!(
                "{} is a symbolic link, and a wheel cannot carry one: pip writes it as a plain file that holds the link target",
                entry.path
            ),
            Data::File { src, .. } => src,
        };
        let (place, name) = archive_name(&entry.path, site, data_dir);
        if place == Place::Data {
            outside.push(entry.path.as_str());
        }
        if entry.config {
            configs.push(entry.path.as_str());
        }
        let mut folder = entry.path.as_str();
        while let Some((parent, _)) = folder.rsplit_once('/') {
            if !folders_with_files.insert(parent) {
                break;
            }
            folder = parent;
        }
        ensure!(
            names.insert(name.clone()),
            "two files of the package go to {name} in the wheel"
        );
        members.push(Member {
            name,
            src,
            mode: entry.mode,
        });
    }
    for entry in &package.entries {
        if entry.is_dir() && !folders_with_files.contains(entry.path.as_str()) {
            host.warn(&format!(
                "the folder {} holds no files, and a wheel holds files only: it is not in the wheel",
                entry.path
            ));
        }
    }
    if let Some(first) = outside.first() {
        host.warn(&format!(
            "{} file(s) are outside site-packages and the bin folders: they go to {data_dir}/data/, which pip installs under the Python prefix, not at /. First: {first}",
            outside.len()
        ));
    }
    if let Some(first) = configs.first() {
        host.warn(&format!(
            "wheels have no configuration files: {first} is installed as a plain file, and an upgrade overwrites changes to it"
        ));
    }
    Ok(members)
}

/// Write the wheel into `package.out` and return its path.
pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let naming = naming(package, &options)?;
    let site = site_packages(&options)?;
    warn_unmapped(package, host);
    let members = plan(package, site, &naming.data(), host)?;
    let (year, month, day, hour, minute, second) = civil(package.mtime);
    let time = zip::DateTime::from_date_and_time(year, month, day, hour, minute, second)
        .map_err(|_| anyhow::anyhow!("the build time is not a valid zip date"))?;

    std::fs::create_dir_all(&package.out)?;
    let path = package.output(&naming.file_name());
    let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(&path)?));
    let mut record = String::new();
    for member in &members {
        let mut file = File::open(member.src)
            .with_context(|| format!("cannot read {}", member.src.display()))?;
        let (digest, size) = put(&mut zip, time, &member.name, member.mode, &mut file)?;
        record_line(&mut record, &member.name, &digest, size);
    }

    let dist_info = naming.dist_info();
    let mut metadata_files = vec![
        (
            format!("{dist_info}/METADATA"),
            metadata(package, &options, &naming)?,
        ),
        (format!("{dist_info}/WHEEL"), wheel_file(&naming)),
    ];
    let entry_points = entry_points_text(&options.entry_points)?;
    if !entry_points.is_empty() {
        metadata_files.push((format!("{dist_info}/entry_points.txt"), entry_points));
    }
    for (name, text) in &metadata_files {
        let (digest, size) = put(&mut zip, time, name, 0o644, &mut text.as_bytes())?;
        record_line(&mut record, name, &digest, size);
    }

    // RECORD lists every other file, and not itself: its own digest and size are empty.
    let record_name = format!("{dist_info}/RECORD");
    record.push_str(&format!("{},,\n", csv_field(&record_name)));
    put(&mut zip, time, &record_name, 0o644, &mut record.as_bytes())?;

    let mut file = zip.finish()?;
    file.flush()?;
    Ok(path)
}

/// Write one file into the archive, deflated, and return its SHA-256 and size in bytes. Read once, in chunks.
fn put<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    time: zip::DateTime,
    name: &str,
    mode: u32,
    data: &mut impl Read,
) -> Result<([u8; 32], u64)> {
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(mode)
        .last_modified_time(time);
    zip.start_file(name, options)?;
    let mut sha = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let read = data.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha.update(&buffer[..read]);
        zip.write_all(&buffer[..read])?;
        size += read as u64;
    }
    Ok((sha.finalize().into(), size))
}

/// One `RECORD` line: the path, `sha256=` and the digest in urlsafe base64 without padding, and the size.
fn record_line(record: &mut String, path: &str, digest: &[u8], size: u64) {
    let digest = URL_SAFE_NO_PAD.encode(digest);
    record.push_str(&format!("{},sha256={digest},{size}\n", csv_field(path)));
}

/// A CSV field: quoted with doubled quotes when it holds a comma, a quote or a line break.
fn csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// One `Key: value` header line. A line break in the value would start a new header, so it is an error.
fn header(out: &mut String, key: &str, value: &str) -> Result<()> {
    ensure!(
        !value.contains(['\n', '\r']),
        "the {key} field has a line break"
    );
    out.push_str(&format!("{key}: {value}\n"));
    Ok(())
}

/// `Name <email>` as the name and the email.
fn split_maintainer(text: &str) -> (&str, Option<&str>) {
    if let Some((name, rest)) = text.rsplit_once('<')
        && let Some(email) = rest.strip_suffix('>')
    {
        return (name.trim(), Some(email.trim()));
    }
    (text.trim(), None)
}

/// `METADATA`, Metadata-Version 2.4 (the first version with `License-Expression`). The first line of the description is
/// the summary, and the rest is the body after a blank line.
fn metadata(package: &Package, options: &Options, naming: &Naming) -> Result<String> {
    let mut out = String::from("Metadata-Version: 2.4\n");
    header(&mut out, "Name", &package.name)?;
    header(&mut out, "Version", &naming.version)?;
    let summary = package.summary();
    if !summary.is_empty() {
        header(&mut out, "Summary", summary)?;
    }
    if let Some(license) = non_empty(package.license.as_deref()) {
        header(&mut out, "License-Expression", license)?;
    }
    if let Some(maintainer) = non_empty(package.maintainer.as_deref()) {
        let (name, email) = split_maintainer(maintainer);
        if !name.is_empty() {
            header(&mut out, "Maintainer", name)?;
        }
        if let Some(email) = email {
            header(&mut out, "Maintainer-email", email)?;
        }
    }
    if let Some(homepage) = non_empty(package.homepage.as_deref()) {
        header(&mut out, "Project-URL", &format!("Homepage, {homepage}"))?;
    }
    if let Some(requires_python) = non_empty(options.requires_python.as_deref()) {
        header(&mut out, "Requires-Python", requires_python)?;
    }
    for classifier in &options.classifiers {
        header(&mut out, "Classifier", classifier)?;
    }
    for requirement in &options.requires_dist {
        header(&mut out, "Requires-Dist", requirement)?;
    }
    let body = package
        .description
        .split_once('\n')
        .map_or("", |(_, rest)| rest.trim());
    if !body.is_empty() {
        header(&mut out, "Description-Content-Type", "text/plain")?;
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    Ok(out)
}

/// `WHEEL`: the wheel format version, the generator, where the root goes, and the tags and build tag.
fn wheel_file(naming: &Naming) -> String {
    let mut out = format!(
        "Wheel-Version: 1.0\nGenerator: pkgsmith-plugin-wheel {}\nRoot-Is-Purelib: {}\n",
        env!("CARGO_PKG_VERSION"),
        naming.tags.platform == "any"
    );
    for tag in naming.tags.expanded() {
        out.push_str(&format!("Tag: {tag}\n"));
    }
    if let Some(build) = &naming.build {
        out.push_str(&format!("Build: {build}\n"));
    }
    out
}

/// `entry_points.txt`, or the empty string when there are no entry points.
fn entry_points_text(groups: &BTreeMap<String, BTreeMap<String, String>>) -> Result<String> {
    let mut out = String::new();
    for (group, scripts) in groups.iter().filter(|(_, scripts)| !scripts.is_empty()) {
        ensure!(
            !group.is_empty() && !group.contains(['[', ']', '\n', '\r']),
            "wheel: entry point group {group:?} is not a valid name"
        );
        out.push_str(&format!("[{group}]\n"));
        for (name, target) in scripts {
            ensure!(
                !name.is_empty()
                    && !name.contains(['=', '\n', '\r'])
                    && !target.contains(['\n', '\r']),
                "wheel: entry point {name:?} in [{group}] is not a valid name = target pair"
            );
            out.push_str(&format!("{name} = {target}\n"));
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_is_a_pep440_version() {
        assert_eq!(pep440("1.2.3").unwrap(), "1.2.3");
        assert_eq!(
            pep440("1.2.3-nightly.20261031.142.gabc1234").unwrap(),
            "1.2.3.dev202610310142"
        );
        assert_eq!(
            pep440("1.2.3-nightly.20261031.7").unwrap(),
            "1.2.3.dev202610310007"
        );
        assert_eq!(pep440("1.2.3-rc.1").unwrap(), "1.2.3rc1");
        assert_eq!(pep440("1.2.3-beta2").unwrap(), "1.2.3b2");
        assert_eq!(pep440("1.2.3-alpha.1").unwrap(), "1.2.3a1");
        assert!(pep440("1.2.3-hotfix").is_err());
        assert!(pep440("1.2.3-nightly.2026103.142").is_err());
    }

    #[test]
    fn a_nightly_sorts_after_the_previous_release_and_before_the_next() {
        // PEP 440 order: 1.2.2 < 1.2.3.dev202610310142 < 1.2.3.dev202611010005 < 1.2.3.
        let build = |date: &str, n: &str| pep440(&format!("1.2.3-nightly.{date}.{n}")).unwrap();
        assert!(build("20261031", "142") < build("20261101", "5"));
        assert!(build("20261031", "999") < build("20261031", "1000"));
    }

    #[test]
    fn names_are_normalized() {
        assert_eq!(normalize_name("My_Pkg.Name").unwrap(), "my_pkg_name");
        assert_eq!(normalize_name("a--b").unwrap(), "a_b");
        assert!(normalize_name("-bad").is_err());
    }

    #[test]
    fn record_digests_are_urlsafe_base64_without_padding() {
        let mut record = String::new();
        record_line(&mut record, "a.py", &Sha256::digest(b"abc"), 3);
        assert_eq!(
            record,
            "a.py,sha256=ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0,3\n"
        );
    }

    #[test]
    fn record_fields_with_commas_are_quoted() {
        assert_eq!(csv_field("a,b.py"), "\"a,b.py\"");
        assert_eq!(csv_field("say \"hi\".py"), "\"say \"\"hi\"\".py\"");
    }

    #[test]
    fn files_map_to_the_root_scripts_or_data() {
        let site = "/usr/lib/python3.12/site-packages";
        let data = "pkg-1.0.data";
        assert_eq!(
            archive_name(
                "/usr/lib/python3.12/site-packages/pkg/__init__.py",
                site,
                data
            ),
            (Place::SitePackages, "pkg/__init__.py".into())
        );
        assert_eq!(
            archive_name("/usr/bin/pkg", site, data),
            (Place::Scripts, "pkg-1.0.data/scripts/pkg".into())
        );
        assert_eq!(
            archive_name("/usr/share/man/man1/pkg.1", site, data),
            (
                Place::Data,
                "pkg-1.0.data/data/usr/share/man/man1/pkg.1".into()
            )
        );
        assert_eq!(
            archive_name("/usr/lib/python3.12/site-packagesX/x.py", site, data).0,
            Place::Data
        );
    }

    #[test]
    fn maintainer_splits_into_name_and_email() {
        assert_eq!(
            split_maintainer("Kieran <k@example.dev>"),
            ("Kieran", Some("k@example.dev"))
        );
        assert_eq!(split_maintainer("Kieran"), ("Kieran", None));
    }

    #[test]
    fn unix_times_become_calendar_dates_and_old_ones_clamp_to_1980() {
        assert_eq!(civil(1_793_482_000), (2026, 10, 31, 21, 26, 40));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(civil(0), (1980, 1, 1, 0, 0, 0));
    }
}
