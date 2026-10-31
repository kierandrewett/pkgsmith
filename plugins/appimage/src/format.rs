//! AppImage packages (type 2): an ELF runtime with a SquashFS image of an AppDir appended to it.
//!
//! The runtime is not built here. The manifest names a runtime file on disk (`appimage.runtime`), and the package is
//! that file byte for byte followed by the SquashFS image. The runtime finds the image where its own ELF data ends, so
//! the runtime file must end exactly there; `check_runtime` checks this.
//!
//! The image is an AppDir. Its root holds `AppRun` (a symlink to the program), `NAME.desktop`, the icon as
//! `NAME.png` or `NAME.svg`, and `.DirIcon`, a symlink to the icon. The files of the package keep their install paths
//! without the leading `/`, so `/usr/bin/hello` is `usr/bin/hello` inside the AppDir.
//!
//! Specification: <https://github.com/AppImage/AppImageSpec/blob/master/draft.md> (the type 2 format, the contents of
//! an AppDir and the desktop entry). Runtime: <https://github.com/AppImage/type2-runtime>. The runtime links zstd and
//! zlib only (see its Makefile), so the image is compressed with zstd (the default) or gzip.

use anyhow::{Context, Result, bail};
use backhand::compression::Compressor;
use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};
use pkgsmith_plugin::entry::{self, Data};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use serde::Deserialize;
use std::fs::File;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// ELF machine number of x86-64, from the ELF specification.
const EM_X86_64: u16 = 0x3e;
/// ELF machine number of AArch64, from the ELF specification.
const EM_AARCH64: u16 = 0xb7;
/// The type 2 magic: `AI` and the byte 2, at offset 8 of the runtime.
const TYPE2_MAGIC: &[u8] = b"AI\x02";

/// The `appimage:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Path of a type 2 runtime file, for example `runtime-x86_64` from the type2-runtime releases. Required.
    runtime: Option<PathBuf>,
    /// The program AppRun starts: a path of a file in the package, without a leading `/`. Default `usr/bin/NAME`.
    exec: Option<String>,
    /// An icon file on disk, `.png` or `.svg`. Required: the AppImage spec requires a `.DirIcon`.
    icon: Option<PathBuf>,
    /// `zstd` (the default) or `gzip`.
    compression: Option<String>,
    /// The desktop entry, written as `NAME.desktop` in the root of the AppDir.
    #[serde(default)]
    desktop: Desktop,
}

/// The keys of the desktop entry that the manifest can set. `Type`, `Exec` and `Icon` are always derived.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Desktop {
    /// `Name=`. Default: the package name.
    name: Option<String>,
    /// `Comment=`. Default: the summary of the description.
    comment: Option<String>,
    /// `Categories=`, a list that ends in `;`. Default `Utility;`.
    categories: Option<String>,
    /// `Terminal=`. Default `false`.
    terminal: Option<bool>,
}

/// The name is the file name of the desktop entry and the icon, so it must be a plain file name.
fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('.') || name.contains('/') {
        bail!("an AppImage needs a package name that is a plain file name, not {name:?}");
    }
    Ok(())
}

/// The architecture as the AppImage file name spells it, and the ELF machine of a runtime for it.
fn target(arch: &str) -> Result<(&'static str, u16)> {
    match arch {
        "amd64" => Ok(("x86_64", EM_X86_64)),
        "arm64" => Ok(("aarch64", EM_AARCH64)),
        "all" => bail!("an AppImage runs on one architecture: set arch to amd64 or arm64"),
        other => bail!("arch {other:?} has no AppImage architecture; use amd64 or arm64"),
    }
}

/// The path of an entry inside the AppDir: `usr/bin/hello`, never `/usr/bin/hello`.
fn appdir_path(path: &str) -> &str {
    path.trim_start_matches('/')
}

/// Read `N` little-endian bytes at `at`, failing when the runtime is too short.
fn bytes_at<const N: usize>(bytes: &[u8], at: u64) -> Result<[u8; N]> {
    let start = usize::try_from(at).context("the runtime is too large")?;
    let end = start.checked_add(N).context("the runtime is too large")?;
    let slice = bytes
        .get(start..end)
        .with_context(|| format!("the runtime is truncated at byte {at}"))?;
    let out: [u8; N] = slice.try_into()?;
    Ok(out)
}

fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .context("the ELF headers of the runtime point outside the file")
}

