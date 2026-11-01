//!
//! A Flatpak app is a folder with a `metadata` keyfile and a `files/` folder, which the sandbox mounts as `/app`. The
//! package's files go under `files/`, with `prefix` (default `/usr`) taken off the front: `/usr/bin/x` becomes
//! `files/bin/x`. The bundle is an OSTree commit with its objects, and only the `flatpak` program writes that format, so
//! this plugin runs `flatpak build-finish`, `flatpak build-export` and `flatpak build-bundle` in a temporary folder. The
//! runtime is not in the bundle: it is fetched from `runtime_repo` when the bundle is installed.
//! Reference: `flatpak-build-bundle(1)`, `flatpak-build-export(1)`, `flatpak-build-finish(1)`,
//! <https://docs.flatpak.org/en/latest/conventions.html> and <https://docs.flatpak.org/en/latest/manifests.html>.

use anyhow::{Context, Result, anyhow, bail};
use pkgsmith_plugin::entry::{Data, Entry};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use serde::Deserialize;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

const DEFAULT_RUNTIME: &str = "org.freedesktop.Platform";
const DEFAULT_RUNTIME_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";
const INSTALL_HINT: &str = "flatpak is not installed, or it is not on PATH; install it with `dnf install flatpak` or `apt-get install flatpak`";

/// The `flatpak:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The application ID in reverse DNS, for example `org.example.Hello`. Required.
    app_id: Option<String>,
    /// The runtime the app runs on. Default `org.freedesktop.Platform`.
    runtime: Option<String>,
    /// The version of the runtime, for example `24.08`. Required: pkgsmith does not guess it.
    runtime_version: Option<String>,
    /// The SDK of the build. Default: the runtime with `.Platform` changed to `.Sdk`, or the runtime itself.
    sdk: Option<String>,
    /// The program in `prefix/bin` that `flatpak run` starts. Default: the package name.
    command: Option<String>,
    /// The branch of the bundle. Default `stable`.
    branch: Option<String>,
    /// Arguments for `flatpak build-finish`, such as `--share=network` or `--socket=wayland`. Each starts with `--`.
    /// `--command` is set from `command`, so it is not allowed here.
    #[serde(default)]
    finish_args: Vec<String>,
    /// The `.flatpakrepo` file of the repository that holds the runtime. Default Flathub. `""` leaves it out.
    runtime_repo: Option<String>,
    /// The folder the package installs under, which the sandbox calls `/app`. Default `/usr`.
    prefix: Option<String>,
}

/// The options with the defaults filled in.
struct Settings {
    app_id: String,
    /// The bundle architecture, `x86_64` or `aarch64`.
    arch: &'static str,
    runtime: String,
    runtime_version: String,
    sdk: String,
    command: String,
    branch: String,
    finish_args: Vec<String>,
    runtime_repo: Option<String>,
    /// The prefix without a trailing slash, for example `/usr`.
    base: String,
}

impl Settings {
    fn new(package: &Package) -> Result<Settings> {
        let options: Options = package.options()?;
        let arch = architecture(&package.arch)?;

        let app_id = options.app_id.ok_or_else(|| {
            anyhow!(
                "the flatpak section needs app_id, the application ID such as org.example.Hello"
            )
        })?;
        check_id(&app_id, "app_id")?;

        let runtime = options.runtime.unwrap_or_else(|| DEFAULT_RUNTIME.into());
        check_id(&runtime, "runtime")?;
        let sdk = match options.sdk {
            Some(sdk) => sdk,
            None => match runtime.strip_suffix(".Platform") {
                Some(base) => format!("{base}.Sdk"),
                None => runtime.clone(),
            },
        };
        check_id(&sdk, "sdk")?;

        let runtime_version = options.runtime_version.ok_or_else(|| {
            anyhow!("the flatpak section needs runtime_version, for example \"24.08\", the version of {runtime}")
        })?;
        check_token(&runtime_version, "runtime_version")?;

        let command = options.command.unwrap_or_else(|| package.name.clone());
        if command.is_empty() || command.contains('/') || command.chars().any(char::is_whitespace) {
            bail!("command {command:?} must be a program name in prefix/bin, without a slash");
        }

        let branch = options.branch.unwrap_or_else(|| "stable".into());
        check_token(&branch, "branch")?;

        for arg in &options.finish_args {
            let is_permission = arg
                .strip_prefix("--")
                .is_some_and(|rest| !rest.is_empty() && !rest.starts_with("command"));
            if !is_permission {
                bail!(
                    "finish_args entry {arg:?} must be a build-finish option such as --share=network; --command is set from command"
                );
            }
        }

        let runtime_repo = match options.runtime_repo.as_deref() {
            None => Some(DEFAULT_RUNTIME_REPO.to_string()),
            Some("") => None,
            Some(url) => Some(url.to_string()),
        };

        let prefix = options.prefix.unwrap_or_else(|| "/usr".into());
        let plain = Path::new(&prefix)
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
        if !prefix.starts_with('/') || !plain || prefix.trim_end_matches('/').is_empty() {
            bail!("prefix {prefix:?} must be an absolute folder such as /usr, without . or ..");
        }

        Ok(Settings {
            app_id,
            arch,
            runtime,
            runtime_version,
            sdk,
            command,
            branch,
            finish_args: options.finish_args,
            runtime_repo,
            base: prefix.trim_end_matches('/').to_string(),
        })
    }
}

