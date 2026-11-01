//! The manifest: one YAML file describes one package.

use crate::expand::Scope;
use crate::expr;
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
            .filter(|(k, _)| !["file", "needs"].contains(&k.as_str()))
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
        let matches = matches_entry;
        for exclude in &self.exclude {
            combos.retain(|combo| !matches(combo, exclude, &|_| true));
        }
        let original: Vec<&str> = axes.iter().map(|(name, _)| name.as_str()).collect();
        // Like Actions, an entry matches the combinations of the axes only. A job that an earlier entry added is not
        // matched by a later one, so two entries that name the same axis values stay two jobs.
        let from_axes = combos.len();
        for include in &self.include {
            let is_axis = |key: &str| original.contains(&key);
            let mut added = false;
            for combo in combos.iter_mut().take(from_axes) {
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

/// Does a combination have the value of every key of the entry that `only` selects?
fn matches_entry(
    combo: &BTreeMap<String, String>,
    other: &BTreeMap<String, Scalar>,
    only: &dyn Fn(&str) -> bool,
) -> bool {
    other
        .iter()
        .filter(|(key, _)| only(key))
        .all(|(key, value)| combo.get(key) == Some(&value.text()))
}

/// A job that builds files, like a job of a GitHub Actions workflow: a container image, a matrix, environment variables
/// and ordered steps. Its result is the file tree that the steps install. Package targets name the job they pack with
/// `needs`, so several targets, and several formats, can share one run of a job.
///
/// A job sees only its own matrix, the architecture and `env`. It cannot read the format, the distro or any other value
/// of a target, so what it builds cannot depend on them.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildJob {
    /// The container image the steps run in. It is pulled when it is not on the machine.
    #[serde(rename = "runs-on")]
    pub runs_on: String,
    /// The variants of this job, such as `arch`. A target takes the run whose values agree with its own. Without an `arch`
    /// key, a job runs for the architecture of each target that needs it.
    #[serde(default)]
    pub matrix: Matrix,
    /// Variables for every step. The values are literal text: nothing in them is expanded by a shell.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Paths (globs) that the steps install and the package leaves out.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Steps that prepare the environment, for example by installing compilers. They run once, and the result is cached
    /// as an image. What they install is not packaged.
    #[serde(default)]
    pub setup: Vec<Step>,
    /// Steps that make the files. Whatever they install outside `/src` is packaged.
    pub steps: Vec<Step>,
}

/// One script of a job. Each step runs in a fresh `sh -e` in `/src`. A step does not see what an earlier step exported or
/// where it changed directory, but it sees the files earlier steps wrote and the lines they appended to `$PKGSMITH_ENV`
/// and `$PKGSMITH_PATH`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Shown in the progress output.
    pub name: Option<String>,
    /// The script. A `#!` line is not special: the text always runs with `sh -e`.
    pub run: String,
    /// An expression that decides whether the step runs. It reads `matrix.NAME` of its job, `arch` and `env.NAME`.
    #[serde(rename = "if")]
    pub condition: Option<String>,
    /// Variables for this step, after those of the job. The values are literal text.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// What a package target packs: the tree of one job, or nothing.
#[derive(Debug, Clone)]
pub enum Needs {
    /// The name of a job.
    Job(String),
    /// `[]`: no job. The target packs its `contents` only.
    Nothing,
}

impl<'de> Deserialize<'de> for Needs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Needs;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a job name, or [] for no job")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Needs, E> {
                if value.is_empty() {
                    return Err(E::custom(
                        "needs is empty; write a job name, or [] for no job",
                    ));
                }
                Ok(Needs::Job(value.to_string()))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Needs, A::Error> {
                match seq.next_element::<serde::de::IgnoredAny>()? {
                    None => Ok(Needs::Nothing),
                    Some(_) => Err(serde::de::Error::custom(
                        "needs takes one job name; a package comes from one build",
                    )),
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// One package target: a distro, the format or formats to write for it, and the job that builds its files. Every target
/// is crossed with the cells of the `matrix`, which holds what varies for all of them, such as the architecture.
#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    /// One format, or a list of them. Each format is a package of its own.
    pub format: OneOrMany,
    /// Selects the matching `overrides` entry and names the lane in the progress output.
    pub distro: Option<String>,
    /// The job whose tree the target packs. May be left out when the manifest has exactly one job. `[]` means no job.
    pub needs: Option<Needs>,
    /// The name of the output file. A template. See the docs of the matrix.
    pub file: Option<String>,
    /// An expression over `env.NAME`. A false target makes no packages, so a target can exist only when a build argument
    /// such as `STAGE` is given.
    #[serde(rename = "if")]
    pub condition: Option<String>,
    /// Any other key is a variable for `${{ matrix.NAME }}` and `if:`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Scalar>,
}

/// A text, or a list of texts.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn all(&self) -> Vec<String> {
        match self {
            OneOrMany::One(one) => vec![one.clone()],
            OneOrMany::Many(many) => many.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    #[serde(default = "one")]
    pub release: String,
    /// Raises a version over every version with a lower epoch, whatever their numbers. `0` leaves it out.
    #[serde(default)]
    pub epoch: u32,
    /// `amd64`, `arm64` or `all` (no compiled code).
    #[serde(default = "amd64")]
    pub arch: String,
    pub description: String,
    /// Defaults for `${NAME}` and `${{ env.NAME }}`. The process environment and `--set` win.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub matrix: Matrix,
    /// The package targets, crossed with the `matrix`. Without targets, the `matrix` alone makes the packages.
    #[serde(default)]
    pub targets: Vec<Target>,
    /// The jobs that build the files. A target names the one it packs with `needs`.
    #[serde(default)]
    pub jobs: BTreeMap<String, BuildJob>,
    /// What the job of the target gives it. Filled in by `bind`, never read from the file.
    #[serde(skip)]
    pub image: Option<String>,
    /// The setup steps of the job, with the conditions decided. They run once and are cached. Not tracked.
    #[serde(skip)]
    pub setup: Option<String>,
    /// The other steps. Whatever they write outside `/src` is packaged.
    #[serde(skip)]
    pub build: Option<String>,
    /// Paths (globs) the build installed that the package leaves out.
    #[serde(skip)]
    pub exclude: Vec<String>,
    /// What the steps may read: `PKGSMITH_*` and the `MATRIX_*` of the job's own cell.
    #[serde(skip)]
    pub build_env: BTreeMap<String, String>,
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
    /// Options of a format, in a section named like the format (`deb:`, `rpm:`, ...). The plugin that writes the format
    /// defines what the section may hold. `check_options` refuses a key that no format owns, so a typo is an error.
    #[serde(flatten)]
    pub options: BTreeMap<String, serde_json::Value>,
    /// The folder of the manifest. Relative `src` paths and script files resolve against it.
    #[serde(skip)]
    pub base: PathBuf,
    /// The build context: the folder mounted at `/src` in a tracked build. The folder of the manifest unless the
    /// command line names another.
    #[serde(skip)]
    pub context: PathBuf,
}

/// Single quotes around text, so the shell reads it literally.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// What every step starts with. `$PKGSMITH_ENV` holds one `NAME=value` per line and `$PKGSMITH_PATH` one folder per line.
/// Earlier steps append to them, like `GITHUB_ENV` and `GITHUB_PATH` in GitHub Actions. They are read as data: `export
/// "NAME=value"` assigns the text after the first `=` as it is, so quotes and `$` in a value do nothing. They live in
/// `/tmp`, which is never packaged, and a setup step's lines reach the later steps through the cached image.
const PRELUDE: &str = r#"PKGSMITH_ENV=/tmp/pkgsmith-state/env
PKGSMITH_PATH=/tmp/pkgsmith-state/path
mkdir -p /tmp/pkgsmith-state && : >> "$PKGSMITH_ENV" && : >> "$PKGSMITH_PATH"
export PKGSMITH_ENV PKGSMITH_PATH
if [ -s "$PKGSMITH_PATH" ]; then
  PATH="$(paste -sd: "$PKGSMITH_PATH"):$PATH"
  export PATH
fi
while IFS= read -r pkgsmith_line || [ -n "$pkgsmith_line" ]; do
  case "$pkgsmith_line" in
    '' | '#'*) continue ;;
    *=*) ;;
    *) echo "PKGSMITH_ENV: not NAME=value: $pkgsmith_line" >&2; exit 1 ;;
  esac
  case "${pkgsmith_line%%=*}" in
    '' | [0-9]* | *[!A-Za-z0-9_]*) echo "PKGSMITH_ENV: bad name in: $pkgsmith_line" >&2; exit 1 ;;
  esac
  export "$pkgsmith_line"
