//! The Nix formats of pkgsmith, as a plugin: `nix`, `nix-cache` and `nix-export`.

mod nar;
mod nixlang;
mod options;
mod recipe;
mod store;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Nix;

const FORMATS: &[&str] = &["nix", "nix-cache", "nix-export"];

impl Plugin for Nix {
    fn info(&self) -> PluginInfo {
        let format = |name: &str, description: &str| FormatInfo {
            name: name.into(),
            description: description.into(),
            signing: Signing::Detached,
        };
        PluginInfo {
            name: "nix".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![
                format(
                    "nix",
                    "A Nix flake and package.nix with the files, for nix build and callPackage",
                ),
                format(
                    "nix-cache",
                    "A Nix binary cache with the files as one store path, for nix copy --from",
                ),
                format(
                    "nix-export",
                    "The files as one store path in the stream of nix-store --export, for nix-store --import",
                ),
            ],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        match expect_format(package, FORMATS)? {
            "nix" => recipe::file_name(package),
            _ => store::file_name(package),
        }
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        match expect_format(package, FORMATS)? {
            "nix" => recipe::build(package, host),
            _ => store::build(package, host),
        }
    }
}

pkgsmith_plugin::export_plugin!(Nix);

#[cfg(test)]
mod testing {
    use pkgsmith_plugin::{Data, Entry, Package};
    use std::path::Path;

    /// A package of `format` whose files are written below `dir`. Each file is a path on the installed system, its text
    /// and its mode.
    pub fn package(
        dir: &Path,
        format: &str,
        files: &[(&str, &str, u32)],
        options: serde_json::Value,
    ) -> Package {
        let mut entries = Vec::new();
        for (index, (path, text, mode)) in files.iter().enumerate() {
            let src = dir.join(format!("src{index}"));
            std::fs::write(&src, text).unwrap();
            entries.push(Entry {
                path: (*path).into(),
                data: Data::File {
                    src,
                    size: text.len() as u64,
                },
                mode: *mode,
                owner: "root".into(),
                group: "root".into(),
                config: false,
            });
        }
        Package {
            format: format.into(),
            name: "hi".into(),
            version: "1.0.0".into(),
            release: "1".into(),
            arch: "amd64".into(),
            description: "Says hi\n\nMore text.".into(),
            homepage: Some("https://example.org/hi".into()),
            license: Some("MIT".into()),
            entries,
            options,
            mtime: 1_700_000_000,
            out: dir.join("out"),
            ..Default::default()
        }
    }

    /// The members of a tar, by name.
    pub fn members(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
        use std::io::Read;
        let mut archive = tar::Archive::new(bytes);
        let mut out = std::collections::BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            out.insert(name, data);
        }
        out
    }
}
