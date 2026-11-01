//! The package description the host sends to a plugin: everything a format needs, with nothing left to look up.

use crate::entry::Entry;
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The install scripts, as the text of a script: it starts with `#!`. `None` when the manifest has none.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scripts {
    pub pre_install: Option<String>,
    pub post_install: Option<String>,
    pub pre_remove: Option<String>,
    pub post_remove: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Package {
    /// The format to write, one of the names the plugin lists.
    pub format: String,
    pub name: String,
    /// As written in the manifest, `MAJOR.MINOR.PATCH` or `MAJOR.MINOR.PATCH-prerelease`. See [`crate::version`].
    pub version: String,
    pub release: String,
    /// The epoch of the version. `0` means none. `rpm`, `deb` and `archlinux` write it, and the others ignore it.
    #[serde(default)]
    pub epoch: u32,
    /// `amd64`, `arm64` or `all`.
    pub arch: String,
    /// The first line is the summary, the rest the long text.
    pub description: String,
    pub maintainer: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    /// Names of the other packages, after the overrides for this format and distro.
    pub depends: Vec<String>,
    pub recommends: Vec<String>,
    pub suggests: Vec<String>,
    pub conflicts: Vec<String>,
    pub replaces: Vec<String>,
    pub provides: Vec<String>,
    pub scripts: Scripts,
    /// The files. Empty when the host only asks for the file name.
    pub entries: Vec<Entry>,
    /// The section of the manifest named like the format, for example `deb:`. `null` when the manifest has none.
    pub options: serde_json::Value,
    /// The distro of the job, for example `fedora`.
    pub distro: Option<String>,
    /// Seconds since the epoch for every time stamp in the package, so equal input gives equal bytes.
    pub mtime: u64,
    /// The folder that receives the package. The host creates it before `build`.
    pub out: PathBuf,
}

impl Package {
    /// The format options as the type the plugin defines. A missing section gives `T::default()`. Give `T`
    /// `#[serde(deny_unknown_fields)]` so a typo in the manifest is an error.
    pub fn options<T: DeserializeOwned + Default>(&self) -> Result<T> {
        if self.options.is_null() {
            return Ok(T::default());
        }
        serde_json::from_value(self.options.clone())
            .with_context(|| format!("the {} section of the manifest", self.format))
    }

    /// The first line of the description.
    pub fn summary(&self) -> &str {
        self.description.lines().next().unwrap_or("").trim()
    }

    /// Bytes the files take once installed.
    pub fn installed_size(&self) -> u64 {
        self.entries.iter().map(Entry::size).sum()
    }

    /// The path of a file in the output folder.
    pub fn output(&self, file_name: &str) -> PathBuf {
        self.out.join(file_name)
    }
}
