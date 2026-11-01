//! Plugins: the shared libraries that write the package formats.
//!
//! The host finds files named `pkgsmith-plugin-*.so` (cargo names them `libpkgsmith_plugin_*.so`, which is accepted
//! too) in the plugin path, loads them, and asks each one which formats it writes. See `pkgsmith_plugin::abi` for the
//! interface.

use anyhow::{Context, Result, anyhow, bail};
use libloading::Library;
use pkgsmith_plugin::abi::{ABI_VERSION, Buf, ENTRY_SYMBOL, EntryFn, HostVTable, PluginVTable};
use pkgsmith_plugin::{Package, PluginInfo, Scheme, SignRequest, SignerInfo, Signing};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

/// A loaded plugin. The library stays loaded as long as the registry lives, because the table of function pointers
/// points into it.
struct Loaded {
    path: PathBuf,
    info: PluginInfo,
    table: &'static PluginVTable,
    _library: Library,
}

// SAFETY: the table holds plain function pointers into a library that stays loaded. The interface requires a plugin
// to accept calls from any thread.
unsafe impl Send for Loaded {}
unsafe impl Sync for Loaded {}

/// What a plugin may ask of the host while it builds: signatures, made with keys the host holds.
pub trait Signer: Sync {
    fn can_sign(&self, format: &str, scheme: Scheme) -> bool;
    fn sign(&self, format: &str, scheme: Scheme, data: &[u8]) -> Result<Vec<u8>>;
    fn key_name(&self, format: &str, scheme: Scheme) -> Option<String>;
}

/// What the trampolines of one call can reach.
struct CallContext<'a> {
    log: &'a dyn Fn(u32, &str),
    signer: Option<&'a dyn Signer>,
    format: &'a str,
}

/// A format and the plugin that writes it.
#[derive(Debug, Clone)]
pub struct FormatEntry {
    pub name: String,
    pub description: String,
    pub plugin: String,
    pub path: PathBuf,
    pub signing: Signing,
}

pub struct Registry {
    plugins: Vec<Loaded>,
    /// Format name to the index of the plugin that writes it.
    formats: BTreeMap<String, usize>,
    /// Plugins that could not be loaded, and formats that two plugins offer.
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Reply {
    Ok(serde_json::Value),
    Error(String),
}

/// The folders searched for plugins, in order. The first plugin that offers a format wins.
///
/// 1. `extra`, from `--plugin-path`.
/// 2. `PKGSMITH_PLUGIN_PATH`, a list of folders separated by `:` (`;` on Windows).
/// 3. The folder of the `pkgsmith` binary, which is where `cargo build` puts the plugins of this repository.
/// 4. `lib/pkgsmith/plugins` in the folder above the binary (`/usr/lib/pkgsmith/plugins` for `/usr/bin/pkgsmith`), which finds the plugins of a package installed under any prefix.
/// 5. The data folder of the user: `$XDG_DATA_HOME/pkgsmith/plugins` or `~/.local/share/pkgsmith/plugins` on Linux,
///    `~/Library/Application Support/pkgsmith/plugins` on macOS, `%LOCALAPPDATA%\\pkgsmith\\plugins` on Windows.
/// 6. The system folders: `/usr/local/lib`, `/usr/lib` and `/usr/lib64` (all with `/pkgsmith/plugins`) on Linux,
///    `/usr/local/lib` and `/opt/homebrew/lib` on macOS, `%ProgramData%\\pkgsmith\\plugins` on Windows.
pub fn search_path(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut path: Vec<PathBuf> = extra.to_vec();
    if let Some(list) = std::env::var_os("PKGSMITH_PLUGIN_PATH") {
        path.extend(std::env::split_paths(&list).filter(|p| !p.as_os_str().is_empty()));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        path.push(dir.to_path_buf());
        // The tarball of a release holds the program and a `plugins` folder.
        path.push(dir.join("plugins"));
        if let Some(prefix) = dir.parent() {
            path.push(prefix.join("lib/pkgsmith/plugins"));
        }
    }
    if let Some(data) = user_data_dir() {
        path.push(data.join("pkgsmith/plugins"));
    }
    for dir in system_dirs() {
        path.push(dir);
    }
    path
}

/// The data folder of the user: the folder that holds `pkgsmith/plugins`.
pub fn user_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

/// Folders for all users, where a package manager puts plugins.
fn system_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("ProgramData")
            .map(|p| vec![PathBuf::from(p).join("pkgsmith/plugins")])
            .unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        ["/usr/local/lib", "/opt/homebrew/lib"]
            .iter()
            .map(|d| Path::new(d).join("pkgsmith/plugins"))
            .collect()
    } else {
        ["/usr/local/lib", "/usr/lib", "/usr/lib64"]
            .iter()
            .map(|d| Path::new(d).join("pkgsmith/plugins"))
            .collect()
    }
}

