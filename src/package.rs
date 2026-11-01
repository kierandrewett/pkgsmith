//! A package as a plugin sees it: the target of a script turned into a [`pkgsmith_plugin::Package`].

use crate::manifest::{Job, Manifest};
use crate::tree;
use anyhow::{Context, Result};
use pkgsmith_plugin::{Entry, Package, Scripts};
use std::path::PathBuf;

/// What the caller decides about a build.
#[derive(Debug, Clone)]
pub struct Options {
    pub out: PathBuf,
    /// `fedora`, `opensuse`, `debian`, `ubuntu`, `archlinux`: the distro a plugin may adapt to.
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
/// script. A script that has no `#!` line gets `#!/bin/sh` and `set -e`.
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
    Package {
        format: format.to_string(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        release: manifest.release.clone(),
        epoch: manifest.epoch,
        arch: manifest.arch.clone(),
        description: manifest.description.clone(),
        maintainer: manifest.maintainer.clone(),
        homepage: manifest.homepage.clone(),
        license: manifest.license.clone(),
        depends: manifest.depends.clone(),
        recommends: manifest.recommends.clone(),
        suggests: manifest.suggests.clone(),
        conflicts: manifest.conflicts.clone(),
        replaces: manifest.replaces.clone(),
        provides: manifest.provides.clone(),
        scripts: Scripts::default(),
        entries: Vec::new(),
        options: manifest.options.clone(),
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
    let scripts = &manifest.scripts;
    package.scripts = Scripts {
        pre_install: script(manifest, &scripts.pre_install)?,
        post_install: script(manifest, &scripts.post_install)?,
        pre_remove: script(manifest, &scripts.pre_remove)?,
        post_remove: script(manifest, &scripts.post_remove)?,
    };
    package.entries = tree::resolve_with(manifest, tracked)
        .with_context(|| format!("cannot collect the files for {format}"))?;
    Ok(package)
}

/// The name of the output file. A target may set `file`, a template that replaces the name the plugin picked.
/// `{file}` is the name the plugin picked. `{name}`, `{version}`, `{release}`, `{arch}`, `{format}` and `{distro}` are
/// the fields of the package. The result is a file name: no folder, not empty, not hidden.
pub fn output_name(job: &Job, package: &Package, default: &str) -> Result<String> {
    let Some(template) = job.values.get("file") else {
        return Ok(default.to_string());
    };
    let mut name = String::new();
    let mut rest = template.as_str();
    while let Some(open) = rest.find('{') {
        name.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            anyhow::bail!("file {template:?} has a `{{` without a `}}`");
        };
        name.push_str(match &after[..close] {
            "file" => default,
            "name" => &package.name,
            "version" => &package.version,
            "release" => &package.release,
            "arch" => &package.arch,
            "format" => &package.format,
            "distro" => package.distro.as_deref().unwrap_or(""),
            other => anyhow::bail!(
                "file {template:?}: unknown placeholder {{{other}}}; use file, name, version, release, arch, format or distro"
            ),
        });
        rest = &after[close + 1..];
    }
    name.push_str(rest);
    if name.is_empty() || name.starts_with('.') || name.contains('/') || name.contains('\\') {
        anyhow::bail!("file {template:?} gives {name:?}, which is not a plain file name");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(file: &str) -> Job {
        Job {
            values: [("file".to_string(), file.to_string())].into(),
        }
    }

    fn package() -> Package {
        Package {
            name: "hi".into(),
            version: "1.0.0".into(),
            release: "1".into(),
            arch: "amd64".into(),
            format: "tar".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_template_replaces_the_name_and_can_keep_the_default() {
        let p = package();
        assert_eq!(
            output_name(&Job::default(), &p, "hi.tar.xz").unwrap(),
            "hi.tar.xz"
        );
        assert_eq!(
            output_name(&job("{name}_{version}.tar.bz2"), &p, "x").unwrap(),
            "hi_1.0.0.tar.bz2"
        );
        assert_eq!(
            output_name(&job("release-{file}"), &p, "hi.tar.xz").unwrap(),
            "release-hi.tar.xz"
        );
    }

    #[test]
    fn a_template_that_is_not_a_file_name_is_refused() {
        let p = package();
        for bad in ["../x", "a/b", ".hidden", "{nope}", "{name", ""] {
            assert!(output_name(&job(bad), &p, "x").is_err(), "{bad}");
        }
    }
}
