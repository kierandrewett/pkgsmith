//! npm packages (`.tgz`): a gzip tar whose members are all below `package/`, with `package/package.json` at the root.
//!
//! `npm install NAME-VERSION.tgz` unpacks the archive into `node_modules/NAME` and runs its `preinstall` and
//! `postinstall` scripts. `npm pack` and `npm publish` write the same layout. npm carries only regular files: folders
//! are implied by the files in them, and symbolic links and empty folders are dropped. npm normalises the modes of
//! the files it installs to 0644 or 0755, and `npm pack` writes one fixed time stamp (26 October 1985, 08:15 UTC), so
//! this plugin writes the same. The manifest's paths under `prefix` become paths below the package root. npm chooses
//! the install folder itself, so `prefix` only says where the manifest keeps the files.
//! Reference: <https://docs.npmjs.com/cli/v10/configuring-npm/package-json> and
//! <https://docs.npmjs.com/cli/v10/commands/npm-pack>.

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::entry::{Data, Entry};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// `npm pack` writes this time stamp into every member: 1985-10-26 08:15:00 UTC.
const MTIME: u64 = 499_162_500;

/// The folder every member sits in.
const ROOT: &str = "package";

/// The folder inside the package that holds the install scripts.
const SCRIPT_DIR: &str = ".pkgsmith";

/// npm's limit on the length of a package name, scope included.
const NAME_LIMIT: usize = 214;

/// The `npm:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The folder the files live in on the installed system, absolute. Every file must be under it. Default:
    /// `/usr/lib/node_modules/NAME`, with the scope in the path for a scoped name.
    prefix: Option<String>,
    /// The npm scope without the `@`, for example `acme` for `@acme/NAME`.
    scope: Option<String>,
    /// The commands the package installs: command name to the absolute path of the file under the prefix that runs it.
    /// The file gets mode 0755.
    #[serde(default)]
    bin: BTreeMap<String, String>,
    /// The absolute path, under the prefix, of the file that `require` loads. Default: npm's `index.js`.
    main: Option<String>,
    /// The runtime dependencies: package name to an npm version range, for example `^4.18.0`.
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
}

/// The name npm knows the package by: `NAME` or `@SCOPE/NAME`.
fn npm_name(package: &Package, options: &Options) -> Result<String> {
    let name = match &options.scope {
        None => check_part(&package.name, "name")?.to_string(),
        Some(scope) => format!(
            "@{}/{}",
            check_part(scope, "scope")?,
            check_part(&package.name, "name")?
        ),
    };
    if name.len() > NAME_LIMIT {
        bail!("the npm name {name:?} is longer than {NAME_LIMIT} characters");
    }
    Ok(name)
}

/// npm names are lower case, URL safe, and do not start with `.` or `_`.
fn check_part<'a>(text: &'a str, what: &str) -> Result<&'a str> {
    let valid = !text.is_empty()
        && !text.starts_with(['.', '_'])
        && text.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '_' | '~')
        });
    if !valid {
        bail!(
            "npm {what} {text:?} must be lower case letters, digits, '-', '.', '_' or '~', and must not start with '.' or '_'"
        );
    }
    Ok(text)
}

/// The version as npm writes it: semver, `MAJOR.MINOR.PATCH` with an optional pre-release. The pre-release keeps its
/// dots and hyphens, so `1.2.3-nightly.20261031.142.gabc1234` stays as it is. Numbers may not have leading zeros.
fn version(package: &Package) -> Result<String> {
    let version = Version::parse(&package.version)?;
    let leading_zero =
        |id: &str| id.len() > 1 && id.starts_with('0') && id.bytes().all(|b| b.is_ascii_digit());
    if version.core.split('.').any(leading_zero) {
        bail!(
            "version {:?} has a number with a leading zero, which semver does not allow",
            package.version
        );
    }
    if let Some(pre) = &version.pre
        && pre.split('.').any(|id| id.is_empty() || leading_zero(id))
    {
        bail!(
            "version {:?} has an empty or zero-led pre-release identifier, which semver does not allow",
            package.version
        );
    }
    Ok(version.plain())
}

/// `NAME-VERSION.tgz`, with the scope dropped and `/` made `-`, as `npm pack` names it.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    let name = npm_name(package, &options)?;
    Ok(format!(
        "{}-{}.tgz",
        name.trim_start_matches('@').replace('/', "-"),
        version(package)?
    ))
}

/// The `cpu` field: npm refuses to install the package on another processor. `all` has none.
fn cpu(arch: &str) -> Result<Option<&'static str>> {
    Ok(match arch {
        "amd64" => Some("x64"),
        "arm64" => Some("arm64"),
        "all" => None,
        other => bail!("arch {other:?} must be amd64, arm64 or all"),
    })
}

