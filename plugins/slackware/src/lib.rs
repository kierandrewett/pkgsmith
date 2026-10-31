//! The Slackware format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Slackware;

impl Plugin for Slackware {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "slackware".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "slackware".into(),
                description: "A Slackware package (.txz), for installpkg".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["slackware"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["slackware"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Slackware);
