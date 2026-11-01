//! pkgsmith builds distribution packages from one script, `pkgsmith.star`.
//!
//! The script (Starlark) declares targets: a package, a format and the job that builds its files. `build` writes every
//! package format a plugin provides. The formats are shared libraries that `plugins` finds and loads, and
//! `pkgsmith-plugin` is the API they implement.

pub mod host;
pub mod install;
pub mod manifest;
pub mod package;
pub mod plan;
pub mod plugins;
pub mod runner;
pub mod script;
pub mod signing;
pub mod tree;
pub mod ui;
