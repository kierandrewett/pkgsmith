//! A plain tar archive of the files, for people who unpack it themselves.

use crate::archive::{self, Compression};
use crate::manifest::Manifest;
use crate::package::Package;
use crate::version::Version;
use anyhow::Result;
use std::path::PathBuf;

/// `name-version-arch.tar.xz`, with the compression the manifest asks for.
pub fn file_name(manifest: &Manifest) -> Result<String> {
    let version = Version::parse(&manifest.version)?.plain();
    let compression = Compression::parse(manifest.tar.compression.as_deref().unwrap_or("xz"))?;
    Ok(format!(
        "{}-{version}-{}.tar{}",
        manifest.name,
        manifest.arch,
        compression.extension()
    ))
}

pub fn build(package: &Package) -> Result<PathBuf> {
    let manifest = package.manifest;
    let version = Version::parse(&manifest.version)?.plain();
    let compression = Compression::parse(manifest.tar.compression.as_deref().unwrap_or("xz"))?;
    let top = if manifest.tar.top_folder {
        format!("{}-{version}/", manifest.name)
    } else {
        String::new()
    };
    let tar = archive::tar(&package.entries, &top, package.options.mtime, &[])?;
    let file = package.output(&file_name(manifest)?);
    std::fs::create_dir_all(&package.options.out)?;
    std::fs::write(&file, compression.compress(&tar)?)?;
    Ok(file)
}
