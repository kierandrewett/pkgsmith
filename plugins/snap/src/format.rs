//! Snap packages (`.snap`): a SquashFS image of the files plus `meta/snap.yaml`.
//!
//! A snap is mounted read-only at `/snap/NAME/REVISION/`, so the files of the package live below that path and an app
//! names its command relative to the root of the image. The files of the package keep their install paths without the
//! leading `/`, so `/usr/bin/hello` is `usr/bin/hello` in the image and the app command is `usr/bin/hello`.
//! Reference: <https://snapcraft.io/docs/the-snap-format>.

use anyhow::{Context, Result, bail};
use backhand::compression::Compressor;
use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};
use pkgsmith_plugin::entry::{self, Data};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

/// One app of the snap, in the `snap.apps` map of the manifest.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct App {
    /// The program to run: a path in the snap, with or without a leading `/`, and its arguments.
    command: String,
    /// `simple`, `forking`, `oneshot` or `notify` makes the app a service that snapd starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    daemon: Option<String>,
    /// Interfaces the app connects to, for example `network` or `home`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    plugs: Vec<String>,
    /// Environment variables of the app.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    environment: BTreeMap<String, String>,
}

/// The `snap:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The base snap: `core22` (default), `core24`, `core20`, or `bare` for a snap that brings everything.
    base: Option<String>,
    /// `strict` (default), `classic` or `devmode`.
    confinement: Option<String>,
    /// `stable` (default) or `devel`. `devmode` confinement needs `devel`.
    grade: Option<String>,
    /// A title with capitals and spaces for the store. Default: none.
    title: Option<String>,
    /// The summary, at most 78 characters. Default: the first line of the description.
    summary: Option<String>,
    #[serde(default)]
    apps: BTreeMap<String, App>,
}

#[derive(Serialize)]
struct SnapYaml<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    version: &'a str,
    summary: &'a str,
    description: &'a str,
    architectures: Vec<&'a str>,
    base: &'a str,
    confinement: &'a str,
    grade: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<&'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    apps: BTreeMap<&'a str, App>,
}

fn architecture(arch: &str) -> &str {
    match arch {
        "amd64" | "arm64" => arch,
        _ => "all",
    }
}

/// The rules of snapd for a snap name: lowercase letters, digits and single hyphens, at least one letter, not at the
/// ends.
fn check_name(name: &str) -> Result<()> {
    let valid = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && name.chars().any(|c| c.is_ascii_lowercase())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.len() <= 40;
    if !valid {
        bail!(
            "{name:?} is not a snap name: use lowercase letters, digits and single hyphens, with at least one letter, at most 40 characters"
        );
    }
    Ok(())
}

/// snapd limits a version to 32 characters of letters, digits and `. + ~ - _`.
fn snap_version(package: &Package) -> Result<String> {
    let version = Version::parse(&package.version)?.plain();
    let valid = version.len() <= 32
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".+~-_".contains(c));
    if !valid {
        bail!(
            "the version {version:?} is longer than 32 characters or has characters that snapd refuses; shorten the pre-release text"
        );
    }
    Ok(version)
}

/// `name_version_arch.snap`, the name `snapcraft pack` gives.
pub fn file_name(package: &Package) -> Result<String> {
    check_name(&package.name)?;
    Ok(format!(
        "{}_{}_{}.snap",
        package.name,
        snap_version(package)?,
        architecture(&package.arch)
    ))
}

fn header(mode: u32, mtime: u64) -> NodeHeader {
    NodeHeader::new(
        (mode & 0o7777) as u16,
        0,
        0,
        u32::try_from(mtime).unwrap_or(u32::MAX),
    )
}

fn relative(path: &str) -> String {
    format!("/{}", path.trim_start_matches('/'))
}

