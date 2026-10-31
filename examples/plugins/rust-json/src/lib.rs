//! A pkgsmith plugin in Rust. It adds the format "json": the whole package description as a JSON file, which is handy
//! to see exactly what a plugin receives.
//!
//! Build with `cargo build --release`. The library is `target/release/libpkgsmith_plugin_json.so`. Copy it to a folder
//! on the plugin path (see `pkgsmith plugins`), or run `pkgsmith --plugin-path target/release build`.

use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo, Signing};
use std::path::PathBuf;

struct Json;

impl Plugin for Json {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "json".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo {
                name: "json".into(),
                description: "The package description as a JSON file (.json)".into(),
                signing: Signing::Detached,
            }],
 ..Default::default()
}
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["json"])?;
        Ok(format!("{}-{}.json", package.name, package.version))
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        let file = package.output(&self.file_name(package)?);
        std::fs::create_dir_all(&package.out)?;
        std::fs::write(&file, serde_json::to_vec_pretty(package)?)?;
        host.info(&format!("{} files", package.entries.len()));
        Ok(file)
    }
}

pkgsmith_plugin::export_plugin!(Json);
