//! The zip format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Zip;

impl Plugin for Zip {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "zip".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "zip".into(),
                description: "A zip archive (.zip)".into(),
                signing: Signing::Detached,
            }],
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        pkgsmith_plugin::plugin::expect_format(package, &["zip"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, _host: &Host) -> Result<PathBuf> {
        pkgsmith_plugin::plugin::expect_format(package, &["zip"])?;
        format::build(package)
    }
}

pkgsmith_plugin::export_plugin!(Zip);
