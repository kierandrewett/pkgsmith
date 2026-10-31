//! The Rust side of writing a plugin: implement [`Plugin`] and call [`export_plugin!`](crate::export_plugin).

use crate::abi::{Buf, HostVTable};
use crate::package::Package;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One format a plugin writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatInfo {
    /// The name in `format:` of a matrix entry and on the command line, for example `deb`.
    pub name: String,
    pub description: String,
}

/// What `describe` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub formats: Vec<FormatInfo>,
}

/// The host, for messages that belong on the screen of the user.
pub struct Host<'a> {
    raw: Option<&'a HostVTable>,
}

#[derive(Clone, Copy)]
pub enum Level {
    Error = 0,
    Warning = 1,
    Info = 2,
}

impl Host<'_> {
    pub fn log(&self, level: Level, message: &str) {
        if let Some(raw) = self.raw
            && let Some(log) = raw.log
        {
            // SAFETY: the host promises that `ctx` and `log` stay valid for the call.
            unsafe { log(raw.ctx, level as u32, message.as_ptr(), message.len()) };
        }
    }

    pub fn warn(&self, message: &str) {
        self.log(Level::Warning, message);
    }

    pub fn info(&self, message: &str) {
        self.log(Level::Info, message);
    }
}

pub trait Plugin: Send + Sync {
    fn info(&self) -> PluginInfo;

    /// The name of the file `build` writes, from the package alone. `package.entries` is empty.
    fn file_name(&self, package: &Package) -> Result<String>;

    /// Write the package into `package.out` and return the path of the file.
    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf>;
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Reply<T> {
    Ok(T),
    Error(String),
}

fn reply<T: Serialize>(result: Result<T>) -> Buf {
    let reply = match result {
        Ok(value) => Reply::Ok(value),
        Err(error) => Reply::Error(format!("{error:#}")),
    };
    Buf::from_vec(
        serde_json::to_vec(&reply)
            .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}").into_bytes()),
    )
}

/// Answer `describe`. Used by [`export_plugin!`](crate::export_plugin).
pub fn describe(plugin: &dyn Plugin) -> Buf {
    Buf::from_vec(serde_json::to_vec(&plugin.info()).unwrap_or_default())
}

/// Answer `call`. A panic in the plugin becomes an error reply, so it does not unwind into the host. Used by
/// [`export_plugin!`](crate::export_plugin).
///
/// # Safety
/// The pointers must be valid for the lengths given, as the C interface requires.
pub unsafe fn call(
    plugin: &dyn Plugin,
    method: *const u8,
    method_len: usize,
    request: *const u8,
    request_len: usize,
    host: *const HostVTable,
) -> Buf {
    // SAFETY: the caller upholds the contract of the C interface.
    let (method, request, host) = unsafe {
        (
            std::slice::from_raw_parts(method, method_len),
            std::slice::from_raw_parts(request, request_len),
            host.as_ref(),
        )
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Buf {
        let method = match std::str::from_utf8(method) {
            Ok(method) => method,
            Err(_) => return reply::<()>(Err(anyhow::anyhow!("the method name is not UTF-8"))),
        };
        let package: Result<Package> =
            serde_json::from_slice(request).context("the request is not a package");
        let host = Host { raw: host };
        match (method, package) {
            (_, Err(error)) => reply::<()>(Err(error)),
            ("file_name", Ok(package)) => reply(plugin.file_name(&package)),
            ("build", Ok(package)) => reply(
                plugin
                    .build(&package, &host)
                    .map(|p| p.display().to_string()),
            ),
            (other, Ok(_)) => reply::<()>(Err(anyhow::anyhow!("unknown method {other:?}"))),
        }
    }));
    outcome.unwrap_or_else(|_| reply::<()>(Err(anyhow::anyhow!("the plugin panicked"))))
}

/// Check that the format of a package is one the plugin lists. A helper for `Plugin` implementations.
pub fn expect_format<'a>(package: &'a Package, formats: &[&str]) -> Result<&'a str> {
    if formats.contains(&package.format.as_str()) {
        Ok(&package.format)
    } else {
        bail!(
            "this plugin does not write {:?}; it writes {}",
            package.format,
            formats.join(", ")
        )
    }
}

/// Export a [`Plugin`] from a `cdylib`. The argument is an expression that makes the plugin.
///
/// ```ignore
/// pkgsmith_plugin::export_plugin!(MyFormat);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($make:expr) => {
        mod __pkgsmith_export {
            use super::*;
            use ::pkgsmith_plugin::abi::{ABI_VERSION, Buf, HostVTable, PluginVTable};
            use ::pkgsmith_plugin::plugin::{self, Plugin};

            fn instance() -> &'static dyn Plugin {
                static INSTANCE: ::std::sync::OnceLock<Box<dyn Plugin>> =
                    ::std::sync::OnceLock::new();
                INSTANCE.get_or_init(|| Box::new($make)).as_ref()
            }

            unsafe extern "C" fn describe() -> Buf {
                plugin::describe(instance())
            }

            unsafe extern "C" fn call(
                method: *const u8,
                method_len: usize,
                request: *const u8,
                request_len: usize,
                host: *const HostVTable,
            ) -> Buf {
                unsafe { plugin::call(instance(), method, method_len, request, request_len, host) }
            }

            unsafe extern "C" fn free_buf(buf: Buf) {
                unsafe { buf.free() }
            }

            static VTABLE: PluginVTable = PluginVTable {
                abi_version: ABI_VERSION,
                describe,
                call,
                free_buf,
            };

            #[unsafe(no_mangle)]
            pub extern "C" fn pkgsmith_plugin_entry() -> *const PluginVTable {
                &VTABLE
            }
        }
    };
}
