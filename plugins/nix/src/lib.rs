//! The Nix format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Nix;

impl Plugin for Nix {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "nix".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "nix".into(),
                description: "A Nix package: a default.nix with the files, for nix-build".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["nix"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["nix"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Nix);
