//! The OCI image format of pkgsmith, as a plugin.

mod format;

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Oci;

impl Plugin for Oci {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "oci".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "oci".into(),
                description: "An OCI container image (.oci.tar), for podman, docker and containerd"
                    .into(),
                signing: Signing::None,
            }],
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["oci"])?;
        format::file_name(package)
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        expect_format(package, &["oci"])?;
        format::build(package, host)
    }
}

pkgsmith_plugin::export_plugin!(Oci);
