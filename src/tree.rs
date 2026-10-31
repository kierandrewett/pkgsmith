//! The files of a package: the `contents` of a manifest resolved into one sorted list of entries.

use crate::manifest::{Content, Kind, Manifest};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    File {
        src: PathBuf,
        size: u64,
    },
    Dir,
    Symlink {
        target: String,
    },
    /// Owned by the package, not carried in it.
    Ghost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Absolute path on the installed system, without a trailing slash.
    pub path: String,
    pub data: Data,
    pub mode: u32,
    pub owner: String,
    pub group: String,
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

fn has_glob(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

/// `/usr//lib/` -> `/usr/lib`; `/` stays `/`.
pub fn normalise(path: &str) -> String {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    format!("/{}", parts.join("/"))
}

fn join(dir: &str, name: &str) -> String {
    normalise(&format!("{dir}/{name}"))
}

fn identity(content: &Content) -> (String, String) {
    (
        content.owner.clone().unwrap_or_else(|| "root".into()),
        content.group.clone().unwrap_or_else(|| "root".into()),
    )
}

fn file_entry(path: String, src: &Path, content: &Content, config: bool) -> Result<Entry> {
    let metadata = fs::metadata(src).with_context(|| format!("cannot read {}", src.display()))?;
    let (owner, group) = identity(content);
    Ok(Entry {
        path,
        mode: content
            .mode
            .unwrap_or(metadata.permissions().mode() & 0o7777),
        data: Data::File {
            src: src.to_path_buf(),
            size: metadata.len(),
        },
        owner,
        group,
        config,
    })
}

fn walk(src: &Path, dst: &str, content: &Content, out: &mut Vec<Entry>) -> Result<()> {
    let metadata = fs::metadata(src).with_context(|| format!("cannot read {}", src.display()))?;
    let (owner, group) = identity(content);
    out.push(Entry {
        path: dst.to_string(),
        data: Data::Dir,
        mode: metadata.permissions().mode() & 0o7777,
        owner,
        group,
        config: false,
    });
    let mut children: Vec<_> = fs::read_dir(src)
        .with_context(|| format!("cannot list {}", src.display()))?
        .collect::<Result<_, _>>()?;
    children.sort_by_key(|child| child.file_name());
    for child in children {
        let name = child.file_name().to_string_lossy().into_owned();
        let path = child.path();
        let target = join(dst, &name);
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_symlink() {
            let (owner, group) = identity(content);
            out.push(Entry {
                path: target,
                data: Data::Symlink {
                    target: fs::read_link(&path)?.to_string_lossy().into_owned(),
                },
                mode: 0o777,
                owner,
                group,
                config: false,
            });
        } else if kind.is_dir() {
            walk(&path, &target, content, out)?;
        } else {
            out.push(file_entry(
                target,
                &path,
                &Content {
                    mode: None,
                    ..content.clone()
                },
                false,
            )?);
        }
    }
    Ok(())
}

/// Resolve the manifest's `contents` for one format. Later entries replace earlier ones with the same path.
pub fn resolve(manifest: &Manifest, format: &str) -> Result<Vec<Entry>> {
    resolve_with(manifest, format, Vec::new())
}

/// Like `resolve`, starting from the files a tracked build installed. The manifest's `contents` then add to them or
/// replace them, for example to mark a file as configuration or to change its mode.
pub fn resolve_with(manifest: &Manifest, format: &str, tracked: Vec<Entry>) -> Result<Vec<Entry>> {
    let mut found: BTreeMap<String, Entry> = tracked
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    for content in &manifest.contents {
        if content
            .only
            .as_ref()
            .is_some_and(|only| !only.iter().any(|f| f == format))
        {
            continue;
        }
        let mut entries = Vec::new();
        let src = content.src.as_deref().unwrap_or("");
        let source = |text: &str| -> PathBuf {
            let path = Path::new(text);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                manifest.base.join(path)
            }
        };
        let (owner, group) = identity(content);
        match content.kind {
            Kind::Dir => entries.push(Entry {
                path: normalise(&content.dst),
                data: Data::Dir,
                mode: content.mode.unwrap_or(0o755),
                owner,
                group,
                config: false,
            }),
            Kind::Ghost => entries.push(Entry {
                path: normalise(&content.dst),
                data: Data::Ghost,
                mode: content.mode.unwrap_or(0o644),
                owner,
                group,
                config: false,
            }),
            Kind::Symlink => entries.push(Entry {
                path: normalise(&content.dst),
                data: Data::Symlink {
                    target: src.to_string(),
                },
                mode: 0o777,
                owner,
                group,
                config: false,
            }),
            Kind::Tree => walk(
                &source(src),
                &normalise(&content.dst),
                content,
                &mut entries,
            )?,
            Kind::File | Kind::Config => {
                let config = content.kind == Kind::Config;
                if has_glob(src) {
                    let pattern = source(src);
                    let mut matches: Vec<PathBuf> = glob::glob(&pattern.to_string_lossy())
                        .with_context(|| format!("bad glob {src:?}"))?
                        .filter_map(Result::ok)
                        .filter(|path| path.is_file())
                        .collect();
                    matches.sort();
                    if matches.is_empty() {
                        bail!("{src:?} matches no files");
                    }
                    for path in matches {
                        let name = path.file_name().unwrap().to_string_lossy().into_owned();
                        entries.push(file_entry(
                            join(&content.dst, &name),
                            &path,
                            content,
                            config,
                        )?);
                    }
                } else {
                    let path = source(src);
                    let destination = if content.dst.ends_with('/') {
                        join(&content.dst, &path.file_name().unwrap().to_string_lossy())
                    } else {
                        normalise(&content.dst)
                    };
                    entries.push(file_entry(destination, &path, content, config)?);
                }
            }
        }
        for entry in entries {
            found.insert(entry.path.clone(), entry);
        }
    }
    Ok(found.into_values().collect())
}

