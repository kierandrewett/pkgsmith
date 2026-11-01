//! What a script declares: packages. `script.rs` runs `pkgsmith.star` and gives a [`TargetSpec`] for every `package(...)`
//! call it reaches. [`Manifest::from_spec`] checks a spec and turns it into the [`Manifest`] that the rest of pkgsmith works
//! from: the fields of one package in one format.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What a `contents` entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// One file, or the files a glob matches. `dst` is a folder when `src` is a glob or `dst` ends with `/`.
    #[default]
    File,
    /// An empty folder the package owns.
    Dir,
    /// A folder copied recursively to `dst`, including the folder itself and every folder below it.
    Tree,
    /// A symbolic link at `dst`. `src` is the link target as it reads on the installed system.
    Symlink,
    /// A configuration file: kept on upgrade and removal when the user changed it.
    Config,
    /// A file the package owns but does not carry, for example a log. Only rpm records it.
    Ghost,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub src: Option<String>,
    pub dst: String,
    #[serde(rename = "type", default)]
    pub kind: Kind,
    #[serde(default, deserialize_with = "octal")]
    pub mode: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Scripts {
    pub pre_install: Option<String>,
    pub post_install: Option<String>,
    pub pre_remove: Option<String>,
    pub post_remove: Option<String>,
}

/// One package to write: the fields of the package, the format, and the job that builds its files.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSpec {
    pub name: String,
    pub version: String,
    pub format: String,
    pub description: String,
    #[serde(default = "one", deserialize_with = "text_or_number")]
    pub release: String,
    /// Raises a version over every version with a lower epoch, whatever their numbers. `0` leaves it out.
    #[serde(default)]
    pub epoch: u32,
    /// `amd64`, `arm64` or `all` (no compiled code).
    #[serde(default = "amd64")]
    pub arch: String,
    pub maintainer: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub recommends: Vec<String>,
    #[serde(default)]
    pub suggests: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub replaces: Vec<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub contents: Vec<Content>,
    #[serde(default)]
    pub scripts: Scripts,
    /// The options of the format, as the plugin that writes it defines them.
    #[serde(default)]
    pub options: serde_json::Value,
    /// Names the lane in the progress output and the distro a plugin may adapt to, for example `fedora`.
    pub distro: Option<String>,
    /// The name of the output file. A template. See `package::output_name`.
    pub file: Option<String>,
    /// Labels for `--where`.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// Paths (globs) of the package that it leaves out.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Paths (globs) of the package that it keeps. Everything else is left out. Without it, everything is kept.
    #[serde(default)]
    pub include: Vec<String>,
    /// Paths (globs) of the package that are configuration files: kept on upgrade and removal when the user changed them.
    #[serde(default)]
    pub config_files: Vec<String>,
}

/// One package as the rest of pkgsmith sees it.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub release: String,
    pub epoch: u32,
    pub arch: String,
    pub description: String,
    pub maintainer: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub depends: Vec<String>,
    pub recommends: Vec<String>,
    pub suggests: Vec<String>,
    pub conflicts: Vec<String>,
    pub replaces: Vec<String>,
    pub provides: Vec<String>,
    pub contents: Vec<Content>,
    pub scripts: Scripts,
    pub options: serde_json::Value,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    pub config_files: Vec<String>,
    /// The folder of the script. Script files (install scripts) resolve against it.
    pub base: PathBuf,
    /// The prefix: the read-only folder that holds the files to package. A relative `src` resolves against it.
    pub prefix: PathBuf,
}

/// The values of a target that `--where`, the file template and the progress output use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Job {
    pub values: BTreeMap<String, String>,
}

impl Job {
    pub fn distro(&self) -> Option<&str> {
        self.values.get("distro").map(String::as_str)
    }

    pub fn arch(&self) -> Option<&str> {
        self.values.get("arch").map(String::as_str)
    }

    pub fn label(&self) -> String {
        if self.values.is_empty() {
            return "default".into();
        }
        self.values
            .iter()
            .filter(|(k, _)| k.as_str() != "file")
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn one() -> String {
    "1".into()
}

fn amd64() -> String {
    "amd64".into()
}

/// A text, or a number written as text: `release = 1` and `release = "1"` are the same.
fn text_or_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Text {
        Number(i64),
        Text(String),
    }
    Ok(match Text::deserialize(deserializer)? {
        Text::Number(number) => number.to_string(),
        Text::Text(text) => text,
    })
}

/// A mode written as the octal text `"0755"` or as a number read by its digits (`755`).
fn octal<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Mode {
        Number(u32),
        Text(String),
    }
    let digits = match Option::<Mode>::deserialize(deserializer)? {
        None => return Ok(None),
        Some(Mode::Number(number)) => number.to_string(),
        Some(Mode::Text(text)) => text,
    };
    u32::from_str_radix(digits.trim_start_matches("0o"), 8)
        .map(Some)
        .map_err(|_| {
            serde::de::Error::custom(format!(
                "mode {digits:?} is not octal, write it like \"0755\""
            ))
        })
}

