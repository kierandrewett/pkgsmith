//! A RubyGems package (`.gem`): an uncompressed tar of the spec, the files and their checksums.
//!
//! Specifications used:
//! - The `.gem` file (an uncompressed tar of `metadata.gz`, `data.tar.gz` and `checksums.yaml.gz`) and the checksum
//!   file (SHA256 and SHA512 of the two gzipped members): <https://github.com/rubygems/rubygems/blob/master/lib/rubygems/package.rb>
//! - The specification fields and the YAML form of `metadata.gz` (`Gem::Specification`, version 4, with the Ruby
//!   object tags): <https://guides.rubygems.org/specification-reference/>
//! - Versions (`Gem::Version`: a pre-release has an alphabetic segment, so `1.2.3-nightly.X` becomes
//!   `1.2.3.pre.nightly.X`): <https://github.com/rubygems/rubygems/blob/master/lib/rubygems/version.rb>
//!
//! Layout: a file under `gem: prefix` (default `/usr/share/gems/gems/NAME-VERSION`) goes into `data.tar.gz` with its
//! path relative to the prefix, which is the gem root. A file directly in `gem: executable_dir` (default `/usr/bin`)
//! goes to `bin/NAME` and is listed in `executables`; RubyGems writes the wrapper script when the gem is installed.
//! Any other file outside the prefix fails the build, because a gem cannot place it.
//!
//! Version: the release `X.Y.Z` is `X.Y.Z`. A pre-release `X.Y.Z-pre` is `X.Y.Z.pre.pre`, with `-` changed to `.`.
//! Gems have no release counter, so a release other than `1` is reported and not written.
//!
//! Timestamps: RubyGems writes the spec `date` as midnight UTC of the day. Member mtimes are `package.mtime`. The
//! gzip header time is zero, so equal input gives equal bytes.

use anyhow::{Context, Result, bail, ensure};
use pkgsmith_plugin::archive::{self, Compression, Leading, TarStyle};
use pkgsmith_plugin::hash::hex;
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Entry, Host, Package};
use serde::Deserialize;
use sha2::{Digest, Sha256, Sha512};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The RubyGems version written into the spec. Gem reads it only to report the writer.
const RUBYGEMS_VERSION: &str = "3.5.22";

/// The `gem:` section of the manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct Options {
    /// The install path of the gem root, for example `/usr/share/gems/gems/demo-1.0.0`. Default
    /// `/usr/share/gems/gems/NAME-VERSION`, with the RubyGems version of the package.
    prefix: Option<String>,
    /// Files directly in this folder are the executables of the gem. Default `/usr/bin`.
    executable_dir: String,
    /// `require_paths`: the folders under the gem root that `require` searches. Default `["lib"]`.
    require_paths: Vec<String>,
    /// `required_ruby_version`, a requirement such as `>= 2.7`. Default `>= 0`, which is any Ruby.
    required_ruby_version: Option<String>,
    /// The runtime dependencies: the gem name and its requirement, for example `rake: "~> 13.0, >= 13.0.1"`. A
    /// requirement may hold several comma-separated constraints. An empty requirement means any version.
    dependencies: BTreeMap<String, String>,
    /// The platform: `ruby` for a gem that runs on any machine, or a platform such as `x86_64-linux` for compiled
    /// files. Default `ruby`. It is part of the file name.
    platform: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            prefix: None,
            executable_dir: "/usr/bin".into(),
            require_paths: vec!["lib".into()],
            required_ruby_version: None,
            dependencies: BTreeMap::new(),
            platform: "ruby".into(),
        }
    }
}

/// The RubyGems version of the package: `1.2.3`, or `1.2.3.pre.nightly.X` for a pre-release.
fn gem_version(package: &Package) -> Result<String> {
    let version = Version::parse(&package.version)?;
    Ok(match &version.pre {
        None => version.core.clone(),
        Some(pre) => format!("{}.pre.{}", version.core, pre.replace('-', ".")),
    })
}

