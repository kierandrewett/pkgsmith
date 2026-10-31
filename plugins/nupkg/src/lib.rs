//! The NuGet package format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct NuPkg;

impl Plugin for NuPkg {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "nupkg".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "nupkg".into(),
                description:
                    "A NuGet package (.nupkg), for dotnet add package and other NuGet clients"
                        .into(),
                signing: Signing::Detached,
            }],
            ..Default::default()
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["nupkg"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["nupkg"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(NuPkg);