/// `amd64` is `x86_64` and `arm64` is `aarch64`. A bundle belongs to one architecture, so `all` is an error.
fn architecture(arch: &str) -> Result<&'static str> {
    match arch {
        "amd64" => Ok("x86_64"),
        "arm64" => Ok("aarch64"),
        "all" => bail!(
            "a Flatpak bundle is built for one architecture, so arch \"all\" cannot be written; use amd64 or arm64"
        ),
        other => bail!("arch {other:?} must be amd64, arm64 or all"),
    }
}

/// A reverse-DNS application or runtime ID: at least three elements, each of letters, digits or `_`, not starting with a
/// digit. Only the last element may contain `-`, as the Flatpak conventions require.
fn check_id(id: &str, field: &str) -> Result<()> {
    let elements: Vec<&str> = id.split('.').collect();
    let last = elements.len().saturating_sub(1);
    let valid = |index: usize, element: &str| {
        element.chars().next().is_some_and(|c| !c.is_ascii_digit())
            && element
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || (c == '-' && index == last))
    };
    if elements.len() < 3
        || id.len() > 255
        || !elements.iter().enumerate().all(|(i, e)| valid(i, e))
    {
        bail!("{field} {id:?} must be a reverse-DNS ID such as org.example.Hello");
    }
    Ok(())
}

/// A runtime version or a branch: letters, digits, `.`, `_` or `-`.
fn check_token(text: &str, field: &str) -> Result<()> {
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        bail!("{field} {text:?} must hold only letters, digits, '.', '_' or '-'");
    }
    Ok(())
}

/// `app-id-version-release.arch.flatpak`.
pub fn file_name(package: &Package) -> Result<String> {
    let settings = Settings::new(package)?;
    Ok(format!(
        "{}-{}-{}.{}.flatpak",
        settings.app_id,
        Version::parse(&package.version)?.plain(),
        package.release,
        settings.arch
    ))
}

/// The `metadata` keyfile of the app. `flatpak build-finish` adds the command and the permissions.
fn metadata(settings: &Settings) -> String {
    format!(
        "[Application]\nname={}\nruntime={}/{}/{}\nsdk={}/{}/{}\n",
        settings.app_id,
        settings.runtime,
        settings.arch,
        settings.runtime_version,
        settings.sdk,
        settings.arch,
        settings.runtime_version
    )
}

/// The path under `files/` of an installed path, `None` for the prefix itself.
fn relative(path: &str, base: &str) -> Result<Option<PathBuf>> {
    if path == base {
        return Ok(None);
    }
    let Some(rest) = path
        .strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        bail!(
            "{path} is outside the prefix {base}: a Flatpak app installs under {base}, which the sandbox calls /app. Move the file under {base}, or set prefix"
        );
    };
    let rel = Path::new(rest);
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        bail!("{path} is not a plain path");
    }
    Ok(Some(rel.to_path_buf()))
}

/// An absolute target under the prefix becomes the same path under `/app`. Other targets stay as they are.
fn link_target(target: &str, base: &str) -> String {
    match target.strip_prefix(base) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("/app{rest}"),
        _ => target.to_string(),
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o7777))
        .with_context(|| format!("cannot set the mode of {}", path.display()))
}