/// The RubyGems version and the platform of the package.
fn identity(package: &Package, options: &Options) -> Result<(String, String)> {
    ensure!(
        !options.platform.is_empty()
            && options
                .platform
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
        "gem.platform {:?} must be a platform such as ruby or x86_64-linux",
        options.platform
    );
    Ok((gem_version(package)?, options.platform.clone()))
}

pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    let (version, platform) = identity(package, &options)?;
    Ok(if platform == "ruby" {
        format!("{}-{version}.gem", package.name)
    } else {
        format!("{}-{version}-{platform}.gem", package.name)
    })
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let (version, platform) = identity(package, &options)?;
    warn_unexpressed(package, &platform, host);

    let prefix = options
        .prefix
        .clone()
        .unwrap_or_else(|| format!("/usr/share/gems/gems/{}-{version}", package.name));
    let prefix = prefix.trim_end_matches('/').to_string();
    ensure!(
        prefix.starts_with('/') && prefix.len() > 1,
        "gem.prefix {prefix:?} must be an absolute path"
    );
    let executable_dir = options.executable_dir.trim_end_matches('/').to_string();
    ensure!(
        executable_dir.starts_with('/'),
        "gem.executable_dir {executable_dir:?} must be an absolute path"
    );
    ensure!(
        !options.require_paths.is_empty()
            && options
                .require_paths
                .iter()
                .all(|p| !p.is_empty() && !p.starts_with('/') && !p.split('/').any(|s| s == "..")),
        "gem.require_paths must be relative paths under the gem root"
    );

    let (files, executables) = place(package, &prefix, &executable_dir)?;
    let paths: Vec<String> = files.iter().map(|e| e.path.clone()).collect();
    let spec = specification(package, &options, &version, &platform, &paths, &executables)?;

    let mtime = package.mtime;
    let metadata_gz = Compression::Gzip.compress(spec.as_bytes())?;
    let data_gz = Compression::Gzip.compress(&archive::tar_styled(
        &files,
        "",
        mtime,
        &[],
        TarStyle::default(),
    )?)?;
    let checksums_gz = Compression::Gzip.compress(checksums(&metadata_gz, &data_gz).as_bytes())?;

    let leading = [
        ("metadata.gz", metadata_gz),
        ("data.tar.gz", data_gz),
        ("checksums.yaml.gz", checksums_gz),
    ]
    .into_iter()
    .map(|(name, data)| Leading {
        name: name.into(),
        data,
        mode: 0o444,
    })
    .collect::<Vec<_>>();
    let gem = archive::tar_styled(&[], "", mtime, &leading, TarStyle::default())?;

    let path = package.output(&file_name(package)?);
    std::fs::write(&path, gem).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// The manifest fields and entry kinds that a gem cannot express. Each one is named, so no meaning goes silently.
fn warn_unexpressed(package: &Package, platform: &str, host: &Host) {
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
                "gem cannot express {field}; not written: {}",
                values.join(", ")
            ));
        }
    }
    if !package.depends.is_empty() {
        host.warn(&format!(
            "gem cannot express the distro names in depends ({}); list RubyGems names in gem.dependencies",
            package.depends.join(", ")
        ));
    }
    let scripts = [
        ("pre_install", &package.scripts.pre_install),
        ("post_install", &package.scripts.post_install),
        ("pre_remove", &package.scripts.pre_remove),
        ("post_remove", &package.scripts.post_remove),
    ];
    for (name, script) in scripts {
        if script.is_some() {
            host.warn(&format!(
                "gem has no install scripts; the {name} script is not written"
            ));
        }
    }
    for entry in &package.entries {
        match entry.data {
            Data::Ghost => host.warn(&format!(
                "gem carries no files that it does not own; {} is not written",
                entry.path
            )),
            _ if entry.config => host.warn(&format!(
                "gem has no configuration files; {} is not marked as config",
                entry.path
            )),
            _ => {}
        }
    }
    if !matches!(package.release.as_str(), "" | "1") {
        host.warn(&format!(
            "gem has no release counter; release {} is not written",
            package.release
        ));
    }
    if package.arch != "all" && platform == "ruby" {
        host.warn(&format!(
            "arch {} is native but gem.platform is ruby; set gem.platform, for example x86_64-linux",
            package.arch
        ));
    }
}

