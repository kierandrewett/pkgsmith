//! A package as one format sees it: the manifest, the files and the version of that format.

use crate::manifest::{Effective, Manifest};
use crate::tree::{self, Entry};
use anyhow::{Context, Result};
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

pub struct Package<'a> {
    pub manifest: &'a Manifest,
    pub format: &'a str,
    pub effective: Effective,
    /// The files, without the parent folders the format may need to add.
    pub entries: Vec<Entry>,
    pub options: &'a Options,
}

impl<'a> Package<'a> {
    pub fn new(
        manifest: &'a Manifest,
        format: &'a str,
        options: &'a Options,
    ) -> Result<Package<'a>> {
        Ok(Package {
            manifest,
            format,
            effective: manifest.effective(format, options.distro.as_deref()),
            entries: tree::resolve(manifest, format)
                .with_context(|| format!("cannot collect the files for {format}"))?,
            options,
        })
    }

    /// Bytes the files take once installed.
    pub fn installed_size(&self) -> u64 {
        self.entries.iter().map(Entry::size).sum()
    }

    pub fn summary(&self) -> &str {
        self.manifest
            .description
            .lines()
            .next()
            .unwrap_or("")
            .trim()
    }

    /// The text of a script. A value with a line break is the script itself. A one-line value is a file next to the
    /// manifest. A script that has no `#!` line gets `#!/bin/sh` and `set -e`.
    pub fn script(&self, value: &Option<String>) -> Result<Option<String>> {
        let Some(value) = value else { return Ok(None) };
        let text = if value.contains('\n') {
            value.clone()
        } else {
            let path = self.manifest.base.join(value.trim());
            std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read the script {}", path.display()))?
        };
        Ok(Some(if text.starts_with("#!") {
            text
        } else {
            format!("#!/bin/sh\nset -e\n{text}")
        }))
    }

    pub fn output(&self, file_name: &str) -> PathBuf {
        self.options.out.join(file_name)
    }
}
