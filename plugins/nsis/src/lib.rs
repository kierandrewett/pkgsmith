//! The NSIS format of pkgsmith, as a plugin: a Windows installer (`.exe`) built by `makensis`.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Nsis;

impl Plugin for Nsis {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "nsis".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "nsis".into(),
                description: "A Windows installer (.exe) built by makensis; it adds the program to Add/Remove Programs".into(),
                signing: Signing::None,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["nsis"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["nsis"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Nsis);