/// The files of the data tar, with paths relative to the gem root, and the executable names. Directories and ghosts
/// are not carried.
fn place(
    package: &Package,
    prefix: &str,
    executable_dir: &str,
) -> Result<(Vec<Entry>, Vec<String>)> {
    let mut files = Vec::new();
    let mut executables = Vec::new();
    for entry in &package.entries {
        if matches!(entry.data, Data::Dir | Data::Ghost) {
            continue;
        }
        if let Some(relative) = entry
            .path
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            files.push(Entry {
                path: relative.into(),
                config: false,
                ..entry.clone()
            });
            continue;
        }
        let (parent, name) = entry.path.rsplit_once('/').unwrap_or(("", &entry.path));
        if parent == executable_dir {
            ensure!(
                matches!(entry.data, Data::File { .. }),
                "{} is a link in {executable_dir}; a gem's executables are files",
                entry.path
            );
            executables.push(name.to_string());
            files.push(Entry {
                path: format!("bin/{name}"),
                config: false,
                ..entry.clone()
            });
            continue;
        }
        bail!(
            "{} is outside the gem prefix {prefix} and not in {executable_dir}; a gem holds only the files under its prefix and its executables (set gem.prefix)",
            entry.path
        );
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    executables.sort();
    for pair in files.windows(2) {
        ensure!(
            pair[0].path != pair[1].path,
            "two files of the gem are at {}",
            pair[0].path
        );
    }
    Ok((files, executables))
}

/// The `metadata.gz` member: a `Gem::Specification` as YAML with the Ruby object tags.
fn specification(
    package: &Package,
    options: &Options,
    version: &str,
    platform: &str,
    files: &[String],
    executables: &[String],
) -> Result<String> {
    let (authors, emails) = contact(package.maintainer.as_deref());
    let mut y = String::from("--- !ruby/object:Gem::Specification\n");
    y += &format!("name: {}\n", quote(&package.name));
    y += &format!("version: !ruby/object:Gem::Version\n  version: '{version}'\n");
    y += &format!("platform: {}\n", quote(platform));
    list(&mut y, "authors", &authors);
    y += "autorequire:\n";
    y += "bindir: bin\n";
    y += "cert_chain: []\n";
    y += &format!("date: {}\n", date_line(package.mtime));
    dependencies(&mut y, options)?;
    y += &format!("description: {}\n", quote(&package.description));
    list(&mut y, "email", &emails);
    list(&mut y, "executables", executables);
    y += "extensions: []\n";
    y += "extra_rdoc_files: []\n";
    list(&mut y, "files", files);
    match &package.homepage {
        Some(homepage) => y += &format!("homepage: {}\n", quote(homepage)),
        None => y += "homepage:\n",
    }
    match &package.license {
        Some(license) => list(&mut y, "licenses", std::slice::from_ref(license)),
        None => y += "licenses: []\n",
    }
    y += "metadata: {}\n";
    y += "post_install_message:\n";
    y += "rdoc_options: []\n";
    list(&mut y, "require_paths", &options.require_paths);
    y += "required_ruby_version: !ruby/object:Gem::Requirement\n";
    requirement(
        &mut y,
        0,
        &constraints(options.required_ruby_version.as_deref().unwrap_or(""))?,
    );
    y += "required_rubygems_version: !ruby/object:Gem::Requirement\n";
    requirement(&mut y, 0, &constraints("")?);
    y += "requirements: []\n";
    y += &format!("rubygems_version: {RUBYGEMS_VERSION}\n");
    y += "signing_key:\n";
    y += "specification_version: 4\n";
    y += &format!("summary: {}\n", quote(package.summary()));
    y += "test_files: []\n";
    Ok(y)
}

