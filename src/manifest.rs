//! What a script describes: package targets, and the jobs that build their files.
//!
//! `script.rs` runs `pkgsmith.star` and gives a [`TargetSpec`] for every target the script declares. [`Manifest::from_spec`]
//! checks a spec and turns it into the [`Manifest`] that the rest of pkgsmith works from: the fields of one package in one
//! format, and, when the target has a job, the scripts that build its files.

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
    /// Variables for this step, after those of the job. The values are literal text.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// A job that builds files: a container image and ordered steps. Its result is the file tree that the steps install.
///
/// Two targets whose jobs are equal share one run, so a job that three formats need is built once. A job sees only
/// its own fields and the architecture, never the format or the distro of a target.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSpec {
    /// The container image the steps run in. It is pulled when it is not on the machine.
    pub runs_on: String,
    /// Steps that make the files. Whatever they install outside `/src` is packaged.
    pub steps: Vec<Step>,
    /// Steps that prepare the environment, for example by installing compilers. They run once, and the result is cached
    /// as an image. What they install is not packaged.
    #[serde(default)]
    pub setup: Vec<Step>,
    /// Variables for every step. The values are literal text: nothing in them is expanded by a shell.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Paths (globs) that the steps install and the package leaves out.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The architecture the job runs for. Without it, the job runs for the architecture of the target.
    pub arch: Option<String>,
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
    /// The job that builds the files. Without one, the package holds its `contents` only.
    pub job: Option<JobSpec>,
}

