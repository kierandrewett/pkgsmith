//! The options of the three Nix formats, and what they decide from the files of the package.

use anyhow::{Result, bail};
use pkgsmith_plugin::{Data, Entry, Host, Package};
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};

/// The `options` of a `nix`, `nix-cache` or `nix-export` package.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Entries are taken relative to this absolute path. Default `/usr`. All formats.
    pub prefix: Option<String>,
    /// The compression of the archive (`nix`), of every NAR (`nix-cache`) or of the file (`nix-export`). `xz`, `gzip`,
    /// `zstd`, `bzip2` or `none`. Default `xz`, and `none` for `nix-export`.
    pub compression: Option<String>,

    /// `nix`: attributes of nixpkgs for `buildInputs`, for example `openssl` or `stdenv.cc.cc.lib`.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// `nix`: attributes of nixpkgs for `nativeBuildInputs`.
    #[serde(default)]
    pub native_inputs: Vec<String>,
    /// `nix`: attributes of nixpkgs for `runtimeDependencies`, the libraries that a program loads with `dlopen`.
    #[serde(default)]
    pub runtime_inputs: Vec<String>,
    /// `nix`: run `autoPatchelfHook`. Default: when a program or library of the package is dynamically linked.
    pub patchelf: Option<bool>,
    /// `nix`: library names that `autoPatchelfHook` may leave unresolved, or `*` for all.
    #[serde(default)]
    pub ignore_missing: Vec<String>,
    /// `nix`: let the fixup phase strip the programs. Default `false`, so the files stay as they were built.
    #[serde(default)]
    pub strip: bool,
    /// `nix`: globs, relative to the store path, of text files whose `prefix/` is rewritten to `$out/`. Default: the
    /// systemd units, desktop files and D-Bus services.
    pub rewrite: Option<Vec<String>>,
    /// `nix`: `meta.mainProgram`. Default: the package name, when `bin/NAME` exists.
    pub main_program: Option<String>,
    /// `nix`: Nix systems of `meta.platforms` and of the flake, such as `x86_64-linux`. Default: from `arch`.
    pub platforms: Option<Vec<String>>,
    /// `nix`: the license is not free software. Sets `free = false`.
    #[serde(default)]
    pub unfree: bool,
    /// `nix`: the flake input that provides nixpkgs. Default `github:NixOS/nixpkgs/nixos-unstable`.
    pub nixpkgs: Option<String>,

    /// `nix-cache`, `nix-export`: the store directory. Default `/nix/store`. It is part of every store path.
    pub store_dir: Option<String>,
    /// `nix-cache`, `nix-export`: store paths that the files may refer to. One is a reference when its digest occurs in
    /// the files.
    #[serde(default)]
    pub references: Vec<String>,
    /// `nix-cache`, `nix-export`: also take the store paths that the files spell out as references. Default `true`.
    pub scan_references: Option<bool>,
}

pub const DEFAULT_REWRITE: &[&str] = &[
    "lib/systemd/system/*",
    "lib/systemd/user/*",
    "share/applications/*.desktop",
    "share/dbus-1/**/*.service",
];

impl Options {
    /// The options of the package, and an error for a key that the format does not use.
    pub fn of(package: &Package) -> Result<Options> {
        let options: Options = package.options()?;
        let (recipe, store) = (package.format == "nix", package.format != "nix");
        let used = [
            ("inputs", !options.inputs.is_empty(), recipe),
            ("native_inputs", !options.native_inputs.is_empty(), recipe),
            ("runtime_inputs", !options.runtime_inputs.is_empty(), recipe),
            ("patchelf", options.patchelf.is_some(), recipe),
            ("ignore_missing", !options.ignore_missing.is_empty(), recipe),
            ("strip", options.strip, recipe),
            ("rewrite", options.rewrite.is_some(), recipe),
            ("main_program", options.main_program.is_some(), recipe),
            ("platforms", options.platforms.is_some(), recipe),
            ("unfree", options.unfree, recipe),
            ("nixpkgs", options.nixpkgs.is_some(), recipe),
            ("store_dir", options.store_dir.is_some(), store),
            ("references", !options.references.is_empty(), store),
            ("scan_references", options.scan_references.is_some(), store),
        ];
        for (key, set, allowed) in used {
            if set && !allowed {
                bail!(
                    "the option {key} is not used by the {} format; see the nix section of docs/formats.md",
                    package.format
                );
            }
        }
        Ok(options)
    }

    pub fn prefix(&self) -> Result<&str> {
        let prefix = self
            .prefix
            .as_deref()
            .unwrap_or("/usr")
            .trim_end_matches('/');
        if !prefix.is_empty() && !prefix.starts_with('/') {
            bail!("the option prefix {prefix:?} must be an absolute path");
        }
        Ok(prefix)
    }

