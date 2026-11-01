//! MSIX and APPX, the Windows app packages, written unsigned.
//!
//! The package is an OPC zip: the payload files, `AppxManifest.xml`, `AppxBlockMap.xml` (the SHA-256 of each 64 KiB
//! block so Windows can check the package as it reads it) and `[Content_Types].xml`. The zip layout follows the
//! Microsoft reference writer `makemsix` (microsoft/msix-packaging, `src/msix/pack`), so `zip.rs` and `opc.rs` do too.
//! An APPX has the same layout as an MSIX: the [`Flavour`] only changes the file extension, the default `MinVersion`
//! and the words of the messages.
//!
//! Specifications used:
//! - MSIX package and block map: https://learn.microsoft.com/en-us/windows/msix/package/ and the reference writer at
//!   https://github.com/microsoft/msix-packaging/tree/master/src/msix/pack
//! - Package manifest schema: https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/appxmanifest/
//! - Zip file format: https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT (the zip64 records)
//! - Open Packaging Conventions: ECMA-376 Part 2, https://www.ecma-international.org/publications-and-standards/standards/ecma-376/
//!
//! Entries map under `root` (default `VFS/ProgramFilesX64/<name>`), the folder the package maps onto Program Files on
//! the installed system: `/usr/bin/hi` is packaged as `VFS/ProgramFilesX64/<name>/usr/bin/hi`.

use crate::Flavour;
use crate::opc::{self, ContentTypes};
use crate::zip::Writer;
use anyhow::{Context, Result, bail};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Entry, Host, Package};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

const MANIFEST_TYPE: &str = "application/vnd.ms-appx.manifest+xml";
const BLOCK_MAP_TYPE: &str = "application/vnd.ms-appx.blockmap+xml";
const DEFAULT_MAX_TESTED: &str = "10.0.22621.0";
const LANGUAGE: &str = "en-us";

/// The options of an `msix` or `appx` package, the same for both.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Package folder that the Unix root `/` maps to. Default `VFS/ProgramFilesX64/<identity name>`. Use `""` for the
    /// package root.
    root: Option<String>,
    /// Unix path of the program the package starts, an entry of the package. Required.
    executable: String,
    /// Unix path of the PNG the tiles and the store logo use, an entry of the package. Required. Windows scales this one
    /// file for every size, so give it a square image of 150 pixels or more.
    logo: String,
    /// Publisher subject, as signed or declared, for example `CN=Example Ltd`. Required. Windows matches it to the
    /// certificate that signs the package, which pkgsmith does not do.
    publisher: String,
    /// The publisher name Windows shows. Default: the `CN=` value of `publisher`.
    publisher_display_name: Option<String>,
    /// The name Windows shows for the app. Default: the package name.
    display_name: Option<String>,
    /// Oldest Windows version the package runs on. Default: `Flavour::default_min_windows`.
    min_windows: Option<String>,
    /// Newest Windows version the package has been tested on. Default `10.0.22621.0` (Windows 11 22H2).
    max_tested: Option<String>,
}

/// A payload file: its package-relative name and where its bytes are.
struct Payload<'a> {
    name: String,
    src: &'a std::path::Path,
}

/// `name_version_arch.EXTENSION`, the file name Windows tools use.
pub fn file_name(flavour: Flavour, package: &Package) -> Result<String> {
    let name = identity_name(flavour, &package.name)?;
    let version = package_version(flavour, package)?;
    let arch = architecture(flavour, &package.arch)?;
    Ok(format!("{name}_{version}_{arch}.{}", flavour.extension))
}

