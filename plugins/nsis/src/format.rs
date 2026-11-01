//! NSIS installers (`.exe`): a Windows installer that `makensis` compiles from a script.
//!
//! No pure-Rust writer exists for the NSIS stub, so this plugin writes the NSIS script itself, copies the files into a
//! staging folder, and runs `makensis` on the script. The installer copies every file to `install_dir`, writes an
//! uninstaller, and registers the program under the Add/Remove Programs key (HKLM for `execution_level: admin`, HKCU
//! for `user`). Configuration files are copied only when missing and kept on uninstall.
//!
//! Each file gets its own `File` instruction, not `File /r`: the script then holds exactly the files of the manifest,
//! so the uninstaller removes exactly what was installed.
//!
//! Every string that reaches the script is written with NSIS escapes, so a name with a dollar sign, a quote or a
//! backslash cannot change the script. Strings used at run time use `$$`, `$\"`, `$\r`, `$\n` and `$\t`. Version
//! resource values (`VIAddVersionKey`) are read at build time, where `$$` is not an escape; they use `vi_literal`,
//! which refuses `${` and `$\`. Windows file names are checked for the characters and device names Windows refuses.
//!
//! References: the NSIS scripting reference <https://nsis.sourceforge.io/Docs/Chapter4.html> and the NSIS user manual
//! <https://nsis.sourceforge.io/Docs/Chapter3.html>.

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Entry, Host, Package};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The `nsis:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Where the files go: an NSIS constant and folders, or a drive and folders. For example
    /// `$PROGRAMFILES64\Example` or `D:\Apps\Example`. The constant is one of NSIS's, such as `$PROGRAMFILES64`,
    /// `$LOCALAPPDATA` or `$APPDATA`. Default: `$PROGRAMFILES64\NAME` for `admin`, `$LOCALAPPDATA\Programs\NAME` for
    /// `user`.
    install_dir: Option<String>,
    /// The Unix folder that the entry paths are relative to. The entry `/usr/bin/hi` goes to `bin\hi` under
    /// `install_dir`. Default: `/usr`. Every file and folder of the package must be under it.
    prefix: Option<String>,
    /// `lzma` (default), `zlib` or `bzip2`. The compression of the installer.
    compression: Option<String>,
    /// `admin` (default): the installer asks for administrator rights, installs for all users and registers the
    /// program in HKLM. `user`: no rights needed, installs for the current user and registers in HKCU.
    execution_level: Option<String>,
    /// Installed files, given by their entry path, that get a Start Menu shortcut in a folder named after the
    /// package: `/usr/bin/hi` gives `Start Menu\Programs\NAME\hi.lnk`. The name must then be a single folder name.
    #[serde(default)]
    shortcuts: Vec<String>,
    /// One command line that runs after the install, for example `bin\hi.exe --configure`. Its text is literal: `$` is
    /// not expanded. It runs in the install folder. A non-zero exit status stops the install with an error.
    post_install_exec: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Elevation {
    Admin,
    User,
}

impl Elevation {
    fn parse(options: &Options) -> Result<Self> {
        Ok(
            match options.execution_level.as_deref().unwrap_or("admin") {
                "admin" => Elevation::Admin,
                "user" => Elevation::User,
                other => bail!("execution_level {other:?} must be admin or user"),
            },
        )
    }

    fn level(self) -> &'static str {
        match self {
            Elevation::Admin => "admin",
            Elevation::User => "user",
        }
    }

    /// The `SetShellVarContext` value: `all` writes to the machine, `current` to the user.
    fn context(self) -> &'static str {
        match self {
            Elevation::Admin => "all",
            Elevation::User => "current",
        }
    }

    fn default_root(self) -> &'static str {
        match self {
            Elevation::Admin => "$PROGRAMFILES64",
            Elevation::User => "$LOCALAPPDATA\\Programs",
        }
    }
}

/// The installed files and the folders they need, from the manifest.
#[derive(Default)]
struct Layout {
    /// Folders below `install_dir`, each as its components. Sorted, so a parent comes before its children.
    dirs: BTreeSet<Vec<String>>,
    /// Installed files by their entry path on the target system, sorted by that path.
    files: BTreeMap<String, Installed>,
}

struct Installed {
    /// The components below `install_dir`, the last one the file name.
    parts: Vec<String>,
    /// The number of the copy of the bytes in the staging folder.
    payload: usize,
    config: bool,
}

/// The staging folder. It is removed when the build ends, on success or on failure.
struct Stage(PathBuf);

impl Stage {
    fn create(out: &Path) -> Result<Self> {
        let path = out.join(format!(".nsis-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("payload"))
            .with_context(|| format!("cannot create {}", path.display()))?;
        Ok(Stage(path))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SCRIPT: &str = "installer.nsi";
const SETUP: &str = "setup.exe";
const MISSING_MAKENSIS: &str = "makensis, the NSIS compiler, is not installed or not on PATH. Install NSIS: \
Debian or Ubuntu `apt install nsis`, Fedora `dnf install mingw32-nsis`, Arch `pacman -S nsis`, or get it from \
https://nsis.sourceforge.io";

fn compression(options: &Options) -> Result<&'static str> {
    Ok(match options.compression.as_deref().unwrap_or("lzma") {
        "lzma" => "lzma",
        "zlib" => "zlib",
        "bzip2" => "bzip2",
        other => bail!("compression {other:?} must be lzma, zlib or bzip2"),
    })
}

/// `name-version-release-arch.exe`.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    compression(&options)?;
    Elevation::parse(&options)?;
    let version = Version::parse(&package.version)?;
    if package.name.is_empty()
        || package.name.contains('/')
        || matches!(package.name.as_str(), "." | "..")
    {
        bail!("the package name {:?} cannot be a file name", package.name);
    }
    Ok(format!(
        "{}-{}-{}-{}.exe",
        package.name,
        version.plain(),
        package.release,
        package.arch
    ))
}

/// Whether a file name is one of the device names Windows reserves, with or without an extension.
fn is_reserved(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        other => other
            .strip_prefix("COM")
            .or_else(|| other.strip_prefix("LPT"))
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit()),
    }
}

/// One component of a Windows path: a file name Windows accepts.
fn check_component(part: &str) -> Result<()> {
    if part.is_empty() || part == "." || part == ".." {
        bail!("{part:?} is not a file name");
    }
    if let Some(bad) = part
        .chars()
        .find(|c| c.is_control() || "<>:\"/\\|?*".contains(*c))
    {
        bail!("{part:?} holds {bad:?}, which Windows does not allow in a file name");
    }
    if part.ends_with(['.', ' ']) {
        bail!("{part:?} ends in a dot or a space, which Windows does not allow in a file name");
    }
    if is_reserved(part) {
        bail!("{part:?} is a device name that Windows reserves");
    }
    if part.len() > 255 {
        bail!("{part:?} is longer than 255 bytes");
    }
    Ok(())
}

/// Text for an NSIS string in double quotes. A dollar sign and a quote are escaped, and so are tab and line breaks.
fn literal(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '$' => out.push_str("$$"),
            '"' => out.push_str("$\\\""),
            '\r' => out.push_str("$\\r"),
            '\n' => out.push_str("$\\n"),
            '\t' => out.push_str("$\\t"),
            c if c.is_control() => bail!(
                "{text:?} holds the control character {c:?}, which an installer string cannot hold"
            ),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Text for a version resource value (`VIAddVersionKey`), which the compiler reads at build time. There a dollar
/// sign is literal, so it is not doubled. Only `$\"`, `$\r`, `$\n` and `$\t` are escapes. `${` would be expanded
/// as a define, and `$\` cannot be spelled, so both are refused.
fn vi_literal(text: &str) -> Result<String> {
    if text.contains("${") {
        bail!("{text:?} holds ${{, which the version resource would expand as a define");
    }
    if text.contains("$\\") {
        bail!("{text:?} holds $\\, which a version resource cannot spell");
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("$\\\""),
            '\r' => out.push_str("$\\r"),
            '\n' => out.push_str("$\\n"),
            '\t' => out.push_str("$\\t"),
            c if c.is_control() => bail!(
                "{text:?} holds the control character {c:?}, which a version resource cannot hold"
            ),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// `install_dir` as NSIS text: a constant or a drive, then folders. Every folder is checked and escaped.
fn install_dir_expr(text: &str) -> Result<String> {
    let bytes = text.as_bytes();
    let (root, rest) = if let Some(after) = text.strip_prefix('$') {
        let len = after
            .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(after.len());
        if len == 0 {
            bail!(
                "install_dir {text:?}: the constant after $ is missing, for example $PROGRAMFILES64"
            );
        }
        (format!("${}", &after[..len]), &after[len..])
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        (text[..2].to_string(), &text[2..])
    } else {
        bail!(
            "install_dir {text:?} must start with an NSIS constant such as $PROGRAMFILES64, or with a drive such as C:"
        );
    };
    let Some(rest) = rest.strip_prefix('\\') else {
        bail!("install_dir {text:?} must name a folder below {root}, written with backslashes");
    };
    let mut parts = Vec::new();
    for part in rest.split('\\') {
        check_component(part).with_context(|| format!("install_dir {text:?}"))?;
        parts.push(literal(part)?);
    }
    Ok(format!("{root}\\{}", parts.join("\\")))
}

fn default_install_dir(package: &Package, elevation: Elevation) -> Result<String> {
    check_component(&package.name).context(
        "the default install_dir needs the package name as one folder name; set install_dir in the nsis section",
    )?;
    install_dir_expr(&format!("{}\\{}", elevation.default_root(), package.name))
}

fn prefix(options: &Options) -> Result<String> {
    let prefix = options.prefix.as_deref().unwrap_or("/usr");
    if !prefix.starts_with('/') {
        bail!("prefix {prefix:?} must be an absolute Unix path such as /usr");
    }
    Ok(prefix.trim_end_matches('/').to_string())
}

/// The version resource `a.b.c.d`: the three numbers of the version, then the release. A release that is not a number
/// from 0 to 65535 gives `.0`, with a warning.
fn version_resource(package: &Package, version: &Version, host: &Host) -> Result<String> {
    let mut parts = Vec::with_capacity(4);
    for part in version.core.split('.') {
        let number: u16 = part.parse().with_context(|| {
            format!(
                "the version {} has a part above 65535, but the version resource holds at most 65535 in each part",
                package.version
            )
        })?;
        parts.push(number);
    }
    let release = match package.release.parse::<u16>() {
        Ok(number) => number,
        Err(_) => {
            host.warn(&format!(
                "release {:?} is not a number from 0 to 65535, so the version resource ends in .0",
                package.release
            ));
            0
        }
    };
    parts.push(release);
    Ok(parts
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join("."))
}

/// The entry path of a file or folder, as the components below `prefix`.
fn components(path: &str, prefix: &str) -> Result<Vec<String>> {
    let rest = match path.strip_prefix(prefix) {
        Some(rest) if rest.starts_with('/') => &rest[1..],
        _ => bail!("{path} is outside the prefix {prefix:?}; set prefix so every file is under it"),
    };
    let mut parts = Vec::new();
    for part in rest.split('/') {
        check_component(part).with_context(|| format!("the entry {path}"))?;
        parts.push(part.to_string());
    }
    Ok(parts)
}

/// Adds the folder and every folder above it.
fn add_dirs(dirs: &mut BTreeSet<Vec<String>>, parts: &[String]) {
    for end in 1..=parts.len() {
        dirs.insert(parts[..end].to_vec());
    }
}

/// The folders above a file: every component but the last.
fn folders(parts: &[String]) -> &[String] {
    &parts[..parts.len().saturating_sub(1)]
}

/// The absolute path that a symbolic link at `link` points at, with `.` and `..` resolved.
fn resolve(link: &str, target: &str) -> String {
    let joined = if target.starts_with('/') {
        target.to_string()
    } else {
        let parent = link.rsplit_once('/').map_or("", |(parent, _)| parent);
        format!("{parent}/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

fn layout(package: &Package, prefix: &str, payload: &Path, host: &Host) -> Result<Layout> {
    let mut entries: Vec<&Entry> = package.entries.iter().collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut layout = Layout::default();
    let mut symlinks: Vec<(&Entry, &str)> = Vec::new();
    let mut copies = 0usize;
    for entry in entries {
        if entry.path == prefix && entry.is_dir() {
            continue;
        }
        match &entry.data {
            Data::Ghost => host.warn(&format!(
                "{} is owned by the package but not carried in it, so it is not installed",
                entry.path
            )),
            Data::Dir => {
                let parts = components(&entry.path, prefix)?;
                add_dirs(&mut layout.dirs, &parts);
            }
            Data::File { src, .. } => {
                let parts = components(&entry.path, prefix)?;
                add_dirs(&mut layout.dirs, folders(&parts));
                std::fs::copy(src, payload.join(copies.to_string()))
                    .with_context(|| format!("cannot copy {} into the installer", src.display()))?;
                layout.files.insert(
                    entry.path.clone(),
                    Installed {
                        parts,
                        payload: copies,
                        config: entry.config,
                    },
                );
                copies += 1;
            }
            Data::Symlink { target } => symlinks.push((entry, target)),
        }
    }
    // NSIS cannot make a symbolic link, so a link to an installed file is installed as a copy of that file.
    for (entry, target) in symlinks {
        let resolved = resolve(&entry.path, target);
        let Some(file) = layout.files.get(&resolved) else {
            host.warn(&format!(
                "{} links to {target}, which is not an installed file, so the link is skipped; NSIS cannot make links",
                entry.path
            ));
            continue;
        };
        let payload = file.payload;
        let parts = components(&entry.path, prefix)?;
        add_dirs(&mut layout.dirs, folders(&parts));
        host.warn(&format!(
            "{} is a symbolic link, which NSIS cannot make, so it is installed as a copy of {resolved}",
            entry.path
        ));
        layout.files.insert(
            entry.path.clone(),
            Installed {
                parts,
                payload,
                config: false,
            },
        );
    }
    Ok(layout)
}

/// A Windows path under `base` (NSIS text already) with each component escaped.
fn win_path(base: &str, parts: &[String]) -> Result<String> {
    let mut out = base.to_string();
    for part in parts {
        out.push('\\');
        out.push_str(&literal(part)?);
    }
    Ok(out)
}

/// The uninstaller's file path, for `WriteUninstaller` and `Delete`.
const UNINSTALLER: &str = "$INSTDIR\\uninstall.exe";

fn script(
    package: &Package,
    options: &Options,
    elevation: Elevation,
    install_dir: &str,
    layout: &Layout,
    resource: &str,
) -> Result<String> {
    let name = literal(&package.name)?;
    let summary = match package.summary() {
        "" => package.name.as_str(),
        summary => summary,
    };
    let key = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}");
    // A registry value holds the command line, so the path inside it is quoted with the escape `$\"`.
    let uninstall_command = format!("$\\\"{UNINSTALLER}$\\\"");
    let mut lines: Vec<String> = vec![
        "Unicode true".into(),
        format!("Name \"{name}\""),
        format!("OutFile \"{SETUP}\""),
        format!("SetCompressor /SOLID {}", compression(options)?),
        format!("RequestExecutionLevel {}", elevation.level()),
        format!("InstallDir \"{install_dir}\""),
        format!("VIProductVersion \"{resource}\""),
        format!(
            "VIAddVersionKey \"ProductName\" \"{}\"",
            vi_literal(&package.name)?
        ),
        format!(
            "VIAddVersionKey \"FileDescription\" \"{}\"",
            vi_literal(summary)?
        ),
        format!("VIAddVersionKey \"FileVersion\" \"{resource}\""),
        format!(
            "VIAddVersionKey \"ProductVersion\" \"{}\"",
            vi_literal(&package.version)?
        ),
        String::new(),
        "Function un.onInit".into(),
        format!("  SetShellVarContext {}", elevation.context()),
        "FunctionEnd".into(),
        String::new(),
        "Section \"Install\"".into(),
        format!("  SetShellVarContext {}", elevation.context()),
        "  CreateDirectory \"$INSTDIR\"".into(),
    ];
    for dir in &layout.dirs {
        lines.push(format!(
            "  CreateDirectory \"{}\"",
            win_path("$INSTDIR", dir)?
        ));
    }

    let mut current: Option<&[String]> = None;
    for installed in layout.files.values() {
        let (file, folder) = installed
            .parts
            .split_last()
            .context("an installed file has no name")?;
        if current != Some(folder) {
            lines.push(format!(
                "  SetOutPath \"{}\"",
                win_path("$INSTDIR", folder)?
            ));
            current = Some(folder);
        }
        let copy = format!(
            "File \"/oname={}\" \"payload/{}\"",
            literal(file)?,
            installed.payload
        );
        if installed.config {
            // A configuration file the user may have changed: copy it only when there is none.
            lines.push(format!(
                "  IfFileExists \"{}\" +2 0",
                win_path("$INSTDIR", &installed.parts)?
            ));
            lines.push(format!("    {copy}"));
        } else {
            lines.push(format!("  {copy}"));
        }
    }

    lines.push(format!("  WriteUninstaller \"{UNINSTALLER}\""));

    let mut links = Vec::new();
    if !options.shortcuts.is_empty() {
        check_component(&package.name).context(
            "shortcuts need the package name as one folder name in the Start Menu; rename the package or drop shortcuts",
        )?;
        let folder = format!("$SMPROGRAMS\\{name}");
        lines.push(format!("  CreateDirectory \"{folder}\""));
        for shortcut in &options.shortcuts {
            let installed = layout.files.get(shortcut).with_context(|| {
                format!("shortcut {shortcut} is not an installed file under the prefix")
            })?;
            let file = installed.parts.last().map_or("", String::as_str);
            let stem = file
                .rsplit_once('.')
                .filter(|(stem, _)| !stem.is_empty())
                .map_or(file, |(stem, _)| stem);
            let link = format!("{folder}\\{}.lnk", literal(stem)?);
            lines.push(format!(
                "  CreateShortCut \"{link}\" \"{}\"",
                win_path("$INSTDIR", &installed.parts)?
            ));
            links.push(link);
        }
    }

    lines.push(format!(
        "  WriteRegStr SHCTX \"{key}\" \"DisplayName\" \"{name}\""
    ));
    lines.push(format!(
        "  WriteRegStr SHCTX \"{key}\" \"DisplayVersion\" \"{}\"",
        literal(&package.version)?
    ));
    if let Some(maintainer) = &package.maintainer {
        lines.push(format!(
            "  WriteRegStr SHCTX \"{key}\" \"Publisher\" \"{}\"",
            literal(maintainer)?
        ));
    }
    if let Some(homepage) = &package.homepage {
        lines.push(format!(
            "  WriteRegStr SHCTX \"{key}\" \"URLInfoAbout\" \"{}\"",
            literal(homepage)?
        ));
    }
    lines.push(format!(
        "  WriteRegStr SHCTX \"{key}\" \"InstallLocation\" \"$INSTDIR\""
    ));
    lines.push(format!(
        "  WriteRegStr SHCTX \"{key}\" \"UninstallString\" \"{uninstall_command}\""
    ));
    lines.push(format!(
        "  WriteRegStr SHCTX \"{key}\" \"QuietUninstallString\" \"{uninstall_command} /S\""
    ));
    lines.push(format!("  WriteRegDWORD SHCTX \"{key}\" \"NoModify\" 1"));
    lines.push(format!("  WriteRegDWORD SHCTX \"{key}\" \"NoRepair\" 1"));

    if let Some(command) = &options.post_install_exec {
        lines.push("  SetOutPath \"$INSTDIR\"".into());
        lines.push(format!("  ExecWait \"{}\" $0", literal(command)?));
        // A failed command must not leave a half-installed program. The uninstaller removes the files and the registry
        // key. `_?=` keeps it in place and makes ExecWait wait for it, so it can be deleted afterwards.
        lines.push("  StrCmp $0 0 done 0".into());
        lines.push(format!("  ExecWait '\"{UNINSTALLER}\" /S _?=$INSTDIR'"));
        lines.push(format!("  Delete \"{UNINSTALLER}\""));
        // The installer's working folder is $INSTDIR, and Windows will not remove a folder that is in use.
        lines.push("  SetOutPath \"$TEMP\"".into());
        lines.push("  RMDir \"$INSTDIR\"".into());
        lines.push("  Abort \"post_install_exec failed, so the install was rolled back\"".into());
        lines.push("  done:".into());
    }
    lines.push("SectionEnd".into());
    lines.push(String::new());

    lines.push("Section \"Uninstall\"".into());
    for installed in layout.files.values().filter(|f| !f.config) {
        lines.push(format!(
            "  Delete \"{}\"",
            win_path("$INSTDIR", &installed.parts)?
        ));
    }
    lines.push(format!("  Delete \"{UNINSTALLER}\""));
    for dir in layout.dirs.iter().rev() {
        lines.push(format!("  RMDir \"{}\"", win_path("$INSTDIR", dir)?));
    }
    lines.push("  RMDir \"$INSTDIR\"".into());
    for link in &links {
        lines.push(format!("  Delete \"{link}\""));
    }
    if !links.is_empty() {
        lines.push(format!("  RMDir \"$SMPROGRAMS\\{name}\""));
    }
    lines.push(format!("  DeleteRegKey SHCTX \"{key}\""));
    lines.push("SectionEnd".into());
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn check_makensis() -> Result<()> {
    match Command::new("makensis")
        .arg("-VERSION")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => bail!("makensis -VERSION failed with {status}"),
        Err(err) if err.kind() == ErrorKind::NotFound => bail!("{MISSING_MAKENSIS}"),
        Err(err) => Err(err).context("cannot run makensis"),
    }
}

fn run_makensis(stage: &Path) -> Result<()> {
    let output = Command::new("makensis")
        .arg(SCRIPT)
        .current_dir(stage)
        .stdin(Stdio::null())
        .output()
        .context("cannot run makensis")?;
    if !output.status.success() {
        bail!(
            "makensis failed on the generated script:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn warn_unsupported(package: &Package, host: &Host) {
    let scripts = [
        ("pre_install", &package.scripts.pre_install),
        ("post_install", &package.scripts.post_install),
        ("pre_remove", &package.scripts.pre_remove),
        ("post_remove", &package.scripts.post_remove),
    ];
    let present: Vec<&str> = scripts
        .iter()
        .filter(|(_, script)| script.is_some())
        .map(|(name, _)| *name)
        .collect();
    if !present.is_empty() {
        host.warn(&format!(
            "NSIS cannot run the shell scripts {}, so they are ignored; post_install_exec runs one command after the install",
            present.join(", ")
        ));
    }
    let relations = [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, values) in relations {
        if !values.is_empty() {
            host.warn(&format!(
                "NSIS installers do not record {field}, so {field} is ignored"
            ));
        }
    }
    if package.license.is_some() {
        host.warn("NSIS installers do not record the license, so license is ignored");
    }
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    compression(&options)?;
    let elevation = Elevation::parse(&options)?;
    let version = Version::parse(&package.version)?;
    let resource = version_resource(package, &version, host)?;
    let prefix = prefix(&options)?;
    let install_dir = match &options.install_dir {
        Some(dir) => install_dir_expr(dir)?,
        None => default_install_dir(package, elevation)?,
    };
    if let Some(command) = &options.post_install_exec
        && command.trim().is_empty()
    {
        bail!("post_install_exec is empty; remove it or give a command");
    }
    warn_unsupported(package, host);
    if !package.entries.is_empty() {
        host.info("NSIS installers keep no file modes or owners, so the mode and owner of each file are ignored");
    }
    check_makensis()?;

    let name = file_name(package)?;
    let stage = Stage::create(&package.out)?;
    let layout = layout(package, &prefix, &stage.0.join("payload"), host)?;
    let script = script(
        package,
        &options,
        elevation,
        &install_dir,
        &layout,
        &resource,
    )?;
    // The script starts with a byte order mark, so makensis reads its UTF-8 text (any names) correctly.
    std::fs::write(stage.0.join(SCRIPT), format!("\u{feff}{script}"))
        .context("cannot write the NSIS script")?;
    run_makensis(&stage.0)?;
    let file = package.output(&name);
    std::fs::rename(stage.0.join(SETUP), &file)
        .with_context(|| format!("cannot move the installer to {}", file.display()))?;
    Ok(file)
}