/// The runtime dependencies, in the form RubyGems writes them.
fn dependencies(y: &mut String, options: &Options) -> Result<()> {
    if options.dependencies.is_empty() {
        *y += "dependencies: []\n";
        return Ok(());
    }
    *y += "dependencies:\n";
    for (name, requirement_text) in &options.dependencies {
        let parsed = constraints(requirement_text)
            .with_context(|| format!("gem.dependencies, the requirement of {name}"))?;
        *y += "- !ruby/object:Gem::Dependency\n";
        *y += &format!("  name: {}\n", quote(name));
        *y += "  requirement: !ruby/object:Gem::Requirement\n";
        requirement(y, 2, &parsed);
        *y += "  type: :runtime\n";
        *y += "  prerelease: false\n";
        *y += "  version_requirements: !ruby/object:Gem::Requirement\n";
        requirement(y, 2, &parsed);
    }
    Ok(())
}

/// A requirement body. `base` is the indentation of the key that holds the `!ruby/object:Gem::Requirement` tag.
fn requirement(y: &mut String, base: usize, constraints: &[(String, String)]) {
    let pad = |n: usize| " ".repeat(n);
    *y += &format!("{}requirements:\n", pad(base + 2));
    for (operator, version) in constraints {
        *y += &format!("{}- - {}\n", pad(base + 2), quote(operator));
        *y += &format!("{}- !ruby/object:Gem::Version\n", pad(base + 4));
        *y += &format!("{}version: '{version}'\n", pad(base + 6));
    }
}

/// The constraints of a requirement such as `~> 13.0, >= 13.0.1`. Empty means any version (`>= 0`).
fn constraints(text: &str) -> Result<Vec<(String, String)>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(vec![(">=".into(), "0".into())]);
    }
    text.split(',')
        .map(|part| {
            let part = part.trim();
            let operator_length = part
                .find(|c: char| !"<>=~!".contains(c))
                .unwrap_or(part.len());
            let (operator, version) = part.split_at(operator_length);
            let operator = if operator.is_empty() { "=" } else { operator };
            ensure!(
                ["=", "!=", ">", "<", ">=", "<=", "~>"].contains(&operator),
                "unknown requirement operator {operator:?} in {text:?}"
            );
            let version = version.trim();
            ensure!(
                version.starts_with(|c: char| c.is_ascii_digit())
                    && version
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'),
                "{version:?} in {text:?} is not a version"
            );
            Ok((operator.to_string(), version.to_string()))
        })
        .collect()
}

/// The authors and the emails of a maintainer written `Name <email>`.
fn contact(maintainer: Option<&str>) -> (Vec<String>, Vec<String>) {
    let Some(maintainer) = maintainer.map(str::trim).filter(|m| !m.is_empty()) else {
        return (Vec::new(), Vec::new());
    };
    match maintainer.rsplit_once('<') {
        Some((name, rest)) if rest.ends_with('>') => {
            let name = name.trim();
            let authors = if name.is_empty() {
                Vec::new()
            } else {
                vec![name.to_string()]
            };
            (authors, vec![rest.trim_end_matches('>').trim().to_string()])
        }
        _ => (vec![maintainer.to_string()], Vec::new()),
    }
}

/// A YAML sequence under `key`, or `[]` when empty.
fn list(y: &mut String, key: &str, items: &[String]) {
    if items.is_empty() {
        *y += &format!("{key}: []\n");
        return;
    }
    *y += &format!("{key}:\n");
    for item in items {
        *y += &format!("- {}\n", quote(item));
    }
}

/// A YAML double-quoted scalar. Every string of the spec goes through this, so no text can break the YAML.
fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The spec `date`: midnight UTC of the day of `mtime`, as RubyGems writes it.
fn date_line(mtime: u64) -> String {
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = (mtime / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} 00:00:00.000000000 Z")
}

/// The `checksums.yaml.gz` member: SHA256 and SHA512 of the gzipped `metadata.gz` and `data.tar.gz`.
fn checksums(metadata_gz: &[u8], data_gz: &[u8]) -> String {
    format!(
        "---\nSHA256:\n  metadata.gz: {}\n  data.tar.gz: {}\nSHA512:\n  metadata.gz: {}\n  data.tar.gz: {}\n",
        hex(&Sha256::digest(metadata_gz)),
        hex(&Sha256::digest(data_gz)),
        hex(&Sha512::digest(metadata_gz)),
        hex(&Sha512::digest(data_gz)),
    )
}
