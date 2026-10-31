//! The deb and ipk formats of pkgsmith, as a plugin. The two share one control file layout.

mod format;

use anyhow::Result;
use format::Flavour;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Deb;

fn flavour(package: &Package) -> Result<Flavour> {
    Ok(match expect_format(package, &["deb", "ipk"])? {
        "ipk" => Flavour::Ipk,
        _ => Flavour::Deb,
    })
}

impl Plugin for Deb {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "deb".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![
                FormatInfo {
                    name: "deb".into(),
                    description: "A Debian package (.deb), for Debian, Ubuntu and their relatives"
                        .into(),
                    signing: Signing::Detached,
                },
                FormatInfo {
                    name: "ipk".into(),
                    description: "An OpenWrt package (.ipk)".into(),
                    signing: Signing::Detached,
                },
            ],
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        format::file_name(package, flavour(package)?)
    }

    fn build(&self, package: &Package, _host: &Host) -> Result<PathBuf> {
        format::build(package, flavour(package)?)
    }
}

pkgsmith_plugin::export_plugin!(Deb);
