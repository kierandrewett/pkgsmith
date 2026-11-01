//! The APPX format of pkgsmith, as a plugin. The package writer is in `pkgsmith-windows-app`, which the `msix` plugin
//! shares.

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use pkgsmith_windows_app::APPX;
use std::path::PathBuf;

struct Appx;

impl Plugin for Appx {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "appx".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "appx".into(),
                description: "A Windows APPX package (.appx), written unsigned".into(),
                signing: Signing::None,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["appx"])?;
        pkgsmith_windows_app::file_name(APPX, package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["appx"])?;
        pkgsmith_windows_app::build(APPX, package, host)
    }
}

pkgsmith_plugin::export_plugin!(Appx);