/// The folder the files live in, absolute and without a trailing slash.
fn prefix(options: &Options, name: &str) -> Result<String> {
    let text = match &options.prefix {
        Some(text) => text.clone(),
        None => format!("/usr/lib/node_modules/{name}"),
    };
    let trimmed = text.trim_end_matches('/');
    if !text.starts_with('/')
        || trimmed.is_empty()
        || trimmed.split('/').any(|c| c == "." || c == "..")
    {
        bail!("prefix {text:?} must be an absolute folder such as /usr/lib/node_modules/NAME");
    }
    Ok(trimmed.to_string())
}

/// The path of `path` below `prefix`, without the slash, or `None` when it is not below it.
fn relative<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)?.strip_prefix('/')
}

/// Whether the folder `dir` lies on the way to `prefix`, as `/usr` does for `/usr/lib/node_modules/NAME`.
fn is_ancestor(dir: &str, prefix: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    prefix.len() > dir.len() && prefix.starts_with(dir) && prefix.as_bytes()[dir.len()] == b'/'
}

/// The entries below the prefix, sorted by what npm does with them.
#[derive(Default)]
struct Split<'a> {
    /// Regular files, by their path below the prefix.
    files: BTreeMap<String, &'a Entry>,
    /// Folders below the prefix, by their path below the prefix.
    dirs: Vec<String>,
    /// Symbolic links below the prefix, by their path below the prefix.
    links: Vec<String>,
    /// Ghost entries, which the package owns without carrying.
    ghosts: Vec<String>,
}

fn split<'a>(entries: &'a [Entry], prefix: &str) -> Result<Split<'a>> {
    let mut split = Split::default();
    for entry in entries {
        if matches!(entry.data, Data::Ghost) {
            split.ghosts.push(entry.path.clone());
            continue;
        }
        if entry.path == prefix {
            if entry.is_dir() {
                continue;
            }
            bail!(
                "{} is the package root, a folder, so it cannot be a file",
                entry.path
            );
        }
        let Some(rel) = relative(&entry.path, prefix) else {
            if entry.is_dir() && is_ancestor(&entry.path, prefix) {
                continue;
            }
            bail!(
                "{} is outside the prefix {prefix}: npm packages hold their files under the prefix, so move it there or change prefix",
                entry.path
            );
        };
        match &entry.data {
            Data::File { .. } => {
                if split.files.insert(rel.to_string(), entry).is_some() {
                    bail!("{} is in the manifest twice", entry.path);
                }
            }
            Data::Dir => split.dirs.push(rel.to_string()),
            Data::Symlink { .. } => split.links.push(rel.to_string()),
            Data::Ghost => {}
        }
    }
    Ok(split)
}

