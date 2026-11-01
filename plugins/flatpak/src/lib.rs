//! The Flatpak format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Flatpak;

impl Plugin for Flatpak {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "flatpak".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "flatpak".into(),
                description:
                    "A Flatpak single-file bundle (.flatpak), built with the flatpak program".into(),
                signing: Signing::None,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["flatpak"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["flatpak"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Flatpak);
