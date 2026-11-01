//! The writer of Windows app packages, `.msix` and `.appx`, that the `msix` and `appx` plugins of pkgsmith share.
//!
//! The two formats are the same container: an OPC zip with `AppxManifest.xml` and `AppxBlockMap.xml`. They differ in the
//! file extension and in the oldest Windows version the manifest can ask for, so a [`Flavour`] names those and the rest
//! of the code is common.

mod format;
mod opc;
mod zip;

pub use format::{build, file_name};

/// What differs between the two package formats.
#[derive(Debug, Clone, Copy)]
pub struct Flavour {
    /// The name of the format, and of the keys in messages: `msix.logo is required`.
    pub name: &'static str,
    /// The name of the format in prose: `MSIX`.
    pub label: &'static str,
    /// The file extension, without the dot.
    pub extension: &'static str,
    /// The `MinVersion` of the manifest when the script gives none.
    pub default_min_windows: &'static str,
}

/// MSIX, the format of Windows 10 1809 and later (`10.0.17763.0`).
pub const MSIX: Flavour = Flavour {
    name: "msix",
    label: "MSIX",
    extension: "msix",
    default_min_windows: "10.0.17763.0",
};

/// APPX, the older name of the same container. A desktop program in an APPX needs the full-trust support of Windows 10
/// 1607 (`10.0.14393.0`), the first version that runs one.
pub const APPX: Flavour = Flavour {
    name: "appx",
    label: "APPX",
    extension: "appx",
    default_min_windows: "10.0.14393.0",
};
