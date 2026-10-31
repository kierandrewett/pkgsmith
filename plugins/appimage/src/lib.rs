//! The AppImage format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct AppImage;

impl Plugin for AppImage {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "appimage".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "appimage".into(),
                description:
                    "An AppImage (type 2), one executable file that runs without installing".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["appimage"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["appimage"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(AppImage);
