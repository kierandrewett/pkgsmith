//! The MSIX format of pkgsmith, as a plugin. The package writer is in `pkgsmith-windows-app`, which the `appx` plugin
//! shares.

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use pkgsmith_windows_app::MSIX;
use std::path::PathBuf;

struct Msix;

impl Plugin for Msix {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "msix".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "msix".into(),
                description: "A Windows MSIX package (.msix), written unsigned".into(),
                signing: Signing::None,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["msix"])?;
        pkgsmith_windows_app::file_name(MSIX, package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["msix"])?;
        pkgsmith_windows_app::build(MSIX, package, host)
    }
}

pkgsmith_plugin::export_plugin!(Msix);
