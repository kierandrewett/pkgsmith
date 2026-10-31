//! Plugins: the shared libraries that write the package formats.
//!
//! The host finds files named `pkgsmith-plugin-*.so` (cargo names them `libpkgsmith_plugin_*.so`, which is accepted
//! too) in the plugin path, loads them, and asks each one which formats it writes. See `pkgsmith_plugin::abi` for the
//! interface.

use anyhow::{Context, Result, anyhow, bail};
use libloading::Library;
use pkgsmith_plugin::abi::{ABI_VERSION, Buf, ENTRY_SYMBOL, EntryFn, HostVTable, PluginVTable};
use pkgsmith_plugin::{Package, PluginInfo};
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

/// A format and the plugin that writes it.
#[derive(Debug, Clone)]
pub struct FormatEntry {
    pub name: String,
    pub description: String,
    pub plugin: String,
    pub path: PathBuf,
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
/// 2. `PKGSMITH_PLUGIN_PATH`, a list separated by `:`.
/// 3. The folder of the `pkgsmith` binary, which is where `cargo build` puts the plugins of this repository.
/// 4. `lib/pkgsmith/plugins` in the folder above the binary (`/usr/lib/pkgsmith/plugins` for `/usr/bin/pkgsmith`), which finds the plugins of a package installed under any prefix.
/// 5. `$XDG_DATA_HOME/pkgsmith/plugins`, by default `~/.local/share/pkgsmith/plugins`.
/// 6. `/usr/local/lib/pkgsmith/plugins`, `/usr/lib/pkgsmith/plugins` and `/usr/lib64/pkgsmith/plugins`.
pub fn search_path(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut path: Vec<PathBuf> = extra.to_vec();
    if let Some(list) = std::env::var_os("PKGSMITH_PLUGIN_PATH") {
        path.extend(std::env::split_paths(&list).filter(|p| !p.as_os_str().is_empty()));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        path.push(dir.to_path_buf());
        if let Some(prefix) = dir.parent() {
            path.push(prefix.join("lib/pkgsmith/plugins"));
        }
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(data) = data {
        path.push(data.join("pkgsmith/plugins"));
    }
    for dir in ["/usr/local/lib", "/usr/lib", "/usr/lib64"] {
        path.push(Path::new(dir).join("pkgsmith/plugins"));
    }
    path
}

/// Whether a file name is a plugin: `pkgsmith-plugin-NAME.so` or `libpkgsmith_plugin_NAME.so`.
fn is_plugin_file(name: &str) -> bool {
    let stem = name.strip_prefix("lib").unwrap_or(name);
    name.ends_with(".so")
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
                    });
                }
            }
        }
        out
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
    ) -> Result<serde_json::Value> {
        let Some(&index) = self.formats.get(&package.format) else {
            bail!(
                "no plugin writes the format {:?}; installed: {}. Plugins are searched for in the folders `pkgsmith plugins` lists",
                package.format,
                self.format_names().join(", ")
            );
        };
        let plugin = &self.plugins[index];
        let request = serde_json::to_vec(package)?;
        let context: &dyn Fn(u32, &str) = log;
        let host = HostVTable {
            abi_version: ABI_VERSION,
            ctx: &context as *const &dyn Fn(u32, &str) as *mut c_void,
            log: Some(trampoline),
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

    /// The name of the file `build` would write. `package.entries` may be empty.
    pub fn file_name(&self, package: &Package) -> Result<String> {
        match self.call("file_name", package, &|_, _| {})? {
            serde_json::Value::String(name) => Ok(name),
            other => bail!("the plugin answered {other} instead of a file name"),
        }
    }

    /// Write the package and return the path of the file. The output folder exists when the plugin runs. `log` receives the messages of the plugin: a level (0 error,
    /// 1 warning, 2 information) and the text.
    pub fn build(&self, package: &Package, log: &dyn Fn(u32, &str)) -> Result<PathBuf> {
        std::fs::create_dir_all(&package.out)
            .with_context(|| format!("cannot create {}", package.out.display()))?;
        match self.call("build", package, log)? {
            serde_json::Value::String(path) => Ok(PathBuf::from(path)),
            other => bail!("the plugin answered {other} instead of a path"),
        }
    }
}

unsafe extern "C" fn trampoline(ctx: *mut c_void, level: u32, message: *const u8, len: usize) {
    // SAFETY: `ctx` is the `&&dyn Fn` that `Registry::call` built, and `message` is valid for `len` bytes.
    let (log, message) = unsafe {
        (
            &*(ctx as *const &dyn Fn(u32, &str)),
            std::slice::from_raw_parts(message, len),
        )
    };
    log(level, &String::from_utf8_lossy(message));
}