/// Builds the package and returns its path.
pub fn build(flavour: Flavour, package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let name = identity_name(flavour, &package.name)?;
    if name != package.name {
        host.warn(&format!(
            "the package name {:?} has characters {} names cannot have, so the package is named {name:?}",
            package.name, flavour.label
        ));
    }
    let version = Version::parse(&package.version)?;
    if version.pre.is_some() {
        host.warn(&format!(
            "the pre-release text of {:?} is not kept: {} version is four numbers",
            package.version, flavour.label
        ));
    }
    let arch = architecture(flavour, &package.arch)?;
    if package.arch == "all" {
        host.warn("arch all is written as ProcessorArchitecture neutral; a Windows program runs on one architecture, so use amd64 or arm64 for it");
    }
    warn_unexpressed(flavour, package, host);

    for (field, value) in [
        ("publisher", &options.publisher),
        ("executable", &options.executable),
        ("logo", &options.logo),
    ] {
        if value.is_empty() {
            bail!("{}.{field} is required", flavour.name);
        }
    }
    let root = match &options.root {
        Some(root) => root.trim_matches('/').to_owned(),
        None => format!("VFS/ProgramFilesX64/{name}"),
    };
    let payloads = payloads(package, &root)?;
    let executable = package_path(flavour, &package.entries, &root, &options.executable, "executable")?;
    if !executable.to_ascii_lowercase().ends_with(".exe") {
        bail!(
            "{}.executable {:?} must be a .exe file: Windows and makemsix reject any other Application Executable",
            flavour.name, options.executable
        );
    }
    let logo = package_path(flavour, &package.entries, &root, &options.logo, "logo")?;
    if !logo.to_ascii_lowercase().ends_with(".png") {
        bail!("{}.logo {:?} must be a PNG file", flavour.name, options.logo);
    }
    let publisher_display_name = match &options.publisher_display_name {
        Some(text) => text.clone(),
        None => common_name(&options.publisher).with_context(|| {
            format!(
                "{0}.publisher_display_name is needed: {0}.publisher has no CN= value",
                flavour.name
            )
        })?,
    };
    let manifest = Manifest {
        name: &name,
        publisher: &options.publisher,
        publisher_display_name: &publisher_display_name,
        version: package_version(flavour, package)?,
        arch,
        display_name: options.display_name.as_deref().unwrap_or(&package.name),
        description: match package.summary() {
            "" => options.display_name.as_deref().unwrap_or(&package.name),
            summary => summary,
        },
        logo: &logo,
        executable: &executable,
        min_windows: options
            .min_windows
            .as_deref()
            .unwrap_or(flavour.default_min_windows),
        max_tested: options.max_tested.as_deref().unwrap_or(DEFAULT_MAX_TESTED),
    };
    let manifest_xml = manifest.to_xml();

    let output_name = file_name(flavour, package)?;
    let out_path = package.output(&output_name);
    let file =
        File::create(&out_path).with_context(|| format!("cannot create {}", out_path.display()))?;
    let mut zip = Writer::new(BufWriter::new(file), package.mtime);
    let mut members = Vec::with_capacity(payloads.len() + 1);
    let mut types = ContentTypes::default();
    for payload in &payloads {
        let source = File::open(payload.src)
            .with_context(|| format!("cannot read {}", payload.src.display()))?;
        members.push(zip.add(&payload.name, source, opc::stored(&payload.name))?);
        types.add_payload(&payload.name, opc::content_type(&payload.name));
    }
    members.push(zip.add(opc::MANIFEST, manifest_xml.as_bytes(), false)?);
    types.override_part(&format!("/{}", opc::MANIFEST), MANIFEST_TYPE);
    let block_map = opc::block_map(&members);
    zip.add(opc::BLOCK_MAP, block_map.as_bytes(), false)?;
    types.override_part(&format!("/{}", opc::BLOCK_MAP), BLOCK_MAP_TYPE);
    zip.add(opc::CONTENT_TYPES, types.to_xml().as_bytes(), false)?;
    zip.finish()
        .with_context(|| format!("cannot write {}", out_path.display()))?;
    Ok(out_path)
}

/// The package name as an identity name: letters, digits, dots and dashes, 3 to 50 characters.
fn identity_name(flavour: Flavour, name: &str) -> Result<String> {
    let identity: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if !(3..=50).contains(&identity.len()) {
        bail!(
            "the package name {name:?} must be 3 to 50 characters for {} (letters, digits, dots and dashes)",
            flavour.label
        );
    }
    Ok(identity)
}

/// The four-part version: the three numbers of the version, then the release. These versions are numbers only.
fn package_version(flavour: Flavour, package: &Package) -> Result<String> {
    let version = Version::parse(&package.version)?;
    let mut parts = Vec::with_capacity(4);
    for number in version.core.split('.') {
        let value: u16 = number.parse().with_context(|| {
            format!("version {:?}: {} numbers go up to 65535", package.version, flavour.label)
        })?;
        parts.push(value);
    }
    let release: u16 = package.release.parse().with_context(|| {
        format!(
            "release {:?}: {} needs a number up to 65535",
            package.release, flavour.label
        )
    })?;
    parts.push(release);
    Ok(parts
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join("."))
}

