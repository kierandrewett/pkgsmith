//! The npm package format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Npm;

impl Plugin for Npm {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "npm".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "npm".into(),
                description: "An npm package (.tgz), for npm install and npm publish".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["npm"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["npm"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Npm);
