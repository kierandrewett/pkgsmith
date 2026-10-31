//! pkgsmith builds distribution packages from one manifest per package.
//!
//! A manifest (YAML) names a package, its metadata, the files to install and its install scripts. `build` turns the
//! same manifest into any supported format. The files come from a staged prefix, a directory laid out like the
//! installed system, so the build of the program itself stays outside this tool.

pub mod archive;
pub mod expand;
pub mod expr;
pub mod formats;
pub mod hash;
pub mod manifest;
pub mod package;
pub mod plan;
pub mod tree;
pub mod version;