fn warn_list(host: &Host, what: &str, values: &[String]) {
    if !values.is_empty() {
        host.warn(&format!(
            "npm has no field for {what} ({}), so it is ignored",
            values.join(", ")
        ));
    }
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let name = file_name(package)?;
    let npm_name = npm_name(package, &options)?;
    let version = version(package)?;
    let prefix = prefix(&options, &npm_name)?;
    let cpu = cpu(&package.arch)?;
    let split = split(&package.entries, &prefix)?;

    if package.release != "1" {
        host.warn(&format!(
            "npm has no release number, so release {:?} is not written; the version is the only identity",
            package.release
        ));
    }
    if package.scripts.pre_remove.is_some() || package.scripts.post_remove.is_some() {
        host.warn(
            "npm uninstall runs no removal scripts, so pre_remove and post_remove are ignored",
        );
    }
    warn_list(host, "depends (distro package names)", &package.depends);
    warn_list(host, "recommends", &package.recommends);
    warn_list(host, "suggests", &package.suggests);
    warn_list(host, "conflicts", &package.conflicts);
    warn_list(host, "replaces", &package.replaces);
    warn_list(host, "provides", &package.provides);
    if !split.ghosts.is_empty() {
        host.warn(&format!(
            "ghost files are not carried: {}",
            split.ghosts.join(", ")
        ));
    }
    if !split.links.is_empty() {
        host.warn(&format!(
            "npm packages carry no symbolic links, so these are dropped: {}",
            split.links.join(", ")
        ));
    }
    let empty_dirs: Vec<&String> = split
        .dirs
        .iter()
        .filter(|dir| {
            !split
                .files
                .keys()
                .any(|f| f.starts_with(&format!("{dir}/")))
        })
        .collect();
    if !empty_dirs.is_empty() {
        host.warn(&format!(
            "npm packages carry no empty folders, so these are dropped: {}",
            empty_dirs
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if split.files.values().any(|e| e.config) {
        host.warn("npm keeps no configuration files: a changed config file is replaced on upgrade, so config is ignored");
    }
    if package.entries.iter().any(|e| {
        !matches!(e.owner.as_str(), "" | "root") || !matches!(e.group.as_str(), "" | "root")
    }) {
        host.warn(
            "npm carries no owners or groups, so files are owned by the user who installs them",
        );
    }

    // Commands and the main file must be files of the package.
    let mut bins = Map::new();
    let mut bin_files = BTreeSet::new();
    for (command, path) in &options.bin {
        if command.is_empty() || command.contains('/') || command.chars().any(char::is_whitespace) {
            bail!("bin command {command:?} must be a name without '/' or spaces");
        }
        let rel = relative(path, &prefix)
            .filter(|rel| split.files.contains_key(*rel))
            .with_context(|| {
                format!("bin {command:?} runs {path}, which is not a file under {prefix}")
            })?;
        bin_files.insert(rel.to_string());
        bins.insert(command.clone(), json!(format!("./{rel}")));
    }
    let main = match &options.main {
        None => None,
        Some(path) => {
            let rel = relative(path, &prefix)
                .filter(|rel| split.files.contains_key(*rel))
                .with_context(|| format!("main {path} is not a file under {prefix}"))?;
            Some(format!("./{rel}"))
        }
    };

    // The files and their modes. npm installs 0755 for a file that can run, and 0644 for the rest.
    let files: Vec<Entry> = split
        .files
        .iter()
        .map(|(rel, entry)| {
            let mode = if bin_files.contains(rel) || entry.mode & 0o111 != 0 {
                0o755
            } else {
                0o644
            };
            Entry {
                path: rel.clone(),
                data: entry.data.clone(),
                mode,
                owner: String::new(),
                group: String::new(),
                config: false,
            }
        })
        .collect();
    if split.files.contains_key("package.json") {
        bail!("a file at {prefix}/package.json would replace the generated package.json");
    }
    if split
        .files
        .keys()
        .any(|f| f.starts_with(&format!("{SCRIPT_DIR}/")))
    {
        bail!("{prefix}/{SCRIPT_DIR} holds the install scripts, so no file may be there");
    }

    // The readme: npm shows the README at the root. The long description becomes one unless the package ships one.
    let mut leading = Vec::new();
    let long = package
        .description
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    let long = long.trim();
    let ships_readme = split
        .files
        .keys()
        .any(|f| !f.contains('/') && f.to_ascii_lowercase().starts_with("readme"));
    if !long.is_empty() {
        if ships_readme {
            host.warn("the package ships its own README, so the long description is not written");
        } else {
            leading.push(Leading {
                name: format!("{ROOT}/README.md"),
                data: format!("{}\n\n{long}\n", package.summary()).into_bytes(),
                mode: 0o644,
            });
        }
    }

    // The install scripts, run by npm from their files in the package.
    let mut scripts = Map::new();
    let hooks = [
        ("preinstall", &package.scripts.pre_install),
        ("postinstall", &package.scripts.post_install),
    ];
    for (hook, body) in hooks {
        if let Some(body) = body {
            let file = format!("{SCRIPT_DIR}/{hook}");
            scripts.insert(hook.into(), json!(format!("./{file}")));
            leading.push(Leading {
                name: format!("{ROOT}/{file}"),
                data: body.clone().into_bytes(),
                mode: 0o755,
            });
        }
    }

    let mut manifest = Map::new();
    manifest.insert("name".into(), json!(npm_name));
    manifest.insert("version".into(), json!(version));
    if !package.summary().is_empty() {
        manifest.insert("description".into(), json!(package.summary()));
    }
    if let Some(license) = &package.license {
        manifest.insert("license".into(), json!(license));
    }
    if let Some(homepage) = &package.homepage {
        manifest.insert("homepage".into(), json!(homepage));
    }
    if let Some(maintainer) = &package.maintainer {
        manifest.insert("author".into(), json!(maintainer));
    }
    if let Some(main) = main {
        manifest.insert("main".into(), json!(main));
    }
    if !bins.is_empty() {
        manifest.insert("bin".into(), Value::Object(bins));
    }
    if !options.dependencies.is_empty() {
        manifest.insert("dependencies".into(), json!(options.dependencies));
    }
    if !scripts.is_empty() {
        manifest.insert("scripts".into(), Value::Object(scripts));
    }
    if let Some(cpu) = cpu {
        manifest.insert("cpu".into(), json!([cpu]));
    }
    let mut json_text = serde_json::to_vec_pretty(&Value::Object(manifest))?;
    json_text.push(b'\n');
    leading.insert(
        0,
        Leading {
            name: format!("{ROOT}/package.json"),
            data: json_text,
            mode: 0o644,
        },
    );

    let tar = archive::tar(&files, &format!("{ROOT}/"), MTIME, &leading)?;
    let file = package.output(&name);
    std::fs::write(&file, Compression::Gzip.compress(&tar)?)?;
    Ok(file)
}
