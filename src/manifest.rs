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
            .filter(|(k, _)| k.as_str() != "file")
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

/// A place that builds files: a container image and the steps that run in it, in order.
///
/// A job uses the builder whose `if` is true. No builder is a job without a build, which packages `contents` only, and
/// two builders are an error. Jobs that use one builder, and have equal values for the keys in `by` and the same
/// architecture, share one run of its steps.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Builder {
    /// A container image. It is pulled when it is not on the machine.
    pub image: String,
    /// An expression over `matrix.NAME`, `distro`, `arch` and `env.NAME`. It cannot read `format`: a build must not
    /// depend on the format it is packaged as. Without it, every job uses this builder.
    #[serde(rename = "if")]
    pub condition: Option<String>,
    /// The matrix keys, besides `arch`, that the steps may read and that make a separate run. Everything else is hidden
    /// from the steps, which is what lets jobs of different distros share one run.
    #[serde(default)]
    pub by: Vec<String>,
    /// Values every step gets as environment variables. They are literal: nothing in them is expanded by a shell.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Paths (globs) that the steps install and the package leaves out.
    #[serde(default)]
    pub exclude: Vec<String>,
    pub steps: Vec<Step>,
}

/// One script of a builder. Each step runs in a fresh `sh -e` in `/src`, so a step does not see what an earlier step
/// exported or where it changed directory. It sees the files the earlier steps wrote.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Shown in the progress output.
    pub name: Option<String>,
    /// The script. A file with a `#!` line is not special here: the text always runs with `sh -e`.
    pub run: String,
    /// An expression that decides whether the step runs. It sees the same names as the `if` of its builder, but only the
    /// matrix keys in the `by` of the builder.
    #[serde(rename = "if")]
    pub condition: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// A leading step that runs once and is cached as an image, for example to install the build dependencies. What it
    /// installs is not packaged. Every `setup` step comes before every other step.
    #[serde(default)]
    pub setup: bool,
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
    /// The places that build the files: each is a container image and the steps that run in it. A job uses the one
    /// whose `if` is true, and jobs that use the same builder share one run.
    #[serde(default)]
    pub builders: BTreeMap<String, Builder>,
    /// What the selected builder gives this job. Filled in by `bind`, never read from the file.
    #[serde(skip)]
    pub image: Option<String>,
    /// The leading steps of the builder, with the conditions decided. They run once and are cached. Not tracked.
    #[serde(skip)]
    pub setup: Option<String>,
    /// The other steps. Whatever they write outside `/src` is packaged.
    #[serde(skip)]
    pub build: Option<String>,
    /// Paths (globs) the build installed that the package leaves out.
    #[serde(skip)]
    pub exclude: Vec<String>,
    /// The matrix keys the selected builder sees, besides the architecture.
    #[serde(skip)]
    pub by: Vec<String>,
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