/// The size of the ELF data of a 64-bit ELF file, computed as the runtime computes it: the later of the end of the
/// section header table and the end of the section that the last section header describes.
fn elf_size(bytes: &[u8]) -> Result<u64> {
    let shoff = u64::from_le_bytes(bytes_at(bytes, 0x28)?);
    let shentsize = u64::from(u16::from_le_bytes(bytes_at(bytes, 0x3a)?));
    let shnum = u64::from(u16::from_le_bytes(bytes_at(bytes, 0x3c)?));
    if shnum == 0 {
        bail!("the runtime has no section headers, so its size is unknown");
    }
    let table_end = add(shoff, shentsize * shnum)?;
    let last = add(shoff, shentsize * (shnum - 1))?;
    let section_offset = u64::from_le_bytes(bytes_at(bytes, add(last, 0x18)?)?);
    let section_size = u64::from_le_bytes(bytes_at(bytes, add(last, 0x20)?)?);
    Ok(table_end.max(add(section_offset, section_size)?))
}

/// Check that the runtime is a 64-bit little-endian type 2 runtime for `machine`, and that it has no bytes after its
/// ELF data: the SquashFS image starts exactly where the runtime's ELF data ends.
fn check_runtime(path: &Path, bytes: &[u8], machine: u16) -> Result<()> {
    if !bytes.starts_with(b"\x7fELF") || bytes.get(4) != Some(&2) || bytes.get(5) != Some(&1) {
        bail!("{} is not a 64-bit little-endian ELF file", path.display());
    }
    if bytes.get(8..11) != Some(TYPE2_MAGIC) {
        bail!(
            "{} lacks the AppImage type 2 magic at offset 8",
            path.display()
        );
    }
    let found = u16::from_le_bytes(bytes_at(bytes, 18)?);
    if found != machine {
        bail!(
            "{} is for ELF machine {found:#x}, but this package needs {machine:#x}",
            path.display()
        );
    }
    let size = elf_size(bytes)?;
    if size != bytes.len() as u64 {
        bail!(
            "{} is {} bytes but its ELF data ends at {size}; give the bare runtime file, because the image is appended after the ELF data",
            path.display(),
            bytes.len()
        );
    }
    Ok(())
}

/// The extension of the icon in the AppDir root: `png` or `svg`.
fn icon_extension(icon: &Path) -> Result<&'static str> {
    let extension = icon
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => Ok("png"),
        Some("svg") => Ok("svg"),
        _ => bail!("the icon {} must be a .png or .svg file", icon.display()),
    }
}

/// `NAME-VERSION-ARCH.AppImage`, with the architecture spelled as AppImage tools spell it.
pub fn file_name(package: &Package) -> Result<String> {
    check_name(&package.name)?;
    let (arch, _) = target(&package.arch)?;
    let version = Version::parse(&package.version)?.plain();
    Ok(format!("{}-{version}-{arch}.AppImage", package.name))
}

fn header(mode: u32, mtime: u64) -> NodeHeader {
    NodeHeader::new(
        (mode & 0o7777) as u16,
        0,
        0,
        u32::try_from(mtime).unwrap_or(u32::MAX),
    )
}