/// The `ProcessorArchitecture` for a package architecture.
fn architecture(flavour: Flavour, arch: &str) -> Result<&'static str> {
    match arch {
        "amd64" => Ok("x64"),
        "arm64" => Ok("arm64"),
        "all" => Ok("neutral"),
        other => bail!("{} has no architecture for {other:?}", flavour.label),
    }
}

/// The `CN=` value of a distinguished name, up to the next comma.
fn common_name(publisher: &str) -> Option<String> {
    let rest = publisher.split_once("CN=")?.1;
    let value = rest.split(',').next()?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The package-relative name of `unix`, which must be a file entry of the package.
fn package_path(flavour: Flavour, entries: &[Entry], root: &str, unix: &str, what: &str) -> Result<String> {
    let wanted = unix.trim_matches('/');
    let found = entries.iter().any(|entry| {
        entry.path.trim_start_matches('/') == wanted && matches!(entry.data, Data::File { .. })
    });
    if !found {
        bail!("{}.{what} {unix:?} is not a file in the package", flavour.name);
    }
    Ok(join(root, wanted))
}

/// `root` and an absolute Unix path, joined with `/`.
fn join(root: &str, path: &str) -> String {
    let path = path.trim_start_matches('/');
    if root.is_empty() {
        path.to_owned()
    } else {
        format!("{root}/{path}")
    }
}

/// The payload files, each under its package-relative name.
fn payloads<'a>(package: &'a Package, root: &str) -> Result<Vec<Payload<'a>>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in &package.entries {
        let Data::File { src, .. } = &entry.data else {
            continue;
        };
        let name = join(root, &entry.path);
        opc::check_name(&name)?;
        if !seen.insert(name.clone()) {
            bail!("two entries are packaged as {name:?}");
        }
        out.push(Payload { name, src });
    }
    Ok(out)
}

/// Warns about each manifest field the format cannot express. Nothing here stops the build.
fn warn_unexpressed(flavour: Flavour, package: &Package, host: &Host) {
    let label = flavour.label;
    let lists = [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, values) in lists {
        if !values.is_empty() {
            host.warn(&format!(
                "{label} has no {field} field, so {} is not written",
                values.join(", ")
            ));
        }
    }
    let scripts = &package.scripts;
    if [
        &scripts.pre_install,
        &scripts.post_install,
        &scripts.pre_remove,
        &scripts.post_remove,
    ]
    .iter()
    .any(|script| script.is_some())
    {
        host.warn(&format!("{label} runs no install or remove scripts, so the package scripts are not written"));
    }
    let mut ghosts = 0;
    let mut links = 0;
    let mut configs = 0;
    let mut owned = false;
    for entry in &package.entries {
        match entry.data {
            Data::Ghost => ghosts += 1,
            Data::Symlink { .. } => links += 1,
            Data::File { .. } | Data::Dir => {}
        }
        if entry.config {
            configs += 1;
        }
        let standard_mode = matches!(entry.mode & 0o7777, 0o644 | 0o755);
        if entry.owner != "root" || entry.group != "root" || !standard_mode {
            owned = true;
        }
    }
    if ghosts > 0 {
        host.warn(&format!(
            "{ghosts} ghost entries are owned by no package, so they are not written"
        ));
    }
    if links > 0 {
        host.warn(&format!(
            "{links} symbolic links are not written: {label} packages do not carry them"
        ));
    }
    if configs > 0 {
        host.warn(&format!(
            "{configs} configuration files are not kept apart: {label} has no config flag"
        ));
    }
    if owned {
        host.warn("owner, group and mode are not kept: Windows sets the access rules of the installed files");
    }
    let mut folders_with_files: HashSet<String> = HashSet::new();
    for entry in &package.entries {
        if let Data::File { .. } = entry.data {
            let mut path = entry.path.as_str();
            while let Some((parent, _)) = path.rsplit_once('/') {
                if parent.is_empty() {
                    break;
                }
                folders_with_files.insert(parent.to_owned());
                path = parent;
            }
        }
    }
    let empty = package
        .entries
        .iter()
        .filter(|e| e.is_dir() && !folders_with_files.contains(&e.path))
        .count();
    if empty > 0 {
        host.warn(&format!("{empty} empty folders are not written: a package holds files, and folders only as their paths"));
    }
}

