//! A package as a plugin sees it: the manifest of one job and one format turned into a [`pkgsmith_plugin::Package`].

use crate::manifest::Manifest;
use crate::tree;
use anyhow::{Context, Result};
use pkgsmith_plugin::{Entry, Package, Scripts};
use std::path::PathBuf;

/// What the caller decides about a build.
#[derive(Debug, Clone)]
pub struct Options {
    pub out: PathBuf,
    /// `fedora`, `opensuse`, `debian`, `ubuntu`, `archlinux`: picks the matching `overrides`.
    pub distro: Option<String>,
    /// Seconds since the epoch for every file time in the package. `SOURCE_DATE_EPOCH` when set, else the time now.
    pub mtime: u64,
}

impl Options {
    pub fn new(out: PathBuf, distro: Option<String>) -> Options {
        let mtime = std::env::var("SOURCE_DATE_EPOCH")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            });
        Options { out, distro, mtime }
    }
}

/// The text of a script. A value with a line break is the script itself. A one-line value is a file next to the
/// manifest. A script that has no `#!` line gets `#!/bin/sh` and `set -e`.
fn script(manifest: &Manifest, value: &Option<String>) -> Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let text = if value.contains('\n') {
        value.clone()
    } else {
        let path = manifest.base.join(value.trim());
        std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read the script {}", path.display()))?
    };
    Ok(Some(if text.starts_with("#!") {
        text
    } else {
        format!("#!/bin/sh\nset -e\n{text}")
    }))
}

/// The package without its files and scripts. Enough to ask a plugin for the name of the file it would write.
pub fn outline(manifest: &Manifest, format: &str, options: &Options) -> Package {
    let effective = manifest.effective(format, options.distro.as_deref());
    Package {
        format: format.to_string(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        release: manifest.release.clone(),
        arch: manifest.arch.clone(),
        description: manifest.description.clone(),
        maintainer: manifest.maintainer.clone(),
        homepage: manifest.homepage.clone(),
        license: manifest.license.clone(),
        depends: effective.depends,
        recommends: effective.recommends,
        suggests: effective.suggests,
        conflicts: effective.conflicts,
        replaces: effective.replaces,
        provides: effective.provides,
        scripts: Scripts::default(),
        entries: Vec::new(),
        options: manifest
            .options
            .get(format)
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        distro: options.distro.clone(),
        mtime: options.mtime,
        out: options.out.clone(),
    }
}

/// The whole package. Its files start as what a tracked build installed (see `container`), then the `contents` of the
/// manifest apply.
pub fn assemble(
    manifest: &Manifest,
    format: &str,
    options: &Options,
    tracked: Vec<Entry>,
) -> Result<Package> {
    let mut package = outline(manifest, format, options);
    let scripts = manifest
        .effective(format, options.distro.as_deref())
        .scripts;
    package.scripts = Scripts {
        pre_install: script(manifest, &scripts.pre_install)?,
        post_install: script(manifest, &scripts.post_install)?,
        pre_remove: script(manifest, &scripts.pre_remove)?,
        post_remove: script(manifest, &scripts.post_remove)?,
    };
    package.entries = tree::resolve_with(manifest, format, tracked)
        .with_context(|| format!("cannot collect the files for {format}"))?;
    Ok(package)
}
