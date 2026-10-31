//! The package formats. Each one turns a `Package` into a file in the output folder.

use crate::package::Package;
use anyhow::{Result, bail};
use std::path::PathBuf;

pub mod apk;
pub mod arch;
pub mod deb;
pub mod rpm;
pub mod tarball;
pub mod zip;

/// Every format name, in the order `pkgsmith formats` lists them.
pub const ALL: &[&str] = &["tar", "deb", "archlinux", "ipk", "rpm", "apk", "zip"];

/// The name of the file a build writes, from the manifest alone.
pub fn file_name(manifest: &crate::manifest::Manifest, format: &str) -> Result<String> {
    match format {
        "tar" => tarball::file_name(manifest),
        "deb" => deb::file_name(manifest, deb::Flavour::Deb),
        "ipk" => deb::file_name(manifest, deb::Flavour::Ipk),
        "archlinux" => arch::file_name(manifest),
        "rpm" => rpm::file_name(manifest),
        "apk" => apk::file_name(manifest),
        "zip" => zip::file_name(manifest),
        other => bail!("unknown format {other:?}; supported: {}", ALL.join(", ")),
    }
}

pub fn build(package: &Package) -> Result<PathBuf> {
    match package.format {
        "tar" => tarball::build(package),
        "deb" => deb::build(package, deb::Flavour::Deb),
        "ipk" => deb::build(package, deb::Flavour::Ipk),
        "archlinux" => arch::build(package),
        "rpm" => rpm::build(package),
        "apk" => apk::build(package),
        "zip" => zip::build(package),
        other => bail!("unknown format {other:?}; supported: {}", ALL.join(", ")),
    }
}

/// Dependency names must be real package names by now. `@name` helpers are resolved before a format runs.
pub fn ensure_resolved(list: &[String], field: &str) -> Result<()> {
    if let Some(unresolved) = list.iter().find(|item| item.starts_with('@')) {
        bail!(
            "{field}: {unresolved:?} is not resolved; give pkgsmith the dependency table that defines it"
        );
    }
    Ok(())
}