/// The suffix of a shared library on this system: `.so`, `.dylib` or `.dll`.
pub const LIBRARY_SUFFIX: &str = std::env::consts::DLL_SUFFIX;

/// Whether a file name is a plugin: `pkgsmith-plugin-NAME` or the name that cargo gives a library (`libpkgsmith_plugin_NAME`
/// on Linux and macOS, `pkgsmith_plugin_NAME` on Windows), with the suffix of a shared library on this system.
fn is_plugin_file(name: &str) -> bool {
    let stem = name.strip_prefix("lib").unwrap_or(name);
    name.ends_with(LIBRARY_SUFFIX)
        && (stem.starts_with("pkgsmith-plugin-") || stem.starts_with("pkgsmith_plugin_"))
}

fn open(path: &Path) -> Result<Loaded> {
    // SAFETY: loading a library runs its initialisers. A plugin is code the user chose to install, like any program.
    let library =
        unsafe { Library::new(path) }.with_context(|| format!("cannot load {}", path.display()))?;
    // SAFETY: the symbol has the type that `pkgsmith_plugin::abi` documents.
    let entry: EntryFn = *unsafe { library.get::<EntryFn>(ENTRY_SYMBOL) }.map_err(|_| {
        anyhow!(
            "{} is not a pkgsmith plugin: it has no pkgsmith_plugin_entry",
            path.display()
        )
    })?;
    // SAFETY: the table is static data of the library, which `_library` keeps loaded.
    let table: &'static PluginVTable =
        unsafe { entry().as_ref() }.ok_or_else(|| anyhow!("the plugin returned no table"))?;
    if table.abi_version != ABI_VERSION {
        bail!(
            "{} speaks plugin interface {} and this pkgsmith speaks {ABI_VERSION}",
            path.display(),
            table.abi_version
        );
    }
    // SAFETY: `describe` returns a buffer that `free_buf` releases.
    let info = unsafe {
        let buf = (table.describe)();
        let info = serde_json::from_slice::<PluginInfo>(buf.as_slice());
        (table.free_buf)(buf);
        info
    }
    .with_context(|| format!("{} gave a description that is not valid", path.display()))?;
    Ok(Loaded {
        path: path.to_path_buf(),
        info,
        table,
        _library: library,
    })
}

