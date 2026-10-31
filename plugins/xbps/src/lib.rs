//! The Void Linux format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Xbps;

impl Plugin for Xbps {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "xbps".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "xbps".into(),
                description: "A Void Linux package (.xbps), for xbps-install".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["xbps"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["xbps"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Xbps);
