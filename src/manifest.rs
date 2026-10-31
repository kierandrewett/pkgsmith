//! The manifest: one YAML file describes one package.

use crate::expand::Scope;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// Build this entry for these formats only, for example `[deb, ipk]`.
    pub only: Option<Vec<String>>,
    /// Build this entry only when the expression is true, for example `matrix.arch == 'arm64'`.
    #[serde(rename = "if")]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Scripts {
    pub pre_install: Option<String>,
    pub post_install: Option<String>,
    pub pre_remove: Option<String>,
    pub post_remove: Option<String>,
}

/// Fields a format or a distro may replace. A list that is present replaces the base list in full.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    pub depends: Option<Vec<String>>,
    pub recommends: Option<Vec<String>>,
    pub suggests: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub replaces: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,
    pub scripts: Option<Scripts>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DebOptions {
    pub section: Option<String>,
    pub priority: Option<String>,
    /// `xz` (default), `zstd`, `gzip`.
    pub compression: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RpmOptions {
    pub group: Option<String>,
    /// `zstd` (default), `xz`, `gzip`.
    pub compression: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ArchOptions {
    pub packager: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct IpkOptions {
    /// The OpenWrt architecture, for example `x86_64` or `aarch64_generic`. Default: derived from `arch`.
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TarOptions {
    /// `gzip`, `xz` (default), `zstd`, `none`.
    pub compression: Option<String>,
    /// Put every file below a top folder, `{name}-{version}` when `true`.
    #[serde(default)]
    pub top_folder: bool,
}

/// A matrix value: text, a number or a flag, all kept as text.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Text(String),
    Int(i64),
    Float(f64),
    Flag(bool),
}

impl Scalar {
    fn text(&self) -> String {
        match self {
            Scalar::Text(t) => t.clone(),
            Scalar::Int(i) => i.to_string(),
            Scalar::Float(f) => f.to_string(),
            Scalar::Flag(b) => b.to_string(),
        }
    }
}

/// Like the `strategy.matrix` of a GitHub Actions workflow. Every key except `include` and `exclude` is an axis with
/// a list of values, and the build runs once per combination. The axes `format`, `arch` and `distro` steer the build.
/// Any other axis is a variable for `${{ matrix.NAME }}` and `if:`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Matrix {
    /// Combinations to add, or extra values to give the combinations they match.
    #[serde(default)]
    pub include: Vec<BTreeMap<String, Scalar>>,
    /// Combinations to drop: every key listed must match.
    #[serde(default)]
    pub exclude: Vec<BTreeMap<String, Scalar>>,
    #[serde(flatten)]
    pub axes: BTreeMap<String, Vec<Scalar>>,
}

/// One combination of the matrix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Job {
    pub values: BTreeMap<String, String>,
}

impl Job {
    pub fn format(&self) -> Option<&str> {
        self.values.get("format").map(String::as_str)
    }

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
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Matrix {
    /// All combinations: the product of the axes, minus `exclude`, with `include` applied the way Actions does.
    pub fn jobs(&self) -> Vec<Job> {
        let axes: Vec<(&String, Vec<String>)> = self
            .axes
            .iter()
            .map(|(name, values)| (name, values.iter().map(Scalar::text).collect()))
            .collect();
        let mut combos: Vec<BTreeMap<String, String>> = vec![BTreeMap::new()];
        if axes.is_empty() && !self.include.is_empty() {
            combos.clear();
        }
        for (name, values) in &axes {
            combos = combos
                .into_iter()
                .flat_map(|combo| {
                    values.iter().map(move |value| {
                        let mut next = combo.clone();
                        next.insert((*name).clone(), value.clone());
                        next
                    })
                })
                .collect();
        }
        let matches = |combo: &BTreeMap<String, String>,
                       other: &BTreeMap<String, Scalar>,
                       only: &dyn Fn(&str) -> bool| {
            other
                .iter()
                .filter(|(key, _)| only(key))
                .all(|(key, value)| combo.get(key) == Some(&value.text()))
        };
        for exclude in &self.exclude {
            combos.retain(|combo| !matches(combo, exclude, &|_| true));
        }
        let original: Vec<&str> = axes.iter().map(|(name, _)| name.as_str()).collect();
        for include in &self.include {
            let is_axis = |key: &str| original.contains(&key);
            let mut added = false;
            for combo in combos.iter_mut() {
                if original.is_empty() || !matches(combo, include, &is_axis) {
                    continue;
                }
                for (key, value) in include {
                    combo.entry(key.clone()).or_insert_with(|| value.text());
                }
                added = true;
            }
            if !added {
                combos.push(include.iter().map(|(k, v)| (k.clone(), v.text())).collect());
            }
        }
        combos.into_iter().map(|values| Job { values }).collect()
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    #[serde(default = "one")]
    pub release: String,
    /// `amd64`, `arm64` or `all` (no compiled code).
    #[serde(default = "amd64")]
    pub arch: String,
    pub description: String,
    /// Defaults for `${NAME}` and `${{ env.NAME }}`. The process environment and `--set` win.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub matrix: Matrix,
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
    /// By format (`deb`, `rpm`, ...) or by distro (`fedora`, `opensuse`, `debian`, `ubuntu`, `arch`). A distro wins.
    #[serde(default)]
    pub overrides: BTreeMap<String, Overrides>,
    #[serde(default)]
    pub deb: DebOptions,
    #[serde(default)]
    pub rpm: RpmOptions,
    #[serde(default, rename = "archlinux")]
    pub arch_linux: ArchOptions,
    #[serde(default)]
    pub ipk: IpkOptions,
    #[serde(default)]
    pub tar: TarOptions,
    /// The folder of the manifest. Relative `src` paths and script files resolve against it.
    #[serde(skip)]
    pub base: PathBuf,
}

fn one() -> String {
    "1".into()
}

fn amd64() -> String {
    "amd64".into()
}

/// A mode written as the octal text `"0755"` or as the YAML number `0755` (read as its digits).
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
    /// Read a manifest without expanding anything. `jobs` and `bind` do the rest.
    pub fn parse(path: &Path) -> Result<Manifest> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let mut manifest: Manifest = serde_yaml_ng::from_str(&text)
            .with_context(|| format!("{} is not a valid manifest", path.display()))?;
        manifest.base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        if manifest.base.as_os_str().is_empty() {
            manifest.base = PathBuf::from(".");
        }
        Ok(manifest)
    }

    pub fn jobs(&self) -> Vec<Job> {
        self.matrix.jobs()
    }

    /// The manifest as one job and one format see it: variables and expressions expanded, `if:` entries decided.
    pub fn bind(&self, base: &Scope, job: &Job, format: &str) -> Result<Manifest> {
        let mut context = base.clone();
        context.matrix = job.values.clone();
        context.format = format.to_string();
        context.distro = job.distro().unwrap_or_default().to_string();
        for (name, value) in &self.env {
            if !context.env.contains_key(name) {
                let value = context.expand(value)?;
                context.env.insert(name.clone(), value);
            }
        }
        let mut bound = self.clone();
        context.arch = match job.arch() {
            Some(arch) => arch.to_string(),
            None => context.expand(&self.arch)?,
        };
        bound.arch = context.arch.clone();
        let expand = |text: &mut String| -> Result<()> {
            *text = context.expand(text)?;
            Ok(())
        };
        expand(&mut bound.name)?;
        expand(&mut bound.version)?;
        expand(&mut bound.release)?;
        for text in [
            &mut bound.maintainer,
            &mut bound.homepage,
            &mut bound.license,
        ]
        .into_iter()
        .flatten()
        {
            expand(text)?;
        }
        for list in [
            &mut bound.depends,
            &mut bound.recommends,
            &mut bound.suggests,
            &mut bound.conflicts,
            &mut bound.replaces,
            &mut bound.provides,
        ] {
            for text in list {
                expand(text)?;
            }
        }
        let mut contents = Vec::new();
        for mut content in std::mem::take(&mut bound.contents) {
            let included = match &content.condition {
                Some(condition) => context
                    .test(condition)
                    .with_context(|| format!("in the if: of {}", content.dst))?,
                None => true,
            };
            if !included {
                continue;
            }
            expand(&mut content.dst)?;
            if let Some(src) = &mut content.src {
                expand(src)?;
            }
            contents.push(content);
        }
        bound.contents = contents;
        bound.matrix = Matrix::default();
        bound.check()?;
        Ok(bound)
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

    /// The effective `Overrides` for a format and an optional distro: the format first, the distro on top.
    pub fn effective(&self, format: &str, distro: Option<&str>) -> Effective {
        let mut effective = Effective {
            depends: self.depends.clone(),
            recommends: self.recommends.clone(),
            suggests: self.suggests.clone(),
            conflicts: self.conflicts.clone(),
            replaces: self.replaces.clone(),
            provides: self.provides.clone(),
            scripts: self.scripts.clone(),
        };
        for key in [Some(format), distro].into_iter().flatten() {
            if let Some(over) = self.overrides.get(key) {
                let replace = |target: &mut Vec<String>, source: &Option<Vec<String>>| {
                    if let Some(list) = source {
                        *target = list.clone();
                    }
                };
                replace(&mut effective.depends, &over.depends);
                replace(&mut effective.recommends, &over.recommends);
                replace(&mut effective.suggests, &over.suggests);
                replace(&mut effective.conflicts, &over.conflicts);
                replace(&mut effective.replaces, &over.replaces);
                replace(&mut effective.provides, &over.provides);
                if let Some(scripts) = &over.scripts {
                    effective.scripts = scripts.clone();
                }
            }
        }
        effective
    }
}

#[derive(Debug, Clone, Default)]
pub struct Effective {
    pub depends: Vec<String>,
    pub recommends: Vec<String>,
    pub suggests: Vec<String>,
    pub conflicts: Vec<String>,
    pub replaces: Vec<String>,
    pub provides: Vec<String>,
    pub scripts: Scripts,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Manifest> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pkg.yaml");
        std::fs::write(&path, text).unwrap();
        Manifest::parse(&path)
    }

    fn bind(text: &str, job: Job, format: &str) -> Result<Manifest> {
        let mut context = Scope::default();
        context.set("VERSION", "1.2.3");
        parse(text)?.bind(&context, &job, format)
    }

    fn job(pairs: &[(&str, &str)]) -> Job {
        Job {
            values: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    const BASE: &str = "name: demo\nversion: ${VERSION}\ndescription: A demo\n";

    #[test]
    fn minimal_manifest_gets_defaults_and_expands_the_version() {
        let manifest = bind(BASE, Job::default(), "deb").unwrap();
        assert_eq!(
            (
                manifest.version.as_str(),
                manifest.release.as_str(),
                manifest.arch.as_str()
            ),
            ("1.2.3", "1", "amd64")
        );
    }

    #[test]
    fn modes_read_as_octal_from_text_or_number() {
        let manifest = parse(&format!("{BASE}contents:\n  - {{src: a, dst: /a, mode: 0755}}\n  - {{src: b, dst: /b, mode: \"0644\"}}\n")).unwrap();
        assert_eq!(manifest.contents[0].mode, Some(0o755));
        assert_eq!(manifest.contents[1].mode, Some(0o644));
        assert!(
            parse(&format!(
                "{BASE}contents:\n  - {{src: a, dst: /a, mode: \"9\"}}\n"
            ))
            .is_err()
        );
    }

    #[test]
    fn unknown_keys_are_rejected_so_a_typo_does_not_vanish() {
        assert!(parse(&format!("{BASE}dependz: [a]\n")).is_err());
    }

    #[test]
    fn a_distro_override_replaces_the_format_override_which_replaces_the_base() {
        let manifest = parse(&format!(
            "{BASE}depends: [base]\noverrides:\n  rpm: {{depends: [rpm-dep]}}\n  opensuse: {{depends: [suse-dep]}}\n"
        ))
        .unwrap();
        assert_eq!(manifest.effective("deb", None).depends, ["base"]);
        assert_eq!(
            manifest.effective("rpm", Some("fedora")).depends,
            ["rpm-dep"]
        );
        assert_eq!(
            manifest.effective("rpm", Some("opensuse")).depends,
            ["suse-dep"]
        );
    }

    #[test]
    fn relative_destinations_and_missing_sources_are_errors() {
        assert!(
            bind(
                &format!("{BASE}contents:\n  - {{src: a, dst: usr/bin/a}}\n"),
                Job::default(),
                "deb"
            )
            .is_err()
        );
        assert!(
            bind(
                &format!("{BASE}contents:\n  - {{dst: /a}}\n"),
                Job::default(),
                "deb"
            )
            .is_err()
        );
    }

    fn jobs(matrix: &str) -> Vec<String> {
        let mut labels: Vec<String> = parse(&format!("{BASE}matrix:\n{matrix}"))
            .unwrap()
            .jobs()
            .iter()
            .map(Job::label)
            .collect();
        labels.sort();
        labels
    }

    #[test]
    fn the_matrix_is_the_product_of_its_axes_minus_exclude() {
        let labels = jobs(
            "  arch: [amd64, arm64]\n  format: [deb, apk]\n  exclude:\n    - {format: apk, arch: arm64}\n",
        );
        assert_eq!(
            labels,
            [
                "arch=amd64, format=apk",
                "arch=amd64, format=deb",
                "arch=arm64, format=deb"
            ]
        );
    }

    #[test]
    fn include_adds_values_to_matching_jobs_and_new_jobs_when_nothing_matches() {
        let labels = jobs(
            "  format: [rpm, deb]\n  include:\n    - {format: rpm, distro: fedora}\n    - {format: apk, distro: alpine}\n",
        );
        assert_eq!(
            labels,
            [
                "distro=alpine, format=apk",
                "distro=fedora, format=rpm",
                "format=deb"
            ]
        );
    }

    #[test]
    fn a_manifest_without_a_matrix_is_one_default_job() {
        assert_eq!(parse(BASE).unwrap().jobs(), vec![Job::default()]);
    }

    #[test]
    fn if_and_expressions_choose_contents_and_paths_per_job() {
        let text = format!(
            "{BASE}contents:\n  - {{src: \"${{{{ matrix.arch }}}}/a\", dst: /a}}\n  - {{src: b, dst: /arm-only, if: \"matrix.arch == 'arm64'\"}}\n"
        );
        let amd = bind(&text, job(&[("arch", "amd64")]), "deb").unwrap();
        assert_eq!(amd.contents.len(), 1);
        assert_eq!(amd.contents[0].src.as_deref(), Some("amd64/a"));
        let arm = bind(&text, job(&[("arch", "arm64")]), "deb").unwrap();
        assert_eq!(arm.contents.len(), 2);
        assert_eq!(arm.arch, "arm64");
    }
}
