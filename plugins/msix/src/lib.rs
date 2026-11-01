//! The MSIX format of pkgsmith, as a plugin.

mod format;
mod opc;
mod zip;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
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
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["msix"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Msix);