impl Registry {
    /// Load every plugin on the search path. A plugin that fails to load is skipped with a warning.
    pub fn discover(extra: &[PathBuf]) -> Registry {
        let mut registry = Registry {
            plugins: Vec::new(),
            formats: BTreeMap::new(),
            warnings: Vec::new(),
        };
        let mut seen: Vec<PathBuf> = Vec::new();
        for dir in search_path(extra) {
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut files: Vec<PathBuf> = read
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(is_plugin_file)
                })
                .collect();
            files.sort();
            for file in files {
                let canonical = std::fs::canonicalize(&file).unwrap_or_else(|_| file.clone());
                if seen.contains(&canonical) {
                    continue;
                }
                seen.push(canonical);
                match open(&file) {
                    Ok(loaded) => registry.add(loaded),
                    Err(error) => registry.warnings.push(format!("{error:#}")),
                }
            }
        }
        registry
    }

    fn add(&mut self, loaded: Loaded) {
        let index = self.plugins.len();
        for format in &loaded.info.formats {
            if let Some(&first) = self.formats.get(&format.name) {
                // The same library installed in two places is not a conflict. A different one is.
                let same = std::fs::read(&self.plugins[first].path).ok()
                    == std::fs::read(&loaded.path).ok();
                if !same {
                    self.warnings.push(format!(
                        "{} offers the format {:?}, which {} already provides. The first one is used",
                        loaded.path.display(),
                        format.name,
                        self.plugins[first].path.display()
                    ));
                }
            } else {
                self.formats.insert(format.name.clone(), index);
            }
        }
        self.plugins.push(loaded);
    }

    /// The formats, in the order of the plugins and then the order each plugin lists them.
    pub fn formats(&self) -> Vec<FormatEntry> {
        let mut out = Vec::new();
        for (index, plugin) in self.plugins.iter().enumerate() {
            for format in &plugin.info.formats {
                if self.formats.get(&format.name) == Some(&index) {
                    out.push(FormatEntry {
                        name: format.name.clone(),
                        description: format.description.clone(),
                        plugin: plugin.info.name.clone(),
                        path: plugin.path.clone(),
                        signing: format.signing,
                    });
                }
            }
        }
        out
    }

    /// How the format is signed, or `None` for a format no plugin writes.
    pub fn signing(&self, format: &str) -> Option<Signing> {
        self.formats()
            .into_iter()
            .find(|f| f.name == format)
            .map(|f| f.signing)
    }

    pub fn format_names(&self) -> Vec<String> {
        self.formats().into_iter().map(|f| f.name).collect()
    }

    /// `(name, version, path, formats)` of each loaded plugin.
    pub fn plugins(&self) -> impl Iterator<Item = (&PluginInfo, &Path)> {
        self.plugins.iter().map(|p| (&p.info, p.path.as_path()))
    }

    fn call(
        &self,
        method: &str,
        package: &Package,
        log: &dyn Fn(u32, &str),
        signer: Option<&dyn Signer>,
    ) -> Result<serde_json::Value> {
        let Some(&index) = self.formats.get(&package.format) else {
            bail!(
                "no plugin writes the format {:?}; installed: {}. Plugins are searched for in the folders `pkgsmith plugins` lists",
                package.format,
                self.format_names().join(", ")
            );
        };
        self.invoke(
            index,
            method,
            serde_json::to_vec(package)?,
            &package.format,
            log,
            signer,
        )
    }

    /// Send one request to one plugin and read its answer.
    fn invoke(
        &self,
        index: usize,
        method: &str,
        request: Vec<u8>,
        format: &str,
        log: &dyn Fn(u32, &str),
        signer: Option<&dyn Signer>,
    ) -> Result<serde_json::Value> {
        let plugin = &self.plugins[index];
        let context = CallContext {
            log,
            signer,
            format,
        };
        let host = HostVTable {
            abi_version: ABI_VERSION,
            ctx: &context as *const CallContext as *mut c_void,
            log: Some(log_trampoline),
            can_sign: Some(can_sign_trampoline),
            sign: Some(sign_trampoline),
            key_name: Some(key_name_trampoline),
            free_buf: Some(free_trampoline),
        };
        // SAFETY: the pointers are valid for the call, and the reply is freed by the plugin that made it.
        let reply = unsafe {
            let buf: Buf = (plugin.table.call)(
                method.as_ptr(),
                method.len(),
                request.as_ptr(),
                request.len(),
                &host,
            );
            let reply = serde_json::from_slice::<Reply>(buf.as_slice());
            (plugin.table.free_buf)(buf);
            reply
        }
        .with_context(|| format!("{} gave an answer that is not valid", plugin.path.display()))?;
        match reply {
            Reply::Ok(value) => Ok(value),
            Reply::Error(text) => bail!("{text}"),
        }
    }

    /// Every signer of every loaded plugin, with the plugin that offers it.
    pub fn signers(&self) -> Vec<(&SignerInfo, &PluginInfo)> {
        self.plugins
            .iter()
            .flat_map(|p| p.info.signers.iter().map(move |s| (s, &p.info)))
            .collect()
    }

    /// Ask the signers, in plugin order, which one takes this request. A signer that the user named is the only one
    /// asked. `Ok(None)` means that no signer claims the key. An `Err` is a signer that claims the key and says why it
    /// cannot use it, for example a wrong passphrase. The error is returned at once: no other signer is tried.
    pub fn choose_signer(&self, request: &SignRequest, named: bool) -> Result<Option<String>> {
        for (index, plugin) in self.plugins.iter().enumerate() {
            for signer in &plugin.info.signers {
                if !signer.schemes.contains(&request.scheme)
                    || (named && signer.name != request.signer)
                {
                    continue;
                }
                let mut ask = request.clone();
                ask.signer = signer.name.clone();
                let answer = self
                    .invoke(
                        index,
                        "accepts",
                        serde_json::to_vec(&ask)?,
                        "",
                        &|_, _| {},
                        None,
                    )
                    .with_context(|| format!("the {} signer", signer.name))?;
                if answer == serde_json::Value::Bool(true) {
                    return Ok(Some(signer.name.clone()));
                }
            }
        }
        Ok(None)
    }

    /// Sign with the signer that `choose_signer` found.
    pub fn run_signer(&self, request: &SignRequest) -> Result<()> {
        let index = self
            .plugins
            .iter()
            .position(|p| p.info.signers.iter().any(|s| s.name == request.signer))
            .ok_or_else(|| anyhow::anyhow!("no signer named {:?}", request.signer))?;
        self.invoke(
            index,
            "sign",
            serde_json::to_vec(request)?,
            "",
            &|_, _| {},
            None,
        )
        .with_context(|| format!("the {} signer", request.signer))?;
        Ok(())
    }

    /// The name of the file `build` would write. `package.entries` may be empty.
    pub fn file_name(&self, package: &Package) -> Result<String> {
        match self.call("file_name", package, &|_, _| {}, None)? {
            serde_json::Value::String(name) => Ok(name),
            other => bail!("the plugin answered {other} instead of a file name"),
        }
    }

    /// Write the package and return the path of the file. The output folder exists when the plugin runs. `log` receives the messages of the plugin: a level (0 error,
    /// 1 warning, 2 information) and the text.
    pub fn build(
        &self,
        package: &Package,
        log: &dyn Fn(u32, &str),
        signer: Option<&dyn Signer>,
    ) -> Result<PathBuf> {
        std::fs::create_dir_all(&package.out)
            .with_context(|| format!("cannot create {}", package.out.display()))?;
        match self.call("build", package, log, signer)? {
            serde_json::Value::String(path) => Ok(PathBuf::from(path)),
            other => bail!("the plugin answered {other} instead of a path"),
        }
    }
}