/// The `NAME.desktop` file. `exec` is the path of the program in the AppDir; `Exec=` names its file name, which the
/// desktop integration replaces with the path of the AppImage.
fn desktop_entry(package: &Package, desktop: &Desktop, exec: &str) -> Result<String> {
    let name = desktop.name.as_deref().unwrap_or(&package.name);
    let comment = desktop.comment.as_deref().unwrap_or(package.summary());
    let categories = desktop.categories.as_deref().unwrap_or("Utility;");
    let terminal = desktop.terminal.unwrap_or(false);
    for value in [name, comment, categories] {
        if value.contains(['\n', '\r']) {
            bail!("the desktop entry values cannot contain line breaks: {value:?}");
        }
    }
    if !categories.ends_with(';') {
        bail!(
            "desktop.categories {categories:?} must end with ';', as the desktop entry spec requires"
        );
    }
    let command = exec.rsplit('/').next().unwrap_or(exec);
    let mut lines = vec![
        "[Desktop Entry]".to_string(),
        "Type=Application".to_string(),
        format!("Name={name}"),
    ];
    if !comment.is_empty() {
        lines.push(format!("Comment={comment}"));
    }
    lines.push(format!("Exec={command} %U"));
    lines.push(format!("Icon={}", package.name));
    lines.push(format!("Categories={categories}"));
    lines.push(format!("Terminal={terminal}"));
    Ok(lines.join("\n") + "\n")
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    check_name(&package.name)?;
    let (_, machine) = target(&package.arch)?;
    let name = package.name.as_str();

    let runtime_path = options.runtime.as_deref().context(
        "the appimage section needs runtime: the path of a type 2 runtime file, such as runtime-x86_64 from https://github.com/AppImage/type2-runtime/releases",
    )?;
    let runtime = std::fs::read(runtime_path)
        .with_context(|| format!("cannot read the runtime {}", runtime_path.display()))?;
    check_runtime(runtime_path, &runtime, machine)?;

    let compression = match options.compression.as_deref().unwrap_or("zstd") {
        "zstd" => Compressor::Zstd,
        "gzip" => Compressor::Gzip,
        other => {
            bail!("appimage.compression {other:?} must be zstd or gzip, which the runtime reads")
        }
    };

    let icon_path = options.icon.as_deref().context(
        "the appimage section needs icon: a .png or .svg file, because the AppImage spec requires a .DirIcon",
    )?;
    let icon_ext = icon_extension(icon_path)?;
    let icon_file = format!("{name}.{icon_ext}");
    let desktop_file = format!("{name}.desktop");

    let exec = match options.exec.as_deref() {
        Some(path) => appdir_path(path).to_string(),
        None => format!("usr/bin/{name}"),
    };
    let program = package
        .entries
        .iter()
        .find(|e| appdir_path(&e.path) == exec.as_str())
        .with_context(|| format!("exec {exec:?} is not a file in the package"))?;
    if !matches!(program.data, Data::File { .. }) {
        bail!("exec {exec:?} must be a regular file");
    }
    if program.mode & 0o111 == 0 {
        bail!("exec {exec:?} is not executable (mode {:o})", program.mode);
    }

    // The AppDir root is ours: a package file with one of these names would be overwritten or overwrite the AppDir.
    let reserved = [
        "AppRun",
        ".DirIcon",
        icon_file.as_str(),
        desktop_file.as_str(),
    ];
    for entry in &package.entries {
        let path = appdir_path(&entry.path);
        if reserved.contains(&path) {
            bail!(
                "the package has {}, but the AppDir root holds {path} for the AppImage; install it elsewhere",
                entry.path
            );
        }
    }

    for (field, list) in [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ] {
        if !list.is_empty() {
            host.warn(&format!(
                "an AppImage carries its own libraries and has no package database, so {field} is ignored"
            ));
        }
    }
    let scripts = [
        ("pre_install", &package.scripts.pre_install),
        ("post_install", &package.scripts.post_install),
        ("pre_remove", &package.scripts.pre_remove),
        ("post_remove", &package.scripts.post_remove),
    ];
    for (field, script) in scripts {
        if script.is_some() {
            host.warn(&format!(
                "an AppImage runs no install or remove scripts, so {field} is ignored"
            ));
        }
    }
    for (field, value) in [
        ("maintainer", &package.maintainer),
        ("homepage", &package.homepage),
        ("license", &package.license),
    ] {
        if value.is_some() {
            host.warn(&format!(
                "an AppImage has no field for {field}, so it is ignored"
            ));
        }
    }
    if package.entries.iter().any(|e| e.config) {
        host.warn("an AppImage is read-only and has no configuration files, so the config flag is ignored");
    }
    if package
        .entries
        .iter()
        .any(|e| e.owner != "root" || e.group != "root")
    {
        host.warn("the files of an AppImage belong to the user who runs them, so owner and group are ignored");
    }

    let mtime = package.mtime;
    let mut image = FilesystemWriter::default();
    image.set_time(u32::try_from(mtime).unwrap_or(u32::MAX));
    image.set_root_mode(0o755);
    image.set_compressor(FilesystemCompressor::new(compression, None)?);
    let dir = header(0o755, mtime);
    for item in entry::with_parents(&package.entries) {
        let path = format!("/{}", appdir_path(&item.path));
        let head = header(item.mode, mtime);
        match &item.data {
            Data::Dir => image.push_dir_all(&path, head)?,
            Data::Symlink { target } => {
                if let Some(parent) = Path::new(&path).parent() {
                    image.push_dir_all(parent, dir)?;
                }
                image.push_symlink(target, &path, head)?;
            }
            Data::File { src, .. } => {
                let file =
                    File::open(src).with_context(|| format!("cannot read {}", src.display()))?;
                image.push_file(file, &path, head)?;
            }
            Data::Ghost => {}
        }
    }

    image.push_symlink(&exec, "/AppRun", header(0o777, mtime))?;
    let icon = File::open(icon_path)
        .with_context(|| format!("cannot read the icon {}", icon_path.display()))?;
    image.push_file(icon, format!("/{icon_file}"), header(0o644, mtime))?;
    image.push_symlink(&icon_file, "/.DirIcon", header(0o777, mtime))?;
    let desktop = desktop_entry(package, &options.desktop, &exec)?;
    image.push_file(
        Cursor::new(desktop.into_bytes()),
        format!("/{desktop_file}"),
        header(0o644, mtime),
    )?;

    let mut squashfs = Cursor::new(Vec::new());
    image.write(&mut squashfs)?;
    let mut bytes = runtime;
    bytes.extend_from_slice(&squashfs.into_inner());

    let file = package.output(&file_name(package)?);
    std::fs::write(&file, bytes).with_context(|| format!("cannot write {}", file.display()))?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
    Ok(file)
}
