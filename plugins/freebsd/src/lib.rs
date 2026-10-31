//! The FreeBSD `pkg` format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct FreeBsd;

impl Plugin for FreeBsd {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "freebsd".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "freebsd".into(),
                description: "A FreeBSD pkg package (.pkg), for pkg(8) on FreeBSD".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["freebsd"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["freebsd"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(FreeBsd);
