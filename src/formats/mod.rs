//! The package formats. Each one turns a `Package` into a file in the output folder.

use crate::package::Package;
use anyhow::{Result, bail};
use std::path::PathBuf;

pub mod arch;
pub mod deb;
pub mod rpm;
pub mod tarball;

/// Every format name, in the order `pkgsmith formats` lists them.
pub const ALL: &[&str] = &["tar", "deb", "archlinux", "ipk", "rpm"];

pub fn build(package: &Package) -> Result<PathBuf> {
    match package.format {
        "tar" => tarball::build(package),
        "deb" => deb::build(package, deb::Flavour::Deb),
        "ipk" => deb::build(package, deb::Flavour::Ipk),
        "archlinux" => arch::build(package),
        "rpm" => rpm::build(package),
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
