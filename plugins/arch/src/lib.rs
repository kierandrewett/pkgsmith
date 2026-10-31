//! The Arch Linux format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Arch;

impl Plugin for Arch {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "arch".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "archlinux".into(),
                description: "An Arch Linux package (.pkg.tar.zst)".into(),
                signing: Signing::Detached,
            }],
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["archlinux"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, _host: &Host) -> Result<PathBuf> {
        expect_format(package, &["archlinux"])?;
        format::build(package)
    }
}

pkgsmith_plugin::export_plugin!(Arch);
