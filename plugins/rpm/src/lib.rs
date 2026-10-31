//! The rpm format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Scheme, Signing};
use std::path::PathBuf;

struct Rpm;

impl Plugin for Rpm {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "rpm".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "rpm".into(),
                description:
                    "An rpm package (.rpm), for Fedora, openSUSE, RHEL and their relatives".into(),
                signing: Signing::Embedded(Scheme::OpenPgp),
            }],
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["rpm"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["rpm"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Rpm);
