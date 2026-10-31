//! The Python wheel format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Wheel;

impl Plugin for Wheel {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "wheel".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "wheel".into(),
                description: "A Python wheel (.whl), installed by pip into site-packages".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        pkgsmith_plugin::plugin::expect_format(package, &["wheel"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        pkgsmith_plugin::plugin::expect_format(package, &["wheel"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Wheel);
