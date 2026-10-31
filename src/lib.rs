//! pkgsmith builds distribution packages from one manifest per package.
//!
//! A manifest (YAML) names a package, its metadata, the files to install and its install scripts. `build` turns the
//! same manifest into every package format a plugin provides. The formats are shared libraries that `plugins` finds
//! and loads, and `pkgsmith-plugin` is the API they implement.

pub mod container;
pub mod expand;
pub mod expr;
pub mod install;
pub mod manifest;
pub mod package;
pub mod plan;
pub mod plugins;
pub mod runner;
pub mod signing;
pub mod tree;
pub mod ui;