unsafe fn context<'a>(ctx: *mut c_void) -> &'a CallContext<'a> {
    // SAFETY: `ctx` is the `CallContext` that `Registry::call` built, alive for the whole call.
    unsafe { &*(ctx as *const CallContext) }
}

unsafe extern "C" fn log_trampoline(ctx: *mut c_void, level: u32, message: *const u8, len: usize) {
    // SAFETY: `message` is valid for `len` bytes, as the interface requires.
    let message = unsafe { std::slice::from_raw_parts(message, len) };
    (unsafe { context(ctx) }.log)(level, &String::from_utf8_lossy(message));
}

unsafe extern "C" fn can_sign_trampoline(ctx: *mut c_void, scheme: u32) -> u32 {
    let context = unsafe { context(ctx) };
    let (Some(scheme), Some(signer)) = (Scheme::from_number(scheme), context.signer) else {
        return 0;
    };
    u32::from(signer.can_sign(context.format, scheme))
}

unsafe extern "C" fn sign_trampoline(
    ctx: *mut c_void,
    scheme: u32,
    data: *const u8,
    len: usize,
    out: *mut Buf,
) -> i32 {
    let context = unsafe { context(ctx) };
    // SAFETY: `data` is valid for `len` bytes and `out` is a valid pointer, as the interface requires.
    let (data, out) = unsafe { (std::slice::from_raw_parts(data, len), &mut *out) };
    let result = match (Scheme::from_number(scheme), context.signer) {
        (Some(scheme), Some(signer)) => signer.sign(context.format, scheme, data),
        (None, _) => Err(anyhow!("unknown signature scheme {scheme}")),
        (_, None) => Err(anyhow!("no signing key was given; use --sign-key")),
    };
    match result {
        Ok(bytes) => {
            *out = Buf::from_vec(bytes);
            0
        }
        Err(error) => {
            *out = Buf::from_vec(format!("{error:#}").into_bytes());
            1
        }
    }
}

unsafe extern "C" fn key_name_trampoline(ctx: *mut c_void, scheme: u32, out: *mut Buf) -> i32 {
    let context = unsafe { context(ctx) };
    // SAFETY: `out` is a valid pointer, as the interface requires.
    let out = unsafe { &mut *out };
    let name = Scheme::from_number(scheme)
        .zip(context.signer)
        .and_then(|(scheme, signer)| signer.key_name(context.format, scheme));
    match name {
        Some(name) => {
            *out = Buf::from_vec(name.into_bytes());
            0
        }
        None => 1,
    }
}

unsafe extern "C" fn free_trampoline(buf: Buf) {
    // SAFETY: the buffer came from `Buf::from_vec` in this process.
    unsafe { buf.free() };
}