/// Every parent folder of an entry that the list does not already hold, as an entry owned by root with mode 0755.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::Scope;
    use crate::manifest::Job;

    fn manifest(dir: &Path, contents: &str) -> Manifest {
        let path = dir.join("pkg.yaml");
        fs::write(
            &path,
            format!("name: demo\nversion: 1.0.0\ndescription: d\ncontents:\n{contents}"),
        )
        .unwrap();
        Manifest::parse(&path)
            .unwrap()
            .bind(&Scope::default(), &Job::default(), "deb")
            .unwrap()
    }

    #[test]
    fn a_tree_keeps_folders_files_modes_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("stage/opt/app");
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::write(root.join("lib/a.so"), "x").unwrap();
        fs::write(root.join("run"), "#!/bin/sh").unwrap();
        fs::set_permissions(root.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("lib/a.so", root.join("link")).unwrap();
        let manifest = manifest(
            dir.path(),
            "  - {src: stage/opt/app, dst: /opt/app, type: tree}\n",
        );
        let entries = resolve(&manifest, "deb").unwrap();
        let by_path: BTreeMap<_, _> = entries.iter().map(|e| (e.path.as_str(), e)).collect();
        assert!(by_path["/opt/app"].is_dir() && by_path["/opt/app/lib"].is_dir());
        assert_eq!(by_path["/opt/app/run"].mode, 0o755);
        assert_eq!(
            by_path["/opt/app/link"].data,
            Data::Symlink {
                target: "lib/a.so".into()
            }
        );
        assert_eq!(by_path["/opt/app/lib/a.so"].size(), 1);
    }

    #[test]
    fn a_glob_puts_each_match_in_the_destination_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("man")).unwrap();
        fs::write(dir.path().join("man/a.1"), "x").unwrap();
        fs::write(dir.path().join("man/b.1"), "x").unwrap();
        let manifest = manifest(dir.path(), "  - {src: man/*.1, dst: /usr/share/man/man1}\n");
        let paths: Vec<_> = resolve(&manifest, "rpm")
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(
            paths,
            ["/usr/share/man/man1/a.1", "/usr/share/man/man1/b.1"]
        );
    }

    #[test]
    fn an_entry_limited_to_other_formats_is_skipped_and_a_later_entry_wins() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one"), "1").unwrap();
        fs::write(dir.path().join("two"), "22").unwrap();
        let manifest = manifest(
            dir.path(),
            "  - {src: one, dst: /f}\n  - {src: two, dst: /f}\n  - {src: one, dst: /only-deb, only: [deb]}\n",
        );
        let rpm = resolve(&manifest, "rpm").unwrap();
        assert_eq!(rpm.len(), 1);
        assert_eq!(rpm[0].size(), 2);
        assert_eq!(resolve(&manifest, "deb").unwrap().len(), 2);
    }

    #[test]
    fn parents_are_added_once_and_owned_by_root() {
        let entry = Entry {
            path: "/usr/lib/gnoblin/x".into(),
            data: Data::Symlink { target: "y".into() },
            mode: 0o777,
            owner: "root".into(),
            group: "root".into(),
            config: false,
        };
        let paths: Vec<_> = with_parents(&[entry]).into_iter().map(|e| e.path).collect();
        assert_eq!(
            paths,
            ["/usr", "/usr/lib", "/usr/lib/gnoblin", "/usr/lib/gnoblin/x"]
        );
    }
}