done < "$PKGSMITH_ENV"
unset pkgsmith_line
"#;

/// A step as shell text: a title line, then the script in a fresh `sh -e`. The job's variables come first, then the
/// prelude, then the step's own variables, so a step beats a line of `$PKGSMITH_ENV` and that beats the job. The
/// delimiter of the here document comes from the script itself, so no line of it can end the document early.
fn render_step(
    title: &str,
    job_env: &BTreeMap<String, String>,
    step_env: &BTreeMap<String, String>,
    script: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let exports = |env: &BTreeMap<String, String>| -> String {
        env.iter()
            .map(|(key, value)| format!("export {key}={}\n", sh_quote(value)))
            .collect()
    };
    let mut body = exports(job_env);
    body.push_str(PRELUDE);
    body.push_str(&exports(step_env));
    body.push_str(script);
    if !body.ends_with('\n') {
        body.push('\n');
    }
    let digest = Sha256::digest(body.as_bytes());
    let delimiter: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let title = title.replace('\n', " ");
    format!(
        "#==> {title}\nprintf '%s\\n' {}\nsh -e <<'PKGSMITH_{delimiter}'\n{body}PKGSMITH_{delimiter}\n",
        sh_quote(&format!("==> {title}"))
    )
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

/// How deep `include:` may nest. A chain this long is a mistake, and the limit keeps a loop from running away.
const MAX_INCLUDE_DEPTH: usize = 8;

/// Read a manifest file and the files it includes, merged into one YAML value.
///
/// `include:` is a top-level key that holds a file name or a list of file names, relative to the folder of the file
/// that names them. Included files are merged in order, then the including file is merged on top, so the file that
/// includes wins. Merging works key by key: two maps merge, and for anything else (text, numbers, lists) the later value
/// replaces the earlier one. A list is replaced as a whole, it is not joined.
fn load_merged(path: &Path, stack: &mut Vec<PathBuf>) -> Result<serde_yaml_ng::Value> {
    use serde_yaml_ng::Value;
    let canonical =
        std::fs::canonicalize(path).with_context(|| format!("cannot read {}", path.display()))?;
    if stack.contains(&canonical) {
        let chain: Vec<String> = stack
            .iter()
            .chain([&canonical])
            .map(|p| p.display().to_string())
            .collect();
        bail!("include loop: {}", chain.join(" -> "));
    }
    if stack.len() >= MAX_INCLUDE_DEPTH {
        bail!(
            "include: nested more than {MAX_INCLUDE_DEPTH} files deep at {}",
            path.display()
        );
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut own: Value = serde_yaml_ng::from_str(&text)
        .with_context(|| format!("{} is not valid YAML", path.display()))?;
    let Some(map) = own.as_mapping_mut() else {
        bail!("{} must hold a map of keys", path.display());
    };
    let included = map.remove("include");
    let names: Vec<String> = match included {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(name)) => vec![name],
        Some(Value::Sequence(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::String(name) => Ok(name),
                other => bail!(
                    "include: in {} has {other:?}; list file names",
                    path.display()
                ),
            })
            .collect::<Result<_>>()?,
        Some(other) => bail!(
            "include: in {} must be a file name or a list of them, not {other:?}",
            path.display()
        ),
    };
    let folder = path.parent().unwrap_or(Path::new("."));
    stack.push(canonical);
    let mut merged = Value::Mapping(Default::default());
    for name in names {
        let file = folder.join(&name);
        let part = load_merged(&file, stack)
            .with_context(|| format!("included from {}", path.display()))?;
        merge(&mut merged, part);
    }
    stack.pop();
    merge(&mut merged, own);
    Ok(merged)
}

/// Merge `over` into `base`: maps merge key by key, every other value replaces.
fn merge(base: &mut serde_yaml_ng::Value, over: serde_yaml_ng::Value) {
    use serde_yaml_ng::Value;
    match (base, over) {
        (Value::Mapping(base), Value::Mapping(over)) => {
            for (key, value) in over {
                match base.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, over) => *base = over,
    }
}

impl Manifest {
    /// Read a manifest without expanding anything. `jobs` and `bind` do the rest. Files named by `include:` are read
    /// first and merged underneath: see [`load_merged`].
    pub fn parse(path: &Path) -> Result<Manifest> {
        let merged = load_merged(path, &mut Vec::new())?;
        // Through text and back: reading a YAML value directly is stricter than reading text, for example it will not turn the
        // number 1 into the text "1" for `release: 1`.
        let merged = serde_yaml_ng::to_string(&merged)?;
        let mut manifest: Manifest = serde_yaml_ng::from_str(&merged)
            .with_context(|| format!("{} is not a valid manifest", path.display()))?;
        manifest.base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        if manifest.base.as_os_str().is_empty() {
            manifest.base = PathBuf::from(".");
        }
        manifest.context = manifest.base.clone();
        manifest.check_jobs()?;
        Ok(manifest)
    }

    /// Refuse what cannot work, when the manifest loads: a step that reads something its job does not have, a target that
    /// names no job, a bad variable name.
    fn check_jobs(&self) -> Result<()> {
        let valid_name = |key: &str| {
            key.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        for (name, job) in &self.jobs {
            if job.runs_on.trim().is_empty() {
                bail!("job {name}: runs-on is empty");
            }
            if job.steps.is_empty() {
                bail!("job {name} has no steps");
            }
            let mut keys: Vec<String> = job.matrix.axes.keys().cloned().collect();
            for entry in &job.matrix.include {
                keys.extend(entry.keys().cloned());
            }
            let all_steps = || job.setup.iter().chain(job.steps.iter());
            for key in job
                .env
                .keys()
                .chain(all_steps().flat_map(|step| step.env.keys()))
            {
                if !valid_name(key) {
                    bail!(
                        "job {name}: env name {key:?} must be letters, digits and _, not starting with a digit"
                    );
                }
            }
            for (n, step) in all_steps().enumerate() {
                let Some(condition) = &step.condition else {
                    continue;
                };
                for used in expr::names(condition)
                    .with_context(|| format!("job {name}, step {}: the if: {condition:?}", n + 1))?
                {
                    let allowed = used == "arch"
                        || used.starts_with("env.")
                        || used
                            .strip_prefix("matrix.")
                            .is_some_and(|key| keys.iter().any(|k| k == key));
                    if !allowed {
                        let why = if used == "format"
                            || used == "distro"
                            || used.starts_with("matrix.")
                        {
                            "a job reads only its own matrix, so what it builds cannot depend on the target"
                        } else {
                            "a step reads arch, env.NAME and the matrix of its job"
                        };
                        bail!(
                            "job {name}, step {}: the if: {condition:?} reads {used}; {why}",
                            n + 1
                        );
                    }
                }
            }
        }
        for target in &self.targets {
            if let Some(Needs::Job(job)) = &target.needs
                && !self.jobs.contains_key(job)
            {
                bail!(
                    "a target needs the job {job:?}, which does not exist (jobs: {})",
                    self.jobs.keys().cloned().collect::<Vec<_>>().join(", ")
                );
            }
            let taken = target
                .extra
                .keys()
                .map(String::as_str)
                .chain(["format", "distro", "needs", "file", "if"]);
            for key in taken {
                if self.matrix.axes.contains_key(key) {
                    bail!(
                        "the matrix axis {key:?} is also a key of a target; a target sets {key:?} for itself, so remove the axis"
                    );
                }
            }
        }
        Ok(())
    }

    /// Refuse a top-level key that is neither a field of the manifest nor the name of a format, so a typo such as
    /// `dependz:` does not vanish. `formats` are the names the loaded plugins write.
    pub fn check_options(&self, formats: &[String]) -> Result<()> {
        for key in self.options.keys() {
            if [
                "image",
                "setup",
                "build",
                "build_axes",
                "exclude",
                "builders",
            ]
            .contains(&key.as_str())
            {
                bail!(
                    "{key:?} is not a key of the manifest any more: a job builds the files (jobs: NAME: runs-on, setup, steps, exclude) and a target names it with needs. See docs/build-environment.md"
                );
            }
            if !formats.contains(key) {
                bail!(
                    "unknown key {key:?} in the manifest: it is not a field, and no plugin writes a format with that name (formats: {})",
                    formats.join(", ")
                );
            }
        }
        Ok(())
    }

    /// Every package to make: the cells of the `matrix`, each crossed with every target and every format of the target.
    /// `exclude` of the matrix drops cells after the target's values are in, so it can name `distro` and `format`.
    pub fn jobs(&self) -> Vec<Job> {
        if self.targets.is_empty() {
            return self.matrix.jobs();
        }
        let mut matrix = self.matrix.clone();
        matrix.exclude.clear();
        let mut out = Vec::new();
        for cell in matrix.jobs() {
            for target in &self.targets {
                for format in target.format.all() {
                    let mut values = cell.values.clone();
                    values.insert("format".into(), format);
                    if let Some(distro) = &target.distro {
                        values.insert("distro".into(), distro.clone());
                    }
                    match &target.needs {
                        Some(Needs::Job(job)) => {
                            values.insert("needs".into(), job.clone());
                        }
                        Some(Needs::Nothing) => {
                            values.insert("needs".into(), String::new());
                        }
                        None => {}
                    }
                    if let Some(file) = &target.file {
                        values.insert("file".into(), file.clone());
                    }
                    if let Some(condition) = &target.condition {
                        values.insert("if".into(), condition.clone());
                    }
                    for (key, value) in &target.extra {
                        values.insert(key.clone(), value.text());
                    }
                    let dropped = self
                        .matrix
                        .exclude
                        .iter()
                        .any(|entry| matches_entry(&values, entry, &|_| true));
                    if !dropped {
                        out.push(Job { values });
                    }
                }
            }
        }
        out
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
        bound.select_job(&context, job)?;
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

    /// Find the build job of a package target and fill in `image`, `setup`, `build`, `exclude` and `build_env` from it.
    ///
    /// The target's `needs` names the job. Without it, the one job of the manifest is used, and a manifest with several
    /// jobs asks for the name. `needs: []` is no job, and the target packs `contents` only. The run of the job is the cell
    /// of its own matrix that agrees with the target on every axis of that matrix. A job with no `arch` axis runs for the
    /// architecture of the target.
    fn select_job(&mut self, context: &Scope, cell: &Job) -> Result<()> {
        let name = match cell.values.get("needs").map(String::as_str) {
            Some("") => return Ok(()),
            Some(name) => name.to_string(),
            None => match self.jobs.len() {
                0 => return Ok(()),
                1 => self.jobs.keys().next().cloned().expect("one job"),
                _ => bail!(
                    "the target {} does not say which job builds its files; add needs: JOB, or needs: [] for none (jobs: {})",
                    cell.label(),
                    self.jobs.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            },
        };
        let Some(job) = self.jobs.get(&name).cloned() else {
            bail!(
                "the target {} needs the job {name:?}, which does not exist",
                cell.label()
            );
        };
        let wanted = |key: &str| -> Option<String> {
            if key == "arch" {
                Some(context.arch.clone())
            } else {
                cell.values.get(key).cloned()
            }
        };
        let axes: Vec<&String> = job.matrix.axes.keys().collect();
        let mut missing = Vec::new();
        let run = job.matrix.jobs().into_iter().find(|run| {
            axes.iter().all(|key| match wanted(key) {
                Some(value) => run.values.get(*key) == Some(&value),
                None => {
                    missing.push((*key).clone());
                    false
                }
            })
        });
        if let Some(key) = missing.first() {
            bail!(
                "job {name} has the matrix key {key:?}, but the target {} has no value for it",
                cell.label()
            );
        }
        let Some(run) = run else {
            bail!(
                "job {name} has no run for the target {}: its matrix has no cell that agrees on {}",
                cell.label(),
                axes.iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        // A job that pins the architecture must agree with the target. Without one it builds for the target's.
        let arch = run
            .values
            .get("arch")
            .cloned()
            .unwrap_or_else(|| context.arch.clone());

        let mut steps = context.clone();
        steps.format = String::new();
        steps.distro = String::new();
        steps.arch = arch.clone();
        steps.matrix = run.values.clone();
        steps.matrix.insert("arch".into(), arch.clone());

        let mut env = BTreeMap::new();
        for (key, value) in &job.env {
            env.insert(key.clone(), steps.expand_expressions(value)?);
        }
        let render = |list: &[Step], phase: &str| -> Result<Vec<String>> {
            let mut out = Vec::new();
            for (n, step) in list.iter().enumerate() {
                if let Some(condition) = &step.condition
                    && !steps.test(condition).with_context(|| {
                        format!("in the if: of {phase} step {} of job {name}", n + 1)
                    })?
                {
                    continue;
                }
                let mut step_env = BTreeMap::new();
                for (key, value) in &step.env {
                    step_env.insert(key.clone(), steps.expand_expressions(value)?);
                }
                let title = step
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("{phase} {}", n + 1));
                out.push(render_step(
                    &title,
                    &env,
                    &step_env,
                    &steps.expand_expressions(&step.run)?,
                ));
            }
            Ok(out)
        };
        let setup = render(&job.setup, "setup")?;
        let build = render(&job.steps, "step")?;
        if build.is_empty() {
            bail!(
                "job {name} has no step to run for the target {}: every step has a false if:",
                cell.label()
            );
        }
        self.image = Some(context.expand(&job.runs_on)?);
        self.exclude = job.exclude.clone();
        self.arch = arch.clone();
        self.setup = (!setup.is_empty()).then(|| setup.join("\n"));
        self.build = Some(build.join("\n"));

        let mut build_env = BTreeMap::new();
        build_env.insert("PKGSMITH_NAME".into(), self.name.clone());
        build_env.insert("PKGSMITH_VERSION".into(), self.version.clone());
        build_env.insert("PKGSMITH_RELEASE".into(), self.release.clone());
        build_env.insert("PKGSMITH_ARCH".into(), arch);
        for (key, value) in &steps.matrix {
            let key: String = key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            build_env.insert(format!("MATRIX_{key}"), value.clone());
        }
        self.build_env = build_env;
        Ok(())
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
            // A file or config entry without a src changes the tracked file at dst, so only the rest need a src.
            let needs_src = matches!(content.kind, Kind::Tree | Kind::Symlink);
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
    fn a_key_that_is_not_a_field_or_a_format_is_refused_so_a_typo_does_not_vanish() {
        let formats = ["deb".to_string(), "rpm".to_string()];
        assert!(
            parse(&format!("{BASE}deb: {{section: utils}}\n"))
                .unwrap()
                .check_options(&formats)
                .is_ok()
        );
        assert!(
            parse(&format!("{BASE}dependz: [a]\n"))
                .unwrap()
                .check_options(&formats)
                .is_err()
        );
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
                &format!("{BASE}contents:\n  - {{dst: /a, type: tree}}\n"),
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
    fn an_include_entry_does_not_match_a_job_that_an_earlier_entry_added() {
        let labels = jobs(
            "  arch: [amd64]\n  include:\n    - {format: msi, arch: arm64, distro: windows}\n    - {format: nsis, arch: arm64, distro: windows}\n",
        );
        assert_eq!(
            labels,
            [
                "arch=amd64",
                "arch=arm64, distro=windows, format=msi",
                "arch=arm64, distro=windows, format=nsis"
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

    /// Two jobs, five targets: rpm, deb and Arch on one Debian job, Alpine on its own, and a dmg that packs files only.
    const JOBS: &str = "jobs:\n  linux:\n    runs-on: debian:13\n    setup:\n      - name: tools\n        run: apt-get update\n    steps:\n      - name: compile\n        run: make\n      - name: arm only\n        if: arch == 'arm64'\n        run: echo arm\n  alpine:\n    runs-on: alpine:3.22\n    steps:\n      - run: make\ntargets:\n  - format: rpm\n    distro: fedora\n    needs: linux\n  - format: [deb, tar]\n    distro: debian\n    needs: linux\n  - format: apk\n    distro: alpine\n    needs: alpine\n  - format: dmg\n    distro: macos\n    needs: []\n";

    fn cells(text: &str) -> Vec<String> {
        let mut labels: Vec<String> = parse(text).unwrap().jobs().iter().map(Job::label).collect();
        labels.sort();
        labels
    }

    #[test]
    fn targets_are_crossed_with_the_matrix_and_a_format_list_makes_one_package_each() {
        let text = format!("{BASE}matrix:\n  arch: [amd64, arm64]\n{JOBS}");
        let all = cells(&text);
        assert_eq!(all.len(), 10, "5 packages x 2 architectures: {all:?}");
        assert!(all.contains(&"arch=arm64, distro=debian, format=tar".to_string()));
    }

    #[test]
    fn exclude_can_name_the_distro_of_a_target() {
        let text = format!(
            "{BASE}matrix:\n  arch: [amd64, arm64]\n  exclude:\n    - {{distro: alpine, arch: arm64}}\n{JOBS}"
        );
        let all = cells(&text);
        assert_eq!(all.len(), 9);
        assert!(
            !all.iter()
                .any(|l| l.contains("alpine") && l.contains("arm64"))
        );
    }

    #[test]
    fn a_target_uses_its_job_and_needs_nothing_means_no_build() {
        let text = format!("{BASE}{JOBS}");
        let fedora = bind(
            &text,
            job(&[("distro", "fedora"), ("needs", "linux"), ("arch", "amd64")]),
            "rpm",
        )
        .unwrap();
        assert_eq!(fedora.image.as_deref(), Some("debian:13"));
        assert!(fedora.setup.as_deref().unwrap().contains("apt-get update"));
        assert!(fedora.build.as_deref().unwrap().contains("make"));
        assert!(!fedora.build.as_deref().unwrap().contains("echo arm"));
        let arm = bind(&text, job(&[("needs", "linux"), ("arch", "arm64")]), "rpm").unwrap();
        assert!(arm.build.as_deref().unwrap().contains("echo arm"));
        let alpine = bind(&text, job(&[("needs", "alpine")]), "apk").unwrap();
        assert_eq!(alpine.image.as_deref(), Some("alpine:3.22"));
        assert!(alpine.setup.is_none());
        let dmg = bind(&text, job(&[("needs", "")]), "dmg").unwrap();
        assert!(dmg.image.is_none() && dmg.build.is_none());
    }

    #[test]
    fn the_job_is_optional_with_one_job_and_required_with_several() {
        let one = format!(
            "{BASE}jobs:\n  build:\n    runs-on: a\n    steps:\n      - run: x\ntargets:\n  - format: rpm\n"
        );
        assert_eq!(
            bind(&one, job(&[]), "rpm").unwrap().image.as_deref(),
            Some("a")
        );
        let several = format!("{BASE}{JOBS}");
        let error = bind(&several, job(&[("distro", "x")]), "rpm").unwrap_err();
        assert!(
            format!("{error:#}").contains("does not say which job"),
            "{error:#}"
        );
    }

    #[test]
    fn a_target_that_needs_a_missing_job_is_refused_when_the_manifest_loads() {
        let text = format!(
            "{BASE}jobs:\n  a:\n    runs-on: a\n    steps:\n      - run: x\ntargets:\n  - format: rpm\n    needs: b\n"
        );
        let error = parse(&text).unwrap_err();
        assert!(
            format!("{error:#}").contains("needs the job \"b\""),
            "{error:#}"
        );
        let error = parse(&format!(
            "{BASE}targets:\n  - format: rpm\n    needs: [x]\n"
        ))
        .unwrap_err();
        assert!(format!("{error:#}").contains("one job name"), "{error:#}");
    }

    #[test]
    fn a_job_only_sees_its_own_matrix_and_a_target_takes_the_run_that_agrees() {
        let text = format!(
            "{BASE}jobs:\n  a:\n    runs-on: a\n    matrix:\n      arch: [amd64, arm64]\n      libc: [gnu]\n    steps:\n      - run: echo $MATRIX_LIBC $MATRIX_ARCH\n        if: matrix.libc == 'gnu'\ntargets:\n  - format: rpm\n    needs: a\n    libc: gnu\n"
        );
        let bound = bind(
            &text,
            job(&[("needs", "a"), ("arch", "arm64"), ("libc", "gnu")]),
            "rpm",
        )
        .unwrap();
        assert_eq!(
            bound.build_env.get("MATRIX_LIBC").map(String::as_str),
            Some("gnu")
        );
        assert_eq!(
            bound.build_env.get("PKGSMITH_ARCH").map(String::as_str),
            Some("arm64")
        );
        assert!(
            bound
                .build_env
                .keys()
                .all(|k| k != "MATRIX_DISTRO" && k != "MATRIX_FORMAT")
        );
        let missing = bind(&text, job(&[("needs", "a"), ("arch", "arm64")]), "rpm").unwrap_err();
        assert!(
            format!("{missing:#}").contains("no value for it"),
            "{missing:#}"
        );
    }

    #[test]
    fn a_step_cannot_read_what_its_job_does_not_have() {
        for (cond, wanted) in [
            ("format == 'rpm'", "reads format"),
            ("distro == 'fedora'", "reads distro"),
            ("matrix.distro == 'fedora'", "reads matrix.distro"),
        ] {
            let text = format!(
                "{BASE}jobs:\n  a:\n    runs-on: a\n    steps:\n      - if: {cond}\n        run: x\n"
            );
            let error = parse(&text).unwrap_err();
            assert!(format!("{error:#}").contains(wanted), "{cond}: {error:#}");
        }
    }

    #[test]
    fn the_old_image_setup_and_build_keys_are_refused_with_the_new_place() {
        let manifest = parse(&format!("{BASE}image: fedora:43\nbuild: make\n")).unwrap();
        let error = manifest.check_options(&["rpm".to_string()]).unwrap_err();
        assert!(
            format!("{error}").contains("a job builds the files"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn env_values_stay_literal_and_the_state_files_are_data_not_shell() {
        let tricky = "it's \"quoted\" $HOME `id` $(id)";
        let text = format!(
            "{BASE}jobs:\n  a:\n    runs-on: a\n    env:\n      GREETING: \"{}\"\n    setup:\n      - run: |\n          printf 'FROM_SETUP=%s\\n' \"$GREETING\" >> \"$PKGSMITH_ENV\"\n          echo /opt/tool/bin >> \"$PKGSMITH_PATH\"\n    steps:\n      - run: |\n          printf '%s|%s|%s' \"$GREETING\" \"$FROM_SETUP\" \"$PATH\"\n          echo\n          echo PKGSMITH_END\n",
            tricky.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let bound = bind(&text, Job::default(), "rpm").unwrap();
        // Setup and build share one container in real life, so the state directory stays between them. Run both here.
        let script = format!(
            "{}\n{}",
            bound.setup.as_deref().unwrap(),
            bound.build.as_deref().unwrap()
        );
        let dir = tempfile::tempdir().unwrap();
        let output = std::process::Command::new("sh")
            .current_dir(dir.path())
            .args([
                "-ec",
                &script.replace(
                    "/tmp/pkgsmith-state",
                    &format!("{}/state", dir.path().display()),
                ),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("==> step 1"), "{stdout}");
        assert!(
            stdout.contains(&format!("{tricky}|{tricky}|/opt/tool/bin:")),
            "{stdout}"
        );
        assert!(stdout.contains("PKGSMITH_END"), "{stdout}");
    }

    #[cfg(unix)]
    #[test]
    fn a_bad_line_in_the_env_file_stops_the_step() {
        let text = format!(
            "{BASE}jobs:\n  a:\n    runs-on: a\n    setup:\n      - run: echo 'not valid' >> \"$PKGSMITH_ENV\"\n    steps:\n      - run: echo unreachable\n"
        );
        let bound = bind(&text, Job::default(), "rpm").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "{}\n{}",
            bound.setup.as_deref().unwrap(),
            bound.build.as_deref().unwrap()
        )
        .replace(
            "/tmp/pkgsmith-state",
            &format!("{}/state", dir.path().display()),
        );
        let output = std::process::Command::new("sh")
            .args(["-ec", &script])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("unreachable"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("not NAME=value"));
    }
}