/// A snap app command is relative to the root of the image: `usr/bin/hello`, never `/usr/bin/hello`.
fn command(text: &str) -> String {
    text.trim().trim_start_matches('/').to_string()
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    check_name(&package.name)?;
    let version = snap_version(package)?;

    let confinement = options.confinement.as_deref().unwrap_or("strict");
    if !["strict", "classic", "devmode"].contains(&confinement) {
        bail!("confinement {confinement:?} must be strict, classic or devmode");
    }
    let grade = options.grade.as_deref().unwrap_or("stable");
    if !["stable", "devel"].contains(&grade) {
        bail!("grade {grade:?} must be stable or devel");
    }
    if confinement == "devmode" && grade == "stable" {
        bail!("devmode confinement needs grade: devel");
    }
    let summary = options.summary.as_deref().unwrap_or(package.summary());
    if summary.chars().count() > 78 {
        bail!(
            "the snap summary has {} characters; snaps allow 78. Set snap.summary",
            summary.chars().count()
        );
    }
    if summary.is_empty() {
        bail!("a snap needs a summary: write a description or set snap.summary");
    }
    for (name, app) in &options.apps {
        if let Some(daemon) = &app.daemon
            && !["simple", "forking", "oneshot", "notify"].contains(&daemon.as_str())
        {
            bail!("app {name:?}: daemon {daemon:?} must be simple, forking, oneshot or notify");
        }
    }

    if !package.depends.is_empty() {
        host.warn("a snap bundles what it needs, so depends is ignored");
    }
    if package.scripts.pre_install.is_some() {
        host.warn("snaps have no pre-install hook, so pre_install is ignored");
    }
    if package.scripts.pre_remove.is_some() && package.scripts.post_remove.is_some() {
        host.warn("snaps have one remove hook, so post_remove is used and pre_remove is ignored");
    }

    let apps: BTreeMap<&str, App> = options
        .apps
        .iter()
        .map(|(name, app)| {
            let mut app = app.clone();
            app.command = command(&app.command);
            (name.as_str(), app)
        })
        .collect();
    let yaml = serde_yaml_ng::to_string(&SnapYaml {
        name: &package.name,
        title: options.title.as_deref(),
        version: &version,
        summary,
        description: package.description.trim(),
        architectures: vec![architecture(&package.arch)],
        base: options.base.as_deref().unwrap_or("core22"),
        confinement,
        grade,
        license: package.license.as_deref(),
        apps,
    })?;

    let mut image = FilesystemWriter::default();
    image.set_time(u32::try_from(package.mtime).unwrap_or(u32::MAX));
    image.set_root_mode(0o755);
    // snapd reads xz, which is what `snap pack` writes.
    image.set_compressor(FilesystemCompressor::new(Compressor::Xz, None)?);
    let dir = header(0o755, package.mtime);
    for item in entry::with_parents(&package.entries) {
        let path = relative(&item.path);
        let head = header(item.mode, package.mtime);
        match &item.data {
            Data::Dir => image.push_dir_all(&path, dir)?,
            Data::Symlink { target } => {
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    image.push_dir_all(parent, dir)?;
                }
                image.push_symlink(target, &path, head)?
            }
            Data::File { src, .. } => {
                let file = std::fs::File::open(src)
                    .with_context(|| format!("cannot read {}", src.display()))?;
                image.push_file(file, &path, head)?;
            }
            Data::Ghost => {}
        }
    }
    image.push_dir_all("/meta", dir)?;
    image.push_file(
        Cursor::new(yaml.into_bytes()),
        "/meta/snap.yaml",
        header(0o644, package.mtime),
    )?;
    let hooks = [
        ("install", package.scripts.post_install.as_ref()),
        (
            "remove",
            package
                .scripts
                .post_remove
                .as_ref()
                .or(package.scripts.pre_remove.as_ref()),
        ),
    ];
    for (name, script) in hooks {
        if let Some(text) = script {
            image.push_dir_all("/meta/hooks", dir)?;
            image.push_file(
                Cursor::new(text.clone().into_bytes()),
                format!("/meta/hooks/{name}"),
                header(0o755, package.mtime),
            )?;
        }
    }

    let mut bytes = Cursor::new(Vec::new());
    image.write(&mut bytes)?;
    let file = package.output(&file_name(package)?);
    std::fs::write(&file, bytes.into_inner())?;
    Ok(file)
}
