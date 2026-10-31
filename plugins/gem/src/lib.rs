//! The RubyGems format of pkgsmith (`.gem`), as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Gem;

impl Plugin for Gem {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "gem".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "gem".into(),
                description: "A RubyGems package (.gem), for gem install".into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["gem"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["gem"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Gem);
