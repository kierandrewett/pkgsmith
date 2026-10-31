//! The snap format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Snap;

impl Plugin for Snap {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "snap".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "snap".into(),
                description: "A snap package (.snap), for snapd".into(),
                signing: Signing::None,
            }],
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["snap"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["snap"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Snap);
