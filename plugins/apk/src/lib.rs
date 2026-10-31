//! The Alpine apk format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Scheme, Signing};
use std::path::PathBuf;

struct Apk;

impl Plugin for Apk {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "apk".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "apk".into(),
                description: "An Alpine package (.apk)".into(),
                signing: Signing::Embedded(Scheme::RsaSha256),
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["apk"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["apk"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Apk);
