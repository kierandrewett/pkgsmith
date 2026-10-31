//! The files of a package.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Data {
    /// A regular file. `src` is where the host keeps its bytes, an absolute path.
    File {
        src: PathBuf,
        size: u64,
    },
    Dir,
    /// A symbolic link. `target` is the link target as it reads on the installed system.
    Symlink {
        target: String,
    },
    /// Owned by the package, not carried in it.
    Ghost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Absolute path on the installed system, without a trailing slash.
    pub path: String,
    #[serde(flatten)]
    pub data: Data,
    pub mode: u32,
    pub owner: String,
    pub group: String,
    /// A configuration file: the package manager keeps it when the user changed it.
    #[serde(default)]
    pub config: bool,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.data == Data::Dir
    }

    pub fn size(&self) -> u64 {
        match self.data {
            Data::File { size, .. } => size,
            _ => 0,
        }
    }
}

/// The entries plus every parent folder the list does not hold, as entries owned by root with mode 0755. Sorted by
/// path. Most formats need every folder on the way to a file.
pub fn with_parents(entries: &[Entry]) -> Vec<Entry> {
    let mut all: BTreeMap<String, Entry> = entries
        .iter()
        .map(|e| (e.path.clone(), e.clone()))
        .collect();
    for entry in entries {
        let mut path = entry.path.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            all.entry(parent.to_string()).or_insert_with(|| Entry {
                path: parent.to_string(),
                data: Data::Dir,
                mode: 0o755,
                owner: "root".into(),
                group: "root".into(),
                config: false,
            });
            path = parent;
        }
    }
    all.into_values().collect()
}
