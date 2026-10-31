//! The tar format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Tar;

impl Plugin for Tar {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "tar".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "tar".into(),
                description: "A tar archive (.tar.xz, .tar.gz, .tar.zst, .tar.bz2)".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        pkgsmith_plugin::plugin::expect_format(package, &["tar"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, _host: &Host) -> Result<PathBuf> {
        pkgsmith_plugin::plugin::expect_format(package, &["tar"])?;
        format::build(package)
    }
}

pkgsmith_plugin::export_plugin!(Tar);
