//! A plain tar archive of the files, for people who unpack it themselves.

use anyhow::Result;
use pkgsmith_plugin::Package;
use pkgsmith_plugin::archive::{self, Compression};
use pkgsmith_plugin::version::Version;
use serde::Deserialize;
use std::path::PathBuf;

/// The `tar:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// `gzip`, `xz` (default), `zstd`, `bzip2`, `none`.
    compression: Option<String>,
    /// Put every file below a top folder, `{name}-{version}` when `true`.
    #[serde(default)]
    top_folder: bool,
}

/// `name-version-arch.tar.xz`, with the compression the manifest asks for.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    let version = Version::parse(&package.version)?.plain();
    let compression = Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
    Ok(format!(
        "{}-{version}-{}.tar{}",
        package.name,
        package.arch,
        compression.extension()
    ))
}

pub fn build(package: &Package) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let version = Version::parse(&package.version)?.plain();
    let compression = Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
    let top = if options.top_folder {
        format!("{}-{version}/", package.name)
    } else {
        String::new()
    };
    let tar = archive::tar(&package.entries, &top, package.mtime, &[])?;
    let file = package.output(&file_name(package)?);
    std::fs::create_dir_all(&package.out)?;
    std::fs::write(&file, compression.compress(&tar)?)?;
    Ok(file)
}