/// The manifest's values.
struct Manifest<'a> {
    name: &'a str,
    publisher: &'a str,
    publisher_display_name: &'a str,
    version: String,
    arch: &'static str,
    display_name: &'a str,
    description: &'a str,
    /// Package-relative path of the logo, with backslashes.
    logo: &'a str,
    /// Package-relative path of the program, with backslashes.
    executable: &'a str,
    min_windows: &'a str,
    max_tested: &'a str,
}

impl Manifest<'_> {
    /// `AppxManifest.xml`. The element order is the schema's.
    fn to_xml(&self) -> String {
        let windows_path = |path: &str| path.replace('/', "\\");
        let logo = windows_path(self.logo);
        let executable = windows_path(self.executable);
        let escape = opc::escape;
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <Package xmlns=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10\"\n\
             \x20   xmlns:uap=\"http://schemas.microsoft.com/appx/manifest/uap/windows10\"\n\
             \x20   xmlns:rescap=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities\"\n\
             \x20   IgnorableNamespaces=\"uap rescap\">\n\
             \x20 <Identity Name=\"{name}\" Publisher=\"{publisher}\" Version=\"{version}\" ProcessorArchitecture=\"{arch}\" />\n\
             \x20 <Properties>\n\
             \x20   <DisplayName>{display}</DisplayName>\n\
             \x20   <PublisherDisplayName>{publisher_display}</PublisherDisplayName>\n\
             \x20   <Logo>{logo}</Logo>\n\
             \x20   <Description>{description}</Description>\n\
             \x20 </Properties>\n\
             \x20 <Dependencies>\n\
             \x20   <TargetDeviceFamily Name=\"Windows.Desktop\" MinVersion=\"{min}\" MaxVersionTested=\"{max}\" />\n\
             \x20 </Dependencies>\n\
             \x20 <Resources>\n\
             \x20   <Resource Language=\"{LANGUAGE}\" />\n\
             \x20 </Resources>\n\
             \x20 <Applications>\n\
             \x20   <Application Id=\"App\" Executable=\"{executable}\" EntryPoint=\"Windows.FullTrustApplication\">\n\
             \x20     <uap:VisualElements DisplayName=\"{display}\" Description=\"{description}\" BackgroundColor=\"transparent\" Square150x150Logo=\"{logo}\" Square44x44Logo=\"{logo}\" />\n\
             \x20   </Application>\n\
             \x20 </Applications>\n\
             \x20 <Capabilities>\n\
             \x20   <rescap:Capability Name=\"runFullTrust\" />\n\
             \x20 </Capabilities>\n\
             </Package>\n",
            name = escape(self.name),
            publisher = escape(self.publisher),
            version = self.version,
            arch = self.arch,
            display = escape(self.display_name),
            publisher_display = escape(self.publisher_display_name),
            logo = escape(&logo),
            description = escape(self.description),
            min = escape(self.min_windows),
            max = escape(self.max_tested),
            executable = escape(&executable),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_versions_follow_app_package_rules() {
        use crate::{APPX, MSIX};
        assert_eq!(identity_name(MSIX, "hi_tool+").unwrap(), "hi-tool-");
        assert!(identity_name(MSIX, "ab").is_err());
        assert_eq!(architecture(MSIX, "amd64").unwrap(), "x64");
        assert_eq!(architecture(APPX, "arm64").unwrap(), "arm64");
        assert!(architecture(APPX, "riscv64").is_err());
        let error = identity_name(APPX, "ab").unwrap_err().to_string();
        assert!(error.contains("APPX"), "{error}");
        let error = architecture(MSIX, "riscv64").unwrap_err().to_string();
        assert!(error.contains("MSIX"), "{error}");
        assert_eq!(
            common_name("CN=Example Ltd, O=Example").as_deref(),
            Some("Example Ltd")
        );
        assert_eq!(common_name("O=Example"), None);
        assert_eq!(
            join("VFS/ProgramFilesX64/hi", "/usr/bin/hi"),
            "VFS/ProgramFilesX64/hi/usr/bin/hi"
        );
        assert_eq!(join("", "/usr/bin/hi"), "usr/bin/hi");
    }
}