/// Copies the entries into `files/`. Folders get their mode last, so a read-only folder still takes its files.
fn install(entries: &[Entry], base: &str, files: &Path) -> Result<()> {
    let mut folders: Vec<(PathBuf, u32)> = Vec::new();
    for entry in entries {
        let Some(rel) = relative(&entry.path, base)? else {
            if !matches!(entry.data, Data::Dir | Data::Ghost) {
                bail!(
                    "{} is the prefix itself, and only folders can sit there",
                    entry.path
                );
            }
            continue;
        };
        let dest = files.join(&rel);
        match &entry.data {
            Data::Dir => {
                fs::create_dir_all(&dest).with_context(|| format!("cannot make {}", entry.path))?;
                folders.push((dest, entry.mode));
            }
            Data::File { src, .. } => {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(src, &dest)
                    .with_context(|| format!("cannot copy {} to {}", src.display(), entry.path))?;
                set_mode(&dest, entry.mode)?;
            }
            Data::Symlink { target } => {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                symlink(link_target(target, base), &dest)
                    .with_context(|| format!("cannot make the link {}", entry.path))?;
            }
            Data::Ghost => {}
        }
    }
    folders.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, mode) in folders {
        set_mode(&path, mode)?;
    }
    Ok(())
}

/// Warns about the manifest fields a Flatpak app cannot express, and about the files that cannot work in the sandbox.
fn warn(package: &Package, settings: &Settings, host: &Host) {
    let lists = [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, names) in lists {
        if !names.is_empty() {
            host.warn(&format!(
                "{field} is ignored: a Flatpak app takes its libraries from the runtime {}, so list them in the runtime option instead",
                settings.runtime
            ));
        }
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
                "the {name} script is ignored: Flatpak runs no install or removal scripts"
            ));
        }
    }
    let fields = [
        ("maintainer", &package.maintainer),
        ("homepage", &package.homepage),
        ("license", &package.license),
    ];
    for (name, value) in fields {
        if value.is_some() {
            host.warn(&format!(
                "{name} is not written: a Flatpak bundle has no field for it"
            ));
        }
    }
    let config = package.entries.iter().filter(|e| e.config).count();
    if config > 0 {
        host.warn(&format!(
            "{config} configuration file(s) are installed as ordinary files: Flatpak keeps no configuration-file state, so an update replaces them"
        ));
    }
    if package
        .entries
        .iter()
        .any(|e| e.owner != "root" || e.group != "root")
    {
        host.warn("the owner and group of the files are ignored: Flatpak installs every file as the user who installs the app");
    }
    let command = format!("{}/bin/{}", settings.base, settings.command);
    if !package.entries.is_empty() && !package.entries.iter().any(|e| e.path == command) {
        host.warn(&format!(
            "the command {command} is not in the package, so `flatpak run` has nothing to start"
        ));
    }
    for entry in &package.entries {
        if let Data::Symlink { target } = &entry.data
            && target.starts_with('/')
            && link_target(target, &settings.base) == *target
        {
            host.warn(&format!(
                "{} points at {target}, outside the prefix, so it does not resolve inside the sandbox",
                entry.path
            ));
        }
    }
}

/// Runs a `flatpak` command and returns an error with its message when it fails.
fn run(command: &mut Command) -> Result<()> {
    let shown: Vec<String> = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => anyhow!(INSTALL_HINT),
            _ => anyhow!("cannot run {}: {e}", shown.join(" ")),
        })?;
    if !output.status.success() {
        bail!(
            "{} failed: {}",
            shown.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// The ISO 8601 UTC time of `seconds` since the epoch, for `flatpak build-export --timestamp`.
fn iso8601(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since the epoch to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let settings = Settings::new(package)?;
    let name = file_name(package)?;
    warn(package, &settings, host);

    let work = tempfile::tempdir().context("cannot make a temporary folder for the app")?;
    let app = work.path().join("app");
    let repo = work.path().join("repo");
    let files = app.join("files");
    fs::create_dir_all(&files)?;
    install(&package.entries, &settings.base, &files)?;
    fs::write(app.join("metadata"), metadata(&settings))?;

    run(Command::new("flatpak")
        .arg("build-finish")
        .arg(format!("--command={}", settings.command))
        .args(&settings.finish_args)
        .arg(&app))?;
    run(Command::new("flatpak")
        .arg("build-export")
        .arg(format!("--arch={}", settings.arch))
        .arg(format!("--timestamp={}", iso8601(package.mtime)))
        .arg(&repo)
        .arg(&app)
        .arg(&settings.branch))?;

    let file = package.output(&name);
    if file.exists() {
        fs::remove_file(&file)
            .with_context(|| format!("cannot replace the old {}", file.display()))?;
    }
    let mut bundle = Command::new("flatpak");
    bundle
        .arg("build-bundle")
        .arg(format!("--arch={}", settings.arch));
    if let Some(url) = &settings.runtime_repo {
        bundle.arg(format!("--runtime-repo={url}"));
    }
    run(bundle
        .arg(&repo)
        .arg(&file)
        .arg(&settings.app_id)
        .arg(&settings.branch))?;
    Ok(file)
}