    pub fn store_dir(&self) -> Result<&str> {
        let dir = self.store_dir.as_deref().unwrap_or("/nix/store");
        if !dir.starts_with('/') || dir.len() < 2 || dir.ends_with('/') || dir.contains("//") {
            bail!(
                "the option store_dir {dir:?} must be an absolute path without a trailing slash, such as /nix/store"
            );
        }
        Ok(dir)
    }
}

/// The entries with the prefix removed, so that the result is the content of the store path. An entry outside the
/// prefix keeps its whole path below the store path, because a Nix package cannot write outside it.
pub fn relocate(package: &Package, options: &Options, host: &Host) -> Result<Vec<Entry>> {
    let prefix = options.prefix()?;
    let mut outside = Vec::new();
    let mut entries = Vec::new();
    for entry in &package.entries {
        if matches!(entry.data, Data::Ghost) {
            continue;
        }
        let mut entry = entry.clone();
        match entry.path.strip_prefix(prefix) {
            Some("") if !prefix.is_empty() => continue,
            Some(rest) if rest.starts_with('/') => entry.path = rest.to_owned(),
            _ => outside.push(entry.path.clone()),
        }
        entries.push(entry);
    }
    if !outside.is_empty() {
        let top: std::collections::BTreeSet<&str> = outside
            .iter()
            .filter_map(|p| p.trim_start_matches('/').split('/').next())
            .collect();
        host.info(&format!(
            "{} entries are outside the prefix {prefix:?} ({}); they go below the store path with their whole path, because a Nix package cannot write outside it",
            outside.len(),
            top.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !entries.iter().any(|e| matches!(e.data, Data::File { .. })) {
        bail!("a Nix package needs at least one file below the prefix {prefix:?}");
    }
    Ok(entries)
}

/// Warnings for what a Nix package cannot say. Both are shared by the three formats.
pub fn warn_unexpressed(package: &Package, host: &Host) {
    let lists = [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, values) in lists {
        if !values.is_empty() {
            host.warn(&format!(
                "the {field} names of a distro mean nothing to Nix, so {} is not written; a Nix package names its libraries as nixpkgs attributes in the options inputs and runtime_inputs",
                values.join(", ")
            ));
        }
    }
    let scripts = &package.scripts;
    if [
        &scripts.pre_install,
        &scripts.post_install,
        &scripts.pre_remove,
        &scripts.post_remove,
    ]
    .iter()
    .any(|script| script.is_some())
    {
        host.warn("a Nix package runs no install or remove scripts, so the package scripts are not written");
    }
    if package.maintainer.is_some() {
        host.warn(
            "the maintainer is not written: meta.maintainers needs a nixpkgs maintainer entry",
        );
    }
}

/// Warnings about the entries, after the prefix is removed.
pub fn warn_entries(entries: &[Entry], host: &Host) {
    let absolute_links = entries
        .iter()
        .filter(|e| matches!(&e.data, Data::Symlink { target } if target.starts_with('/')))
        .count();
    if absolute_links > 0 {
        host.warn(&format!(
            "{absolute_links} symbolic links have an absolute target, which does not point into the store path"
        ));
    }
    if entries.iter().any(|e| e.config) {
        host.warn("a Nix package has no config flag: a configuration file is an ordinary read-only file in the store path");
    }
    if entries
        .iter()
        .any(|e| e.mode & 0o4000 != 0 || e.mode & 0o2000 != 0)
    {
        host.warn("the store is read-only and shared: a setuid or setgid bit is dropped, as Nix keeps only the executable bit of a mode");
    }
}

/// The Nix systems for `package`, or `None` for every system. `platforms` of the options wins.
pub fn systems(package: &Package, options: &Options) -> Result<Option<Vec<String>>> {
    if let Some(list) = &options.platforms {
        for system in list {
            let valid = system.split_once('-').is_some_and(|(cpu, os)| {
                !cpu.is_empty()
                    && !os.is_empty()
                    && system
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            });
            if !valid {
                bail!(
                    "the option platforms has {system:?}; write a Nix system such as x86_64-linux"
                );
            }
        }
        return Ok(Some(list.clone()));
    }
    let cpu = match package.arch.as_str() {
        "all" => return Ok(None),
        "amd64" | "x86_64" => "x86_64",
        "arm64" | "aarch64" => "aarch64",
        "i386" | "i686" | "x86" => "i686",
        "armhf" | "armv7l" | "armv7" => "armv7l",
        "riscv64" => "riscv64",
        "ppc64le" | "powerpc64le" => "powerpc64le",
        "s390x" => "s390x",
        other => bail!(
            "pkgsmith does not know the Nix CPU for the arch {other:?}; set the option platforms"
        ),
    };
    let os = match package.distro.as_deref() {
        Some("macos") => "darwin",
        Some("windows") => bail!("a Nix package is not written for Windows"),
        _ => "linux",
    };
    Ok(Some(vec![format!("{cpu}-{os}")]))
}

/// `None` when the file at `path` is not ELF. Otherwise whether it is dynamically linked: it has a program interpreter
/// or a dynamic section. Static programs have neither, and nothing in them needs patching.
pub fn elf_is_dynamic(path: &std::path::Path) -> Option<bool> {
    fn read(path: &std::path::Path) -> std::io::Result<Option<bool>> {
        let mut file = std::fs::File::open(path)?;
        let mut head = [0u8; 64];
        if file.read_exact(&mut head).is_err() || &head[..4] != b"\x7fELF" {
            return Ok(None);
        }
        let (wide, big) = (head[4] == 2, head[5] == 2);
        let u16at = |at: usize| {
            let b = [head[at], head[at + 1]];
            if big {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            }
        };
        let u32of = |b: [u8; 4]| {
            if big {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            }
        };
        let (phoff, size_at, num_at) = if wide {
            let mut b = [0u8; 8];
            b.copy_from_slice(&head[32..40]);
            (
                if big {
                    u64::from_be_bytes(b)
                } else {
                    u64::from_le_bytes(b)
                },
                54,
                56,
            )
        } else {
            let b = [head[28], head[29], head[30], head[31]];
            (u64::from(u32of(b)), 42, 44)
        };
        let (size, num) = (u64::from(u16at(size_at)), u64::from(u16at(num_at)));
        if size < 4 || num > 4096 {
            return Ok(Some(false));
        }
        for i in 0..num {
            file.seek(SeekFrom::Start(phoff + i * size))?;
            let mut kind = [0u8; 4];
            if file.read_exact(&mut kind).is_err() {
                return Ok(Some(false));
            }
            // PT_DYNAMIC is 2 and PT_INTERP is 3.
            if matches!(u32of(kind), 2 | 3) {
                return Ok(Some(true));
            }
        }
        Ok(Some(false))
    }
    read(path).unwrap_or(None)
}

/// Whether the entry is a regular file whose path matches one of the globs. `*` stands for a part of one name, `**` for
/// any number of folders and `?` for one character.
pub fn glob(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'*', rest)) if rest.first() == Some(&b'*') => {
                let rest = &rest[1..];
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            Some((b'*', rest)) => {
                (0..=s.len()).any(|i| go(rest, &s[i..]) && !s[..i].contains(&b'/'))
            }
            Some((b'?', rest)) => s.first().is_some_and(|c| *c != b'/') && go(rest, &s[1..]),
            Some((c, rest)) => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_one_name_or_many_folders() {
        assert!(glob(
            "lib/systemd/system/*",
            "lib/systemd/system/hi.service"
        ));
        assert!(!glob(
            "lib/systemd/system/*",
            "lib/systemd/system/x/hi.service"
        ));
        assert!(glob(
            "share/dbus-1/**/*.service",
            "share/dbus-1/services/a.service"
        ));
        assert!(glob(
            "share/dbus-1/**/*.service",
            "share/dbus-1/a/b/c.service"
        ));
        assert!(glob("share/dbus-1/**/*.service", "share/dbus-1/a.service"));
        assert!(!glob(
            "share/applications/*.desktop",
            "share/applications/a.txt"
        ));
        assert!(glob("a?c", "abc"));
    }

    #[test]
    fn a_shell_script_and_a_missing_file_are_not_elf() {
        let dir = std::env::temp_dir().join(format!("pkgsmith-elf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("hi");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        assert_eq!(elf_is_dynamic(&script), None);
        assert_eq!(elf_is_dynamic(&dir.join("missing")), None);
        // The test binary itself is a dynamic ELF on Linux.
        if cfg!(target_os = "linux") {
            assert_eq!(
                elf_is_dynamic(&std::env::current_exe().unwrap()),
                Some(true)
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn arch_maps_to_a_nix_system() {
        let mut package = Package::default();
        let options = Options::default();
        package.arch = "amd64".into();
        assert_eq!(
            systems(&package, &options).unwrap(),
            Some(vec!["x86_64-linux".to_owned()])
        );
        package.arch = "arm64".into();
        package.distro = Some("macos".into());
        assert_eq!(
            systems(&package, &options).unwrap(),
            Some(vec!["aarch64-darwin".to_owned()])
        );
        package.arch = "all".into();
        assert_eq!(systems(&package, &options).unwrap(), None);
        package.arch = "m68k".into();
        assert!(systems(&package, &options).is_err());
    }
}