/// A step as shell text: a title line, then the script in a fresh `sh -e` with the environment as literal variables. The
/// delimiter of the here document comes from the script itself, so no line of the script can end it early.
fn render_step(title: &str, env: &BTreeMap<String, String>, script: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut body = String::new();
    for (key, value) in env {
        body.push_str(&format!("export {key}={}\n", sh_quote(value)));
    }
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
        manifest.check_builders()?;
        Ok(manifest)
    }

    /// The names a builder or a step may read. Refusing the rest at load time is what makes a shared build sound: a step
    /// cannot depend on a value that the jobs sharing its run do not share.
    fn check_builders(&self) -> Result<()> {
        let mut known: Vec<String> = self.matrix.axes.keys().cloned().collect();
        for entry in &self.matrix.include {
            known.extend(entry.keys().cloned());
        }
        known.extend(["arch".into(), "distro".into()]);
        for (name, builder) in &self.builders {
            for key in &builder.by {
                if !known.contains(key) {
                    bail!(
                        "builder {name}: by lists {key:?}, which is not a matrix key (matrix keys: {})",
                        known.join(", ")
                    );
                }
            }
            if builder.steps.is_empty() {
                bail!("builder {name} has no steps");
            }
            let envs = builder
                .env
                .keys()
                .chain(builder.steps.iter().flat_map(|step| step.env.keys()));
            for key in envs {
                let valid = key
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !valid {
                    bail!(
                        "builder {name}: env name {key:?} must be letters, digits and _, not starting with a digit"
                    );
                }
            }
            let mut after_build = false;
            for (n, step) in builder.steps.iter().enumerate() {
                if step.setup && after_build {
                    bail!(
                        "builder {name}: step {} is a setup step after a step that is not; setup steps come first",
                        n + 1
                    );
                }
                after_build |= !step.setup;
            }
            let check = |expression: &str, owner: String, by: Option<&[String]>| -> Result<()> {
                for used in expr::names(expression)
                    .with_context(|| format!("{owner}: the if: {expression:?}"))?
                {
                    let allowed = match (used.as_str(), by) {
                        ("arch", _) => true,
                        (u, _) if u.starts_with("env.") => true,
                        (u, None) if u == "distro" || u.starts_with("matrix.") => true,
                        ("distro", Some(by)) => by.iter().any(|k| k == "distro"),
                        (u, Some(by)) => u
                            .strip_prefix("matrix.")
                            .is_some_and(|key| by.iter().any(|k| k == key)),
                        _ => false,
                    };
                    if !allowed {
                        let why = if used == "format" {
                            "a build must not depend on the format".to_string()
                        } else if by.is_some() {
                            "a step reads only arch, env.NAME and the matrix keys in the by of its builder".to_string()
                        } else {
                            "a builder reads only arch, distro, matrix.NAME and env.NAME"
                                .to_string()
                        };
                        bail!("{owner}: the if: {expression:?} reads {used}; {why}");
                    }
                }
                Ok(())
            };
            if let Some(condition) = &builder.condition {
                check(condition, format!("builder {name}"), None)?;
            }
            for (n, step) in builder.steps.iter().enumerate() {
                if let Some(condition) = &step.condition {
                    check(
                        condition,
                        format!("builder {name}, step {}", n + 1),
                        Some(&builder.by),
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Refuse a top-level key that is neither a field of the manifest nor the name of a format, so a typo such as
    /// `dependz:` does not vanish. `formats` are the names the loaded plugins write.
    pub fn check_options(&self, formats: &[String]) -> Result<()> {
        for key in self.options.keys() {
            if ["image", "setup", "build", "build_axes", "exclude"].contains(&key.as_str()) {
                bail!(
                    "{key:?} is not a key of the manifest any more: image, setup, build and exclude moved into a builder (builders: NAME: image, steps, exclude). See docs/build-environment.md"
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
        bound.select_builder(&context, job)?;
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

    /// Pick the builder of a job and fill in `image`, `setup`, `build`, `exclude` and `by` from it. No builder is a job
    /// without a build. Two builders are an error, so a typo in an `if:` cannot quietly change what is built.
    fn select_builder(&mut self, context: &Scope, job: &Job) -> Result<()> {
        let mut chosen: Vec<&str> = Vec::new();
        for (name, builder) in &self.builders {
            let uses = match &builder.condition {
                Some(condition) => context
                    .test(condition)
                    .with_context(|| format!("in the if: of builder {name}"))?,
                None => true,
            };
            if uses {
                chosen.push(name);
            }
        }
        let name = match chosen.as_slice() {
            [] => return Ok(()),
            [one] => (*one).to_string(),
            several => bail!(
                "the job {} uses {} builders ({}); the if: of each builder must pick different jobs",
                job.label(),
                several.len(),
                several.join(", ")
            ),
        };
        let builder = self.builders[&name].clone();
        self.image = Some(context.expand(&builder.image)?);
        self.exclude = builder.exclude.clone();
        self.by = builder.by.clone();

        // The steps see the architecture and the matrix keys in `by`, never the format or any other distro value.
        let mut steps = context.clone();
        steps.format = String::new();
        steps.matrix = job
            .values
            .iter()
            .filter(|(key, _)| builder.by.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if !builder.by.iter().any(|key| key == "distro") {
            steps.distro = String::new();
        }
        let (mut setup, mut build) = (Vec::new(), Vec::new());
        for (n, step) in builder.steps.iter().enumerate() {
            if let Some(condition) = &step.condition
                && !steps
                    .test(condition)
                    .with_context(|| format!("in the if: of step {} of builder {name}", n + 1))?
            {
                continue;
            }
            let mut env = BTreeMap::new();
            for (key, value) in builder.env.iter().chain(step.env.iter()) {
                env.insert(key.clone(), steps.expand_expressions(value)?);
            }
            let title = step
                .name
                .clone()
                .unwrap_or_else(|| format!("step {}", n + 1));
            let text = render_step(&title, &env, &steps.expand_expressions(&step.run)?);
            if step.setup {
                setup.push(text);
            } else {
                build.push(text);
            }
        }
        if build.is_empty() {
            bail!(
                "builder {name} has no step to run for the job {}: every step that is not a setup step has a false if:",
                job.label()
            );
        }
        self.setup = (!setup.is_empty()).then(|| setup.join("\n"));
        self.build = Some(build.join("\n"));
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

    const LINUX: &str = "builders:\n  linux:\n    if: matrix.distro == 'fedora' || matrix.distro == 'debian'\n    image: debian:13\n    steps:\n      - name: tools\n        setup: true\n        run: apt-get update\n      - name: compile\n        run: make\n      - name: arm only\n        if: arch == 'arm64'\n        run: echo arm\n  alpine:\n    if: matrix.distro == 'alpine'\n    image: alpine:3.22\n    steps:\n      - run: make\n";

    fn with_builders(extra: &str) -> String {
        format!("{BASE}matrix:\n  distro: [fedora, alpine]\n{extra}")
    }

    #[test]
    fn a_job_uses_the_builder_whose_if_is_true_and_a_job_with_none_has_no_build() {
        let text = with_builders(LINUX);
        let fedora = bind(&text, job(&[("distro", "fedora")]), "rpm").unwrap();
        assert_eq!(fedora.image.as_deref(), Some("debian:13"));
        assert!(fedora.setup.as_deref().unwrap().contains("apt-get update"));
        assert!(fedora.build.as_deref().unwrap().contains("make"));
        let alpine = bind(&text, job(&[("distro", "alpine")]), "apk").unwrap();
        assert_eq!(alpine.image.as_deref(), Some("alpine:3.22"));
        assert!(alpine.setup.is_none());
        let macos = bind(&text, job(&[("distro", "macos")]), "dmg").unwrap();
        assert!(macos.image.is_none() && macos.build.is_none());
    }

    #[test]
    fn two_builders_for_one_job_is_an_error() {
        let text = with_builders(
            "builders:\n  a:\n    image: a\n    steps:\n      - run: x\n  b:\n    image: b\n    steps:\n      - run: x\n",
        );
        let error = bind(&text, job(&[("distro", "fedora")]), "rpm").unwrap_err();
        assert!(
            format!("{error:#}").contains("uses 2 builders"),
            "{error:#}"
        );
    }

    #[test]
    fn a_builder_cannot_read_the_format_or_a_step_a_key_it_is_not_split_by() {
        for (extra, wanted) in [
            (
                "builders:\n  a:\n    if: format == 'rpm'\n    image: a\n    steps:\n      - run: x\n",
                "reads format",
            ),
            (
                "builders:\n  a:\n    image: a\n    steps:\n      - if: distro == 'fedora'\n        run: x\n",
                "reads distro",
            ),
            (
                "builders:\n  a:\n    image: a\n    steps:\n      - if: matrix.distro == 'fedora'\n        run: x\n",
                "reads matrix.distro",
            ),
        ] {
            let error = parse(&with_builders(extra)).unwrap_err();
            assert!(format!("{error:#}").contains(wanted), "{extra}: {error:#}");
        }
        // The same step is fine when the builder is split by that key.
        let text = with_builders(
            "builders:\n  a:\n    image: a\n    by: [distro]\n    steps:\n      - if: matrix.distro == 'fedora'\n        run: x\n",
        );
        assert!(parse(&text).is_ok());
    }

    #[test]
    fn by_must_name_a_matrix_key_and_setup_steps_must_come_first() {
        let error = parse(&with_builders(
            "builders:\n  a:\n    image: a\n    by: [distroo]\n    steps:\n      - run: x\n",
        ))
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("not a matrix key"),
            "{error:#}"
        );
        let error = parse(&with_builders(
            "builders:\n  a:\n    image: a\n    steps:\n      - run: x\n      - setup: true\n        run: y\n",
        ))
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("setup steps come first"),
            "{error:#}"
        );
    }

    #[test]
    fn the_old_image_setup_and_build_keys_are_refused_with_the_new_place() {
        let manifest = parse(&format!("{BASE}image: fedora:43\nbuild: make\n")).unwrap();
        let error = manifest.check_options(&["rpm".to_string()]).unwrap_err();
        assert!(
            format!("{error}").contains("moved into a builder"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_step_gets_its_env_as_literal_text_and_the_script_cannot_end_the_heredoc_early() {
        let tricky = "it's \"quoted\" $HOME `id` $(id)";
        let text = format!(
            "{BASE}builders:\n  a:\n    image: a\n    env:\n      GREETING: \"{}\"\n    steps:\n      - run: |\n          printf '%s' \"$GREETING\"\n          echo\n          echo PKGSMITH_END\n",
            tricky.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let bound = bind(&text, Job::default(), "rpm").unwrap();
        let output = std::process::Command::new("sh")
            .args(["-ec", bound.build.as_deref().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("==> step 1"), "{stdout}");
        assert!(stdout.contains(tricky), "{stdout}");
        assert!(stdout.contains("PKGSMITH_END"), "{stdout}");
    }
}
