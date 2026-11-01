//! The MSI format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Msi;

impl Plugin for Msi {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "msi".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "msi".into(),
                description: "A Windows Installer package (.msi), written unsigned".into(),
                signing: Signing::None,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["msi"])?;
        Ok(format::file_name(package))
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["msi"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Msi);
