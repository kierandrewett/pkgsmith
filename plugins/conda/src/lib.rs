//! The conda package format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Conda;

impl Plugin for Conda {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "conda".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "conda".into(),
                description: "A conda package (.conda, or .tar.bz2 with the archive option), for conda and micromamba".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["conda"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["conda"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Conda);
