//! The plugin API of pkgsmith.
//!
//! Every package format of pkgsmith is a plugin: a shared library named `pkgsmith-plugin-*` that pkgsmith finds on
//! the plugin path and loads. This crate has what a plugin needs:
//!
//! * [`Package`], the description of one package that the host sends to the plugin as JSON.
//! * [`abi`], the C interface of the shared library, so a plugin can be written in any language.
//! * [`plugin`], the [`Plugin`] trait and [`export_plugin!`] for plugins written in Rust.
//! * [`archive`], [`hash`] and [`version`], the helpers the built-in formats use.

pub mod abi;
pub mod archive;
pub mod entry;
pub mod hash;
pub mod package;
pub mod plugin;
pub mod version;

pub use abi::Scheme;
pub use entry::{Data, Entry};
pub use package::{Package, Scripts};
pub use plugin::{FormatInfo, Host, Plugin, PluginInfo, SignRequest, SignerInfo, Signing};
