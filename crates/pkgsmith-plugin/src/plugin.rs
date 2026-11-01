//! The Rust side of writing a plugin: implement [`Plugin`] and call [`export_plugin!`](crate::export_plugin).

use crate::abi::{Buf, HostVTable, Scheme};
use crate::package::Package;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How a format carries a signature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Signing {
    /// The package cannot be signed by pkgsmith. Snap and OCI images are signed by other means.
    None,
    /// The package file stays as it is and the host writes a detached OpenPGP signature next to it. The default.
    #[default]
    Detached,
    /// The plugin puts the signature inside the package. It asks the host for it with [`Host::sign`], using the scheme.
    Embedded(Scheme),
}

/// One format a plugin writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatInfo {
    /// The name in `format:` of a matrix entry and on the command line, for example `deb`.
    pub name: String,
    pub description: String,
    /// How the format is signed. In JSON: `"none"`, `"detached"` (the default) or `{"embedded": "rsa-sha256"}`.
    #[serde(default)]
    pub signing: Signing,
}

/// A way to sign that a plugin offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerInfo {
    /// The name in `--sign-key SIGNER:KEY`, for example `gpg`.
    pub name: String,
    pub description: String,
    /// The schemes this signer can make.
    pub schemes: Vec<Scheme>,
}

/// What `describe` returns.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    /// The formats the plugin writes. Empty for a plugin that only signs.
    #[serde(default)]
    pub formats: Vec<FormatInfo>,
    /// The ways to sign that the plugin offers. Empty for a plugin that only writes formats.
    #[serde(default)]
    pub signers: Vec<SignerInfo>,
}

/// What the host asks of a signer: sign the file `input` and write the signature to `output`. The data goes by path
/// and not inside the JSON, because a package can be large.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignRequest {
    /// The signer the host chose, one of the names the plugin lists.
    pub signer: String,
    pub scheme: Scheme,
    /// What the user gave after `--sign-key`: a key file, or an identifier such as a key id. The signer decides what it
    /// means.
    pub key: String,
    /// A file that holds the passphrase of the key, or `None`.
    pub passphrase_file: Option<PathBuf>,
    pub input: PathBuf,
    /// Where to write the signature. A binary packet or raw bytes, or ASCII armor when `armor` is true.
    pub output: PathBuf,
    /// Write OpenPGP armor. Only `openpgp` signatures can be armored.
    pub armor: bool,
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

impl Host<'static> {
    /// A host that shows nothing and holds no key, for tests of a plugin that call `build` themselves.
    pub fn silent() -> Host<'static> {
        Host { raw: None }
    }
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

    /// Whether the host holds a key for `scheme` for the format of this call. A plugin signs only when it does.
    pub fn can_sign(&self, scheme: Scheme) -> bool {
        match self.raw {
            // SAFETY: the host promises that `ctx` and the functions stay valid for the call.
            Some(raw) => raw
                .can_sign
                .is_some_and(|f| unsafe { f(raw.ctx, scheme as u32) } == 1),
            None => false,
        }
    }

    /// Sign `data` with the key of the host. The key stays with the host, but a plugin is trusted code in the same
    /// process, so this is a convenience and not isolation.
    pub fn sign(&self, scheme: Scheme, data: &[u8]) -> Result<Vec<u8>> {
        let Some(raw) = self.raw else {
            bail!("there is no host to sign with")
        };
        let (Some(sign), Some(free)) = (raw.sign, raw.free_buf) else {
            bail!("the host cannot sign");
        };
        let mut out = Buf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        // SAFETY: the pointers are valid for the call. `out` is filled by the host and freed by it.
        unsafe {
            let status = sign(raw.ctx, scheme as u32, data.as_ptr(), data.len(), &mut out);
            let bytes = out.as_slice().to_vec();
            free(out);
            if status != 0 {
                bail!("{}", String::from_utf8_lossy(&bytes));
            }
            Ok(bytes)
        }
    }

    /// The name of the key for `scheme`, which Alpine writes into the package so that `apk` finds the public key.
    pub fn key_name(&self, scheme: Scheme) -> Option<String> {
        let raw = self.raw?;
        let (key_name, free) = (raw.key_name?, raw.free_buf?);
        let mut out = Buf {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        // SAFETY: as in `sign`.
        unsafe {
            let status = key_name(raw.ctx, scheme as u32, &mut out);
            let name = String::from_utf8_lossy(out.as_slice()).into_owned();
            free(out);
            (status == 0).then_some(name)
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

    /// The name of the file `build` writes, from the package alone. `package.entries` is empty. A plugin that writes
    /// formats must implement this.
    fn file_name(&self, _package: &Package) -> Result<String> {
        bail!("this plugin writes no formats")
    }

    /// Write the package into `package.out` and return the path of the file. A plugin that writes formats must
    /// implement this.
    fn build(&self, _package: &Package, _host: &Host) -> Result<PathBuf> {
        bail!("this plugin writes no formats")
    }

    /// Whether this signer can sign with the request: it is one of its names, it makes the scheme, and `key` is a key
    /// it can use. `Ok(false)` means "not mine". An `Err` means the key is mine but cannot be used, and its text is
    /// shown to the user, for example "the key is locked". A plugin that signs must implement this.
    fn accepts(&self, _request: &SignRequest) -> Result<bool> {
        Ok(false)
    }

    /// Sign `request.input` and write the signature to `request.output`. A plugin that signs must implement this.
    fn sign(&self, _request: &SignRequest) -> Result<()> {
        bail!("this plugin has no signer")
    }
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
        let host = Host { raw: host };
        match method {
            "file_name" | "build" => match serde_json::from_slice::<Package>(request) {
                Err(error) => reply::<()>(Err(
                    anyhow::Error::from(error).context("the request is not a package")
                )),
                Ok(package) if method == "file_name" => reply(plugin.file_name(&package)),
                Ok(package) => reply(
                    plugin
                        .build(&package, &host)
                        .map(|p| p.display().to_string()),
                ),
            },
            "accepts" | "sign" => match serde_json::from_slice::<SignRequest>(request) {
                Err(error) => reply::<()>(Err(
                    anyhow::Error::from(error).context("the request is not a signing request")
                )),
                Ok(request) if method == "accepts" => reply(plugin.accepts(&request)),
                Ok(request) => reply(plugin.sign(&request).map(|()| true)),
            },
            other => reply::<()>(Err(anyhow::anyhow!("unknown method {other:?}"))),
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
