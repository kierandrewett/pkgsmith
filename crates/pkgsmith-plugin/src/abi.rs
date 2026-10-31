//! The C interface between pkgsmith and a plugin.
//!
//! A plugin is a shared library whose file name starts with `pkgsmith-plugin-` (`libpkgsmith_plugin_` when cargo
//! names it). It exports one function, `pkgsmith_plugin_entry`, that returns a [`PluginVTable`]. Everything else
//! crosses the boundary as UTF-8 JSON, so a plugin can be written in any language that can export a C function.
//!
//! ```c
//! #include <stddef.h>
//! #include <stdint.h>
//!
//! typedef struct { uint8_t *ptr; size_t len; } PkgsmithBuf;
//!
//! typedef struct {
//!     uint32_t abi_version;                    /* 1 */
//!     void *ctx;                               /* give it back as the first argument of every function below */
//!     void (*log)(void *ctx, uint32_t level, const uint8_t *message, size_t len); /* 0 error, 1 warn, 2 info */
//!     uint32_t (*can_sign)(void *ctx, uint32_t scheme);
//!     int32_t (*sign)(void *ctx, uint32_t scheme, const uint8_t *data, size_t len, PkgsmithBuf *out);
//!     int32_t (*key_name)(void *ctx, uint32_t scheme, PkgsmithBuf *out);
//!     void (*free_buf)(PkgsmithBuf buf);       /* frees a buffer that sign or key_name returned */
//! } PkgsmithHost;
//!
//! typedef struct {
//!     uint32_t abi_version;                    /* 1 */
//!     PkgsmithBuf (*describe)(void);
//!     PkgsmithBuf (*call)(const uint8_t *method, size_t method_len,
//!                         const uint8_t *request, size_t request_len, const PkgsmithHost *host);
//!     void (*free_buf)(PkgsmithBuf buf);       /* frees a buffer that describe or call returned */
//! } PkgsmithPlugin;
//!
//! const PkgsmithPlugin *pkgsmith_plugin_entry(void);
//! ```
//!
//! `describe` returns `{"name": "deb", "version": "0.1.0", "formats": [{"name": "deb", "description": "..."}]}`.
//! `call` takes a method name and a JSON request, and returns `{"ok": VALUE}` or `{"error": "text"}`. The methods are
//! `file_name` (request: a [`crate::Package`] without files, value: the file name as a string) and `build` (request:
//! a [`crate::Package`], value: the path of the file that was written). The host owns the request. The plugin owns
//! the returned buffer until the host hands it back to `free_buf`.

/// The version of this interface. A plugin with another version is refused.
pub const ABI_VERSION: u32 = 1;

/// The name of the symbol every plugin exports.
pub const ENTRY_SYMBOL: &[u8] = b"pkgsmith_plugin_entry";

/// Bytes the plugin allocated. Only the plugin's `free_buf` may free them.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Buf {
    pub ptr: *mut u8,
    pub len: usize,
}

impl Buf {
    pub fn from_vec(bytes: Vec<u8>) -> Buf {
        let boxed = bytes.into_boxed_slice();
        let len = boxed.len();
        Buf {
            ptr: Box::into_raw(boxed) as *mut u8,
            len,
        }
    }

    /// # Safety
    /// `self` must come from [`Buf::from_vec`] of this library and must not be used again.
    pub unsafe fn free(self) {
        if !self.ptr.is_null() {
            drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.ptr, self.len)) });
        }
    }

    /// # Safety
    /// `ptr` and `len` must describe memory that is valid for the lifetime `'a`.
    pub unsafe fn as_slice<'a>(&self) -> &'a [u8] {
        if self.ptr.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// What the host gives a plugin during a call.
///
/// The signing functions let a plugin ask the host for a signature without handling the key. This keeps the key out of
/// the request, but it is not isolation: a plugin is native code in the process of pkgsmith and has the permissions of
/// the user. Install only plugins that you trust.
#[repr(C)]
pub struct HostVTable {
    pub abi_version: u32,
    pub ctx: *mut std::ffi::c_void,
    /// `level`: 0 error, 1 warning, 2 information.
    pub log: Option<
        unsafe extern "C" fn(
            ctx: *mut std::ffi::c_void,
            level: u32,
            message: *const u8,
            len: usize,
        ),
    >,
    /// 1 when the host holds a key for `scheme` for the format of this call, else 0. See [`Scheme`].
    pub can_sign: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, scheme: u32) -> u32>,
    /// Sign `data`. Returns 0 and the signature in `out`, or 1 and an error message in `out`. The host allocated `out`:
    /// give it back to `free_buf`.
    pub sign: Option<
        unsafe extern "C" fn(
            ctx: *mut std::ffi::c_void,
            scheme: u32,
            data: *const u8,
            len: usize,
            out: *mut Buf,
        ) -> i32,
    >,
    /// The name of the key for `scheme`: the file name of the private key. Returns 0 and the name in `out`, or 1 when
    /// there is none. The host allocated `out`.
    pub key_name:
        Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void, scheme: u32, out: *mut Buf) -> i32>,
    /// Frees a buffer that `sign` or `key_name` returned.
    pub free_buf: Option<unsafe extern "C" fn(buf: Buf)>,
}

/// How a signature is made. The numbers are part of the C interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u32)]
pub enum Scheme {
    /// A binary OpenPGP detached signature packet (SHA-256) from an OpenPGP key.
    OpenPgp = 1,
    /// An RSA PKCS#1 v1.5 signature over the SHA-256 digest, from an RSA key.
    RsaSha256 = 2,
    /// An RSA PKCS#1 v1.5 signature over the SHA-1 digest, from an RSA key.
    RsaSha1 = 3,
}

impl Scheme {
    pub fn from_number(number: u32) -> Option<Scheme> {
        match number {
            1 => Some(Scheme::OpenPgp),
            2 => Some(Scheme::RsaSha256),
            3 => Some(Scheme::RsaSha1),
            _ => None,
        }
    }
}

/// What a plugin gives the host.
#[repr(C)]
pub struct PluginVTable {
    pub abi_version: u32,
    pub describe: unsafe extern "C" fn() -> Buf,
    pub call: unsafe extern "C" fn(
        method: *const u8,
        method_len: usize,
        request: *const u8,
        request_len: usize,
        host: *const HostVTable,
    ) -> Buf,
    pub free_buf: unsafe extern "C" fn(Buf),
}

/// The type of `pkgsmith_plugin_entry`.
pub type EntryFn = unsafe extern "C" fn() -> *const PluginVTable;
