//! The Apple disk image format of pkgsmith, as a plugin.

mod format;
mod hfsplus;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Dmg;

impl Plugin for Dmg {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "dmg".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "dmg".into(),
                description: "An Apple disk image (.dmg) holding one HFS+ volume with the files, for drag-and-drop installs".into(),
                signing: Signing::None,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["dmg"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["dmg"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Dmg);
