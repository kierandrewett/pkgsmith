//! The files of a package: the `contents` of a manifest resolved into one sorted list of entries.

use crate::manifest::{Content, Kind, Manifest};
use anyhow::{Context, Result, bail};
use pkgsmith_plugin::{Data, Entry};
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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

/// The mode of a file on disk. Unix keeps it. Windows has none, so a program (`.exe`, `.bat`, `.cmd`, `.sh`, `.ps1`) gets
/// 0755 and every other file 0644. A `mode` in the manifest overrides both.
#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata, _path: &Path) -> u32 {
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn file_mode(_metadata: &fs::Metadata, path: &Path) -> u32 {
    let program = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["exe", "bat", "cmd", "sh", "ps1"].contains(&e.to_ascii_lowercase().as_str())
    });
    if program { 0o755 } else { 0o644 }
}

#[cfg(unix)]
fn dir_mode(metadata: &fs::Metadata) -> u32 {
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn dir_mode(_metadata: &fs::Metadata) -> u32 {
    0o755
}

fn file_entry(path: String, src: &Path, content: &Content, config: bool) -> Result<Entry> {
    let metadata = fs::metadata(src).with_context(|| format!("cannot read {}", src.display()))?;
    let (owner, group) = identity(content);
    Ok(Entry {
        path,
        mode: content.mode.unwrap_or(file_mode(&metadata, src)),
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
        mode: dir_mode(&metadata),
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

/// Is `path` inside one of the excluded locations? An exclude is a path prefix or a glob.
pub fn excluded(path: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|rule| {
        // `/etc/app/**` names everything below the folder and the folder itself.
        let rule = rule.trim_end_matches('/');
        let rule = rule.strip_suffix("/**").unwrap_or(rule);
        path == rule
            || path
                .strip_prefix(rule)
                .is_some_and(|rest| rest.starts_with('/'))
            || glob::Pattern::new(rule).is_ok_and(|pattern| pattern.matches(path))
    })
}

/// Folders that the system owns. A package that copies a prefix does not own them: an rpm that did would conflict with the
/// `filesystem` package, because its folders have other modes than the ones in the prefix.
const SYSTEM_DIRS: &[&str] = &[
    "/",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/usr",
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/lib64",
    "/usr/libexec",
    "/usr/share",
    "/usr/include",
    "/usr/src",
    "/usr/local",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/local/lib",
    "/usr/local/share",
    "/usr/local/include",
    "/etc",
    "/opt",
    "/var",
    "/var/lib",
    "/var/log",
    "/var/cache",
    "/var/spool",
    "/var/tmp",
    "/run",
    "/tmp",
    "/srv",
    "/home",
    "/root",
    "/boot",
    "/mnt",
    "/media",
];

/// The path a `src` names. A relative path is inside the prefix, and an absolute one must be inside the prefix or in the
/// folder of the script. Packaging never reads anything else, so a script cannot pull in files of the system by accident.
fn locate(manifest: &Manifest, text: &str) -> Result<PathBuf> {
    let path = Path::new(text);
    let native = path.is_absolute()
        && (path.starts_with(&manifest.prefix) || path.starts_with(&manifest.base));
    let candidate = if native {
        path.to_path_buf()
    } else if path.is_absolute() || text.starts_with('/') {
        // `/usr/bin/x` names a file of the prefix, as it does in a package. On Windows a path with no drive is not
        // absolute, so the text decides.
        manifest.prefix.join(text.trim_start_matches(['/', '\\']))
    } else {
        manifest.prefix.join(path)
    };
    // The path itself may be a symbolic link that the package keeps as a link, so only its folder is resolved, unless the
    // path is the root itself. A glob is resolved by its folder part.
    let resolved = |p: &Path| fs::canonicalize(p).ok();
    let folder = candidate.parent().and_then(resolved);
    let inside = |root: &Path| {
        fs::canonicalize(root).is_ok_and(|root| {
            folder.as_ref().is_some_and(|f| f.starts_with(&root))
                || resolved(&candidate).is_some_and(|c| c == root)
        })
    };
    if inside(&manifest.prefix) || inside(&manifest.base) {
        Ok(candidate)
    } else {
        bail!(
            "{text}: outside the prefix {} and the script folder {}",
            manifest.prefix.display(),
            manifest.base.display()
        )
    }
}

/// A file that a package reads must resolve, symbolic links followed, to a place inside the prefix or the script folder. A
/// link in the prefix that points outside it is an error, so a prefix cannot pull in files of the system.
fn confine(manifest: &Manifest, path: &Path) -> Result<()> {
    let resolved =
        fs::canonicalize(path).with_context(|| format!("cannot read {}", path.display()))?;
    let inside = |root: &Path| fs::canonicalize(root).is_ok_and(|root| resolved.starts_with(root));
    if inside(&manifest.prefix) || inside(&manifest.base) {
        Ok(())
    } else {
        bail!(
            "{} points to {}, outside the prefix and the script folder",
            path.display(),
            resolved.display()
        )
    }
}

/// Resolve the `contents` of a target into one sorted list of entries. Later entries replace earlier ones with the same
/// path. Then `include`, `exclude` and `config_files` apply to the paths of the package.
pub fn resolve(manifest: &Manifest) -> Result<Vec<Entry>> {
    let mut found: BTreeMap<String, Entry> = BTreeMap::new();
    // Folders that a `directory(...)` entry asks for are the package's, even the ones of the system.
    let mut asked: Vec<String> = Vec::new();
    for content in &manifest.contents {
        let mut entries = Vec::new();
        let src = content.src.as_deref().unwrap_or("");
        let (owner, group) = identity(content);
        match content.kind {
            Kind::Dir => {
                asked.push(normalise(&content.dst));
                entries.push(Entry {
                    path: normalise(&content.dst),
                    data: Data::Dir,
                    mode: content.mode.unwrap_or(0o755),
                    owner,
                    group,
                    config: false,
                });
            }
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
                &locate(manifest, src)?,
                &normalise(&content.dst),
                content,
                &mut entries,
            )?,
            Kind::File | Kind::Config => {
                let config = content.kind == Kind::Config;
                if src.is_empty() {
                    bail!("{}: a file needs a src", content.dst);
                }
                if has_glob(src) {
                    let pattern = locate(manifest, src)?;
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
                        confine(manifest, &path)?;
                        let name = path.file_name().unwrap().to_string_lossy().into_owned();
                        entries.push(file_entry(
                            join(&content.dst, &name),
                            &path,
                            content,
                            config,
                        )?);
                    }
                } else {
                    let path = locate(manifest, src)?;
                    confine(manifest, &path)?;
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
    let matches = |path: &str, rules: &[String]| excluded(path, rules);
    // Folders that hold something now, before the filters take files away.
    let had_children: Vec<String> = found
        .values()
        .filter(|e| e.is_dir())
        .filter(|dir| {
            found
                .keys()
                .any(|p| p.starts_with(&format!("{}/", dir.path)))
        })
        .map(|dir| dir.path.clone())
        .collect();
    let mut entries: Vec<Entry> = found
        .into_values()
        .filter(|entry| {
            !(entry.is_dir()
                && SYSTEM_DIRS.contains(&entry.path.as_str())
                && !asked.contains(&entry.path))
        })
        .filter(|entry| !matches(&entry.path, &manifest.exclude))
        .collect();
    if !manifest.include.is_empty() {
        let kept: Vec<String> = entries
            .iter()
            .filter(|entry| !entry.is_dir() && matches(&entry.path, &manifest.include))
            .map(|entry| entry.path.clone())
            .collect();
        // A folder stays when something below it stays.
        entries.retain(|entry| {
            kept.contains(&entry.path)
                || (entry.is_dir()
                    && kept
                        .iter()
                        .any(|path| path.starts_with(&format!("{}/", entry.path))))
        });
    }
    // A folder that held files and lost them all to exclude or include goes too, unless the script asked for it.
    loop {
        let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
        let before = entries.len();
        entries.retain(|e| {
            !(e.is_dir()
                && had_children.contains(&e.path)
                && !asked.contains(&e.path)
                && !paths.iter().any(|p| p.starts_with(&format!("{}/", e.path))))
        });
        if entries.len() == before {
            break;
        }
    }
    if let Some(own) = &manifest.own_dirs {
        entries.retain(|e| !e.is_dir() || asked.contains(&e.path) || matches(&e.path, own));
    }
    for entry in &mut entries {
        if !entry.is_dir() && matches(&entry.path, &manifest.config_files) {
            entry.config = true;
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(dir: &Path, contents: serde_json::Value) -> Manifest {
        let spec = Manifest::parse_spec(json!({
            "name": "demo", "version": "1.0.0", "description": "d", "format": "deb", "contents": contents,
        }))
        .unwrap();
        Manifest::from_spec(spec, dir.to_path_buf(), dir.to_path_buf()).unwrap()
    }

    #[cfg(unix)]
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
            json!([{"src": "stage/opt/app", "dst": "/opt/app", "type": "tree"}]),
        );
        let entries = resolve(&manifest).unwrap();
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
        let manifest = manifest(
            dir.path(),
            json!([{"src": "man/*.1", "dst": "/usr/share/man/man1"}]),
        );
        let paths: Vec<_> = resolve(&manifest)
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
    fn a_later_entry_replaces_an_earlier_one_with_the_same_path() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one"), "1").unwrap();
        fs::write(dir.path().join("two"), "22").unwrap();
        let manifest = manifest(
            dir.path(),
            json!([{"src": "one", "dst": "/f"}, {"src": "two", "dst": "/f"}]),
        );
        let entries = resolve(&manifest).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size(), 2);
    }

    #[test]
    fn include_exclude_and_config_files_apply_to_the_paths_of_the_package() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("usr/bin")).unwrap();
        fs::create_dir_all(dir.path().join("usr/share/doc")).unwrap();
        fs::create_dir_all(dir.path().join("etc")).unwrap();
        for file in ["usr/bin/a", "usr/share/doc/a.md", "etc/a.conf"] {
            fs::write(dir.path().join(file), "x").unwrap();
        }
        let spec = |extra: serde_json::Value| {
            let mut base = json!({"name": "demo", "version": "1", "description": "d", "format": "deb",
                "contents": [{"src": "/", "dst": "/", "type": "tree"}]});
            base.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            Manifest::from_spec(
                Manifest::parse_spec(base).unwrap(),
                dir.path().to_path_buf(),
                dir.path().to_path_buf(),
            )
            .unwrap()
        };
        let paths = |m: &Manifest| {
            resolve(m)
                .unwrap()
                .into_iter()
                .map(|e| e.path)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            paths(&spec(json!({}))),
            [
                "/etc/a.conf",
                "/usr/bin/a",
                "/usr/share/doc",
                "/usr/share/doc/a.md"
            ],
            "the files, and no folder of the system"
        );
        assert!(
            !paths(&spec(json!({"exclude": ["/usr/share/doc"]})))
                .iter()
                .any(|p| p.contains("doc"))
        );
        assert_eq!(
            paths(&spec(json!({"include": ["/usr/bin/**"]}))),
            ["/usr/bin/a"]
        );
        let configs: Vec<_> = resolve(&spec(json!({"config_files": ["/etc/**"]})))
            .unwrap()
            .into_iter()
            .filter(|e| e.config)
            .map(|e| e.path)
            .collect();
        assert_eq!(configs, ["/etc/a.conf"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_prefix_that_points_outside_is_refused_for_a_file_but_kept_in_a_tree() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "x").unwrap();
        fs::create_dir_all(dir.path().join("prefix/etc")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret"),
            dir.path().join("prefix/etc/passwd"),
        )
        .unwrap();
        let make = |contents: serde_json::Value| {
            let spec = Manifest::parse_spec(
                json!({"name": "demo", "version": "1", "description": "d", "format": "deb",
                "contents": contents}),
            )
            .unwrap();
            Manifest::from_spec(spec, dir.path().to_path_buf(), dir.path().join("prefix")).unwrap()
        };
        let error = resolve(&make(json!([{"src": "etc/passwd", "dst": "/etc/passwd"}])))
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside the prefix"), "{error}");
        let error = resolve(&make(json!([{"src": "etc/*", "dst": "/etc/"}])))
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside the prefix"), "{error}");
        // A tree keeps the link as a link and does not read what it points to.
        let entries = resolve(&make(json!([{"src": "/", "dst": "/", "type": "tree"}]))).unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.path == "/etc/passwd" && matches!(e.data, Data::Symlink { .. }))
        );
    }

    #[test]
    fn excluding_everything_in_a_folder_drops_the_folder_and_own_dirs_picks_the_owned_ones() {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "usr/bin/core",
            "usr/bin/app",
            "usr/share/applications/app.desktop",
            "etc/geoclue/conf.d/50.conf",
        ] {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let make = |extra: serde_json::Value| {
            let mut base = json!({"name": "demo", "version": "1", "description": "d", "format": "rpm",
                "contents": [{"src": "/", "dst": "/", "type": "tree"}]});
            base.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let m = Manifest::from_spec(
                Manifest::parse_spec(base).unwrap(),
                dir.path().to_path_buf(),
                dir.path().to_path_buf(),
            )
            .unwrap();
            resolve(&m)
                .unwrap()
                .into_iter()
                .map(|e| e.path)
                .collect::<Vec<_>>()
        };
        let paths = make(
            json!({"exclude": ["/usr/bin/app", "/usr/share/applications/app.desktop", "/etc/geoclue/**"]}),
        );
        assert_eq!(paths, ["/usr/bin/core"]);
        let own = make(json!({"own_dirs": ["/etc/geoclue", "/etc/geoclue/**"]}));
        assert!(own.contains(&"/etc/geoclue/conf.d".to_string()));
        assert!(
            !own.contains(&"/usr/share/applications".to_string()),
            "{own:?}"
        );
        assert!(own.contains(&"/usr/share/applications/app.desktop".to_string()));
        assert!(
            !make(json!({"own_dirs": []}))
                .iter()
                .any(|p| p == "/etc/geoclue")
        );
    }

    #[test]
    fn a_src_outside_the_prefix_and_the_script_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let spec = Manifest::parse_spec(
            json!({"name": "demo", "version": "1", "description": "d", "format": "deb",
            "contents": [{"src": "../x", "dst": "/x"}]}),
        )
        .unwrap();
        let manifest =
            Manifest::from_spec(spec, dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap();
        let error = resolve(&manifest).unwrap_err().to_string();
        assert!(error.contains("outside the prefix"), "{error}");
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
        let paths: Vec<_> = pkgsmith_plugin::entry::with_parents(&[entry])
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(
            paths,
            ["/usr", "/usr/lib", "/usr/lib/gnoblin", "/usr/lib/gnoblin/x"]
        );
    }
}