/// One package as the rest of pkgsmith sees it. A target of a script, checked, with the scripts of its job rendered.
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
    /// The container image of the job, when there is one.
    pub image: Option<String>,
    /// The setup steps of the job as one script. They run once and are cached. Not tracked.
    pub setup: Option<String>,
    /// The other steps as one script. Whatever they write outside `/src` is packaged.
    pub build: Option<String>,
    /// Paths (globs) the build installed that the package leaves out.
    pub exclude: Vec<String>,
    /// What the steps may read: `PKGSMITH_*`.
    pub build_env: BTreeMap<String, String>,
    /// The folder of the script. Relative `src` paths and script files resolve against it.
    pub base: PathBuf,
    /// The build context: the folder mounted at `/src` in a tracked build.
    pub context: PathBuf,
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
    /// `context` the folder mounted at `/src`.
    pub fn from_spec(spec: TargetSpec, base: PathBuf, context: PathBuf) -> Result<Manifest> {
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
            context,
            ..Manifest::default()
        };
        if manifest.options.as_object().is_some_and(|o| o.is_empty()) {
            manifest.options = serde_json::Value::Null;
        }
        manifest.check()?;
        if let Some(job) = spec.job {
            manifest.use_job(job)?;
        }
        Ok(manifest)
    }

    /// Fill in `image`, `setup`, `build`, `exclude` and `build_env` from the job.
    fn use_job(&mut self, job: JobSpec) -> Result<()> {
        if job.runs_on.trim().is_empty() {
            bail!("the job has an empty runs_on");
        }
        if job.steps.is_empty() {
            bail!("the job has no steps");
        }
        let arch = match &job.arch {
            Some(arch) if !["amd64", "arm64", "all"].contains(&arch.as_str()) => {
                bail!("the job arch {arch:?} must be amd64, arm64 or all")
            }
            Some(arch) => arch.clone(),
            None => self.arch.clone(),
        };
        let valid_name = |key: &str| {
            key.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        for key in job.env.keys().chain(
            job.setup
                .iter()
                .chain(&job.steps)
                .flat_map(|s| s.env.keys()),
        ) {
            if !valid_name(key) {
                bail!(
                    "the job has the env name {key:?}; use letters, digits and _, not starting with a digit"
                );
            }
        }
        let render = |steps: &[Step], phase: &str| -> Vec<String> {
            steps
                .iter()
                .enumerate()
                .map(|(n, step)| {
                    let title = step
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("{phase} {}", n + 1));
                    render_step(&title, &job.env, &step.env, &step.run)
                })
                .collect()
        };
        let setup = render(&job.setup, "setup");
        let build = render(&job.steps, "step");
        self.image = Some(job.runs_on.clone());
        self.exclude = job.exclude.clone();
        self.setup = (!setup.is_empty()).then(|| setup.join("\n"));
        self.build = Some(build.join("\n"));
        self.arch = arch.clone();
        let mut env = BTreeMap::new();
        env.insert("PKGSMITH_NAME".into(), self.name.clone());
        env.insert("PKGSMITH_VERSION".into(), self.version.clone());
        env.insert("PKGSMITH_RELEASE".into(), self.release.clone());
        env.insert("PKGSMITH_ARCH".into(), arch);
        self.build_env = env;
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
        assert!(m.image.is_none() && m.build.is_none());
    }

    #[test]
    fn release_may_be_a_number_and_an_unknown_field_is_an_error() {
        assert_eq!(build(json!({"release": 3})).unwrap().release, "3");
        let error = Manifest::parse_spec(json!({"name": "a", "version": "1", "format": "rpm", "description": "d", "dependz": []}))
            .unwrap_err();
        assert!(format!("{error:#}").contains("dependz"), "{error:#}");
    }

    #[test]
    fn a_job_makes_the_scripts_and_a_job_arch_wins() {
        let m = build(json!({
            "arch": "arm64",
            "job": {"runs_on": "debian:13", "arch": "amd64",
                    "setup": [{"name": "tools", "run": "apt-get update"}],
                    "steps": [{"run": "make"}],
                    "exclude": ["/usr/share/doc/**"]}
        }))
        .unwrap();
        assert_eq!(m.image.as_deref(), Some("debian:13"));
        assert!(m.setup.as_deref().unwrap().contains("apt-get update"));
        assert!(m.build.as_deref().unwrap().contains("make"));
        assert_eq!(m.build_env["PKGSMITH_ARCH"], "amd64");
        assert_eq!(m.exclude, ["/usr/share/doc/**"]);
    }

    #[test]
    fn bad_names_modes_and_jobs_are_refused() {
        assert!(build(json!({"name": "Bad Name"})).is_err());
        assert!(build(json!({"arch": "sparc"})).is_err());
        assert!(build(json!({"contents": [{"dst": "relative"}]})).is_err());
        assert!(build(json!({"contents": [{"dst": "/a", "type": "symlink"}]})).is_err());
        assert!(build(json!({"job": {"runs_on": "a", "steps": []}})).is_err());
        assert!(
            build(json!({"job": {"runs_on": "a", "steps": [{"run": "x", "env": {"1BAD": "v"}}]}}))
                .is_err()
        );
        let error = Manifest::parse_spec(
            json!({"name": "a", "version": "1", "format": "rpm", "description": "d",
            "contents": [{"dst": "/a", "mode": "9"}]}),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("accept"), "{error:#}");
    }

    #[cfg(unix)]
    #[test]
    fn env_values_stay_literal_and_the_state_files_are_data_not_shell() {
        let tricky = "it's \"quoted\" $HOME `id` $(id)";
        let m = build(json!({
            "job": {"runs_on": "a", "env": {"GREETING": tricky},
                    "setup": [{"run": "printf 'FROM_SETUP=%s\\n' \"$GREETING\" >> \"$PKGSMITH_ENV\"\necho /opt/tool/bin >> \"$PKGSMITH_PATH\"\n"}],
                    "steps": [{"run": "printf '%s|%s|%s' \"$GREETING\" \"$FROM_SETUP\" \"$PATH\"\necho\necho PKGSMITH_END\n"}]}
        }))
        .unwrap();
        // Setup and build share one container in real life, so the state directory stays between them. Run both here.
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "{}\n{}",
            m.setup.as_deref().unwrap(),
            m.build.as_deref().unwrap()
        )
        .replace(
            "/tmp/pkgsmith-state",
            &format!("{}/state", dir.path().display()),
        );
        let output = std::process::Command::new("sh")
            .args(["-ec", &script])
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
        let m = build(json!({
            "job": {"runs_on": "a",
                    "setup": [{"run": "echo 'not valid' >> \"$PKGSMITH_ENV\""}],
                    "steps": [{"run": "echo unreachable"}]}
        }))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            "{}\n{}",
            m.setup.as_deref().unwrap(),
            m.build.as_deref().unwrap()
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