impl Manifest {
    /// Check a target and turn it into the package pkgsmith works with. `base` is the folder of the script and
    /// `prefix` the folder that holds the files.
    pub fn from_spec(spec: TargetSpec, base: PathBuf, prefix: PathBuf) -> Result<Manifest> {
        let mut manifest = Manifest {
            name: spec.name,
            version: spec.version,
            release: spec.release,
            epoch: spec.epoch,
            arch: spec.arch,
            description: spec.description,
            maintainer: spec.maintainer,
            homepage: spec.homepage,
            license: spec.license,
            depends: spec.depends,
            recommends: spec.recommends,
            suggests: spec.suggests,
            conflicts: spec.conflicts,
            replaces: spec.replaces,
            provides: spec.provides,
            contents: spec.contents,
            scripts: spec.scripts,
            options: spec.options,
            base,
            prefix,
            exclude: spec.exclude,
            include: spec.include,
            config_files: spec.config_files,
        };
        if manifest.options.as_object().is_some_and(|o| o.is_empty()) {
            manifest.options = serde_json::Value::Null;
        }
        manifest.check()?;
        Ok(manifest)
    }

    fn check(&self) -> Result<()> {
        let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-._".contains(c);
        if self.name.is_empty()
            || !self.name.chars().all(valid)
            || !self.name.starts_with(|c: char| c.is_ascii_alphanumeric())
        {
            bail!(
                "name {:?} must be lowercase letters, digits and + - . _, starting with a letter or digit",
                self.name
            );
        }
        if self.version.is_empty() {
            bail!("version is empty");
        }
        if !["amd64", "arm64", "all"].contains(&self.arch.as_str()) {
            bail!("arch {:?} must be amd64, arm64 or all", self.arch);
        }
        for content in &self.contents {
            if !content.dst.starts_with('/') {
                bail!("dst {:?} must be an absolute path", content.dst);
            }
            let needs_src = !matches!(content.kind, Kind::Dir | Kind::Ghost);
            if needs_src && content.src.is_none() {
                bail!("{}: type {:?} needs a src", content.dst, content.kind);
            }
        }
        Ok(())
    }

    /// Read a target from JSON, the way a script gives it. Used by `script.rs` and by tests.
    pub fn parse_spec(value: serde_json::Value) -> Result<TargetSpec> {
        serde_json::from_value(value)
            .context("the target has a field that pkgsmith does not accept")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(extra: serde_json::Value) -> TargetSpec {
        let mut base =
            json!({"name": "demo", "version": "1.0.0", "format": "rpm", "description": "d"});
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        Manifest::parse_spec(base).unwrap()
    }

    fn build(extra: serde_json::Value) -> Result<Manifest> {
        Manifest::from_spec(spec(extra), PathBuf::from("."), PathBuf::from("."))
    }

    #[test]
    fn a_minimal_target_gets_defaults() {
        let m = build(json!({})).unwrap();
        assert_eq!(
            (m.release.as_str(), m.arch.as_str(), m.epoch),
            ("1", "amd64", 0)
        );
        assert!(m.exclude.is_empty());
    }

    #[test]
    fn release_may_be_a_number_and_an_unknown_field_is_an_error() {
        assert_eq!(build(json!({"release": 3})).unwrap().release, "3");
        let error = Manifest::parse_spec(json!({"name": "a", "version": "1", "format": "rpm", "description": "d", "dependz": []}))
            .unwrap_err();
        assert!(format!("{error:#}").contains("dependz"), "{error:#}");
    }

    #[test]
    fn bad_names_modes_and_contents_are_refused() {
        assert!(build(json!({"name": "Bad Name"})).is_err());
        assert!(build(json!({"arch": "sparc"})).is_err());
        assert!(build(json!({"contents": [{"dst": "relative"}]})).is_err());
        assert!(build(json!({"contents": [{"dst": "/a", "type": "symlink"}]})).is_err());
        let error = Manifest::parse_spec(
            json!({"name": "a", "version": "1", "format": "rpm", "description": "d",
            "contents": [{"dst": "/a", "mode": "9"}]}),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("accept"), "{error:#}");
    }
}
