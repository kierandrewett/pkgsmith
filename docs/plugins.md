# Plugins

Every package format of pkgsmith is a plugin. pkgsmith itself only runs the script, collects the files of the prefix and sends the
result to a plugin. The formats that come with pkgsmith use the same interface that you can use.

```console
$ pkgsmith plugins
Search path:
  /usr/bin
  /usr/lib/pkgsmith/plugins
  ...

Loaded:
  deb 0.1.0  deb, ipk  (/usr/lib/pkgsmith/plugins/pkgsmith-plugin-deb.so)
  rpm 0.1.0  rpm  (/usr/lib/pkgsmith/plugins/pkgsmith-plugin-rpm.so)
  ...
```

```console
$ pkgsmith formats
apk        An Alpine package (.apk)
archlinux  An Arch Linux package (.pkg.tar.zst)
...
```

## Where pkgsmith looks

A plugin is a shared library whose file name starts with `pkgsmith-plugin-` and ends with the shared library suffix of
the system. The name that cargo gives a library is accepted too.

| System  | Suffix   | Name from cargo                |
| ------- | -------- | ------------------------------ |
| Linux   | `.so`    | `libpkgsmith_plugin_NAME.so`   |
| macOS   | `.dylib` | `libpkgsmith_plugin_NAME.dylib` |
| Windows | `.dll`   | `pkgsmith_plugin_NAME.dll`     |

pkgsmith loads every such file in these folders, in this order:

1. The folders you give with `--plugin-path DIR`. Repeat the option for more.
2. The folders in `PKGSMITH_PLUGIN_PATH`, separated by `:`. On Windows the separator is `;`.
3. The folder of the `pkgsmith` binary. A `cargo build` of this repository puts the plugins there.
4. `lib/pkgsmith/plugins` in the folder above the binary. A package that installs `/usr/bin/pkgsmith` finds
   `/usr/lib/pkgsmith/plugins` this way.
5. The data folder of the user, with `pkgsmith/plugins` below it:

   | System  | Folder                                                                          |
   | ------- | ------------------------------------------------------------------------------- |
   | Linux   | `$XDG_DATA_HOME/pkgsmith/plugins`, by default `~/.local/share/pkgsmith/plugins` |
   | macOS   | `~/Library/Application Support/pkgsmith/plugins`                                |
   | Windows | `%LOCALAPPDATA%\pkgsmith\plugins`                                              |

6. The folders for all users:

   | System  | Folders                                                                                        |
   | ------- | ---------------------------------------------------------------------------------------------- |
   | Linux   | `/usr/local/lib/pkgsmith/plugins`, `/usr/lib/pkgsmith/plugins` and `/usr/lib64/pkgsmith/plugins` |
   | macOS   | `/usr/local/lib/pkgsmith/plugins` and `/opt/homebrew/lib/pkgsmith/plugins`                     |
   | Windows | `%ProgramData%\pkgsmith\plugins`                                                              |

If two plugins offer the same format, the first one found is used and pkgsmith prints a warning. A plugin that does
not load is skipped with a warning. Without any plugin, `pkgsmith build` stops and says so.

## What a plugin receives

pkgsmith sends one JSON document, a package, for every package it writes. It holds everything the format needs:

| Field | Meaning |
| ----- | ------- |
| `format` | The format to write, one the plugin lists |
| `name`, `version`, `release`, `arch` | Identity. `version` is as written in the script. `arch` is `amd64`, `arm64` or `all` |
| `description` | The first line is the summary, the rest is the long text |
| `maintainer`, `homepage`, `license` | Text, or `null` |
| `depends`, `recommends`, `suggests`, `conflicts`, `replaces`, `provides` | Lists of package names, as the script gives them |
| `scripts` | `pre_install`, `post_install`, `pre_remove`, `post_remove`. The text of each script, which starts with `#!`, or `null` |
| `entries` | The files, see below |
| `options` | The `options` dict of the package, for example `{"section": "utils"}`. An empty object when the package sets none |
| `distro` | The `distro` of the package, or `null` |
| `mtime` | Seconds since the epoch for every time stamp, so equal input gives equal bytes |
| `out` | The folder for the package. It exists |

Each item of `entries` describes one path:

| Field | Meaning |
| ----- | ------- |
| `path` | Absolute path on the installed system |
| `type` | `file`, `dir`, `symlink` or `ghost` |
| `mode`, `owner`, `group` | Permissions as a number, and the owner and group names |
| `config` | `true` for a file that the package manager keeps when the user changed it |

Some fields depend on `type`:

- `file` adds `src`, the path of the bytes on this machine, and `size`.
- `symlink` adds `target`, the link target as it reads on the installed system.
- `ghost` is a path that the package owns but does not carry.

Entries come sorted by path.

Run `pkgsmith build -F json` with the `json` plugin from [examples/plugins/rust-json](../examples/plugins/rust-json) to
see a real document.

### Format options

The `options` dict of a `package()` call is passed to its format as `options`:

```python
package(
    "deb",
    name = "hello", version = "1.0.0", description = "A small program",
    options = {"section": "utils", "compression": "gzip"},
)
```

pkgsmith does not know the keys. The plugin decides, and a good plugin refuses a key it does not know, so `sectoin` does
not go unnoticed.

The options of the plugins in this repository are in [formats](formats.md).

## The C interface

A plugin exports one function. Everything else is JSON, so any language works.

```c
typedef struct { uint8_t *ptr; size_t len; } PkgsmithBuf;

typedef struct {
    uint32_t abi_version;                    /* 1 */
    void *ctx;                               /* give it back as the first argument of every function below */
    void (*log)(void *ctx, uint32_t level, const uint8_t *message, size_t len); /* 0 error, 1 warning, 2 info */
    uint32_t (*can_sign)(void *ctx, uint32_t scheme);                   /* 1 when the host holds a key */
    int32_t (*sign)(void *ctx, uint32_t scheme, const uint8_t *data, size_t len, PkgsmithBuf *out);
    int32_t (*key_name)(void *ctx, uint32_t scheme, PkgsmithBuf *out);
    void (*free_buf)(PkgsmithBuf buf);       /* frees a buffer that sign or key_name returned */
} PkgsmithHost;

typedef struct {
    uint32_t abi_version;                    /* 1 */
    PkgsmithBuf (*describe)(void);
    PkgsmithBuf (*call)(const uint8_t *method, size_t method_len,
                        const uint8_t *request, size_t request_len, const PkgsmithHost *host);
    void (*free_buf)(PkgsmithBuf buf);       /* frees a buffer that describe or call returned */
} PkgsmithPlugin;

const PkgsmithPlugin *pkgsmith_plugin_entry(void);
```

- `describe` returns `{"name": "listing", "version": "0.1.0", "formats": [{"name": "listing", "description": "..."}]}`.
- `call` takes a method name and a request. It returns `{"ok": VALUE}` or `{"error": "text"}`.
  - `file_name`: the request is a package without `entries` and scripts. `VALUE` is the file name, a string. pkgsmith
    uses it for `pkgsmith plan`.
  - `build`: the request is a package. The plugin writes the file into `out`. `VALUE` is the path of the file.
- The plugin allocates every buffer it returns. pkgsmith gives each one back to `free_buf` after it reads it.
- pkgsmith may call a plugin from several threads at the same time.
- A plugin with another `abi_version` is refused.

### Signing

A format plugin states in `describe` how its format is signed, with the field `signing` of each format:

| Value                       | Meaning                                                                      |
| --------------------------- | ---------------------------------------------------------------------------- |
| `"detached"` (the default)  | pkgsmith writes a detached OpenPGP signature next to the package file        |
| `{"embedded": "SCHEME"}`    | The plugin puts the signature inside the package. See below                  |
| `"none"`                    | The package cannot be signed                                                 |

An embedding plugin calls `can_sign` and, when it returns 1, `sign`. The host finds the key and a signer plugin that
takes it. The `scheme` is a number in C and a name in JSON:

| Number | Name           | The signature                                                                  |
| ------ | -------------- | ------------------------------------------------------------------------------ |
| 1      | `openpgp`      | A binary OpenPGP detached signature packet over SHA-256, from an OpenPGP key   |
| 2      | `rsa-sha256`   | RSA PKCS#1 v1.5 over the SHA-256 digest of the data, from an RSA key           |
| 3      | `rsa-sha1`     | RSA PKCS#1 v1.5 over the SHA-1 digest of the data, from an RSA key             |

- `sign` returns 0 and the signature in `out`, or 1 and an error message in `out`.
- `key_name` gives the file name of the key. The `apk` plugin puts it into the signature name.
- The host allocated both buffers. Give them back to `free_buf`.

See [signing](signing.md) for the keys and the command line options.

## Writing a signer

A signer is a plugin that signs. It lists `signers` in `describe`, next to or instead of `formats`:

```json
{"name": "signer-gpg", "version": "0.1.0", "signers": [
  {"name": "gpg", "description": "Signs with gpg", "schemes": ["openpgp"]}
]}
```

Two more methods reach a signer. Both take a JSON signing request, and the data goes by path, not inside the JSON:

| Field             | Meaning                                                                               |
| ----------------- | ------------------------------------------------------------------------------------- |
| `signer`          | The signer the host chose, one of the names the plugin lists                          |
| `scheme`          | One of the schemes in the table above                                                 |
| `key`             | What the user gave after `--sign-key`: a key file or an identifier. The signer decides |
| `passphrase_file` | A file with the passphrase, or `null`                                                 |
| `input`, `output` | The file to sign and the file to write. Empty for `accepts`                           |
| `armor`           | `true` for OpenPGP ASCII armor                                                        |

- `accepts` returns `{"ok": true}` when the signer can sign with this key, and `{"ok": false}` when the key is not
  its own. For a key that is its own but unusable, such as a locked key or a wrong passphrase, return an error with the
  reason. The host stops at that error and tries no other signer. Open the key in `accepts`, so that a bad key fails
  before any package is written.
- `sign` writes the signature to `output` and returns `{"ok": true}`.

In Rust, implement `accepts` and `sign` of the `Plugin` trait, and leave `file_name` and `build` as they are.
[plugins/signer-gpg](../plugins/signer-gpg) is the smallest example.

A plugin is code in the process of pkgsmith. Signing through the host keeps the key out of the package description, but
it does not hide the key from a plugin.

## Writing a plugin in Rust

Make a library crate with `crate-type = ["cdylib"]` that depends on `pkgsmith-plugin`. The crate has the `Package`
type, the `Plugin` trait, the C interface, and the helpers the built-in formats use: tar and ar writers, compression,
hashes and the version spellings of [versions](versions.md).

```rust
use anyhow::Result;
use pkgsmith_plugin::plugin::expect_format;
use pkgsmith_plugin::{FormatInfo, Host, Package, Plugin, PluginInfo};
use std::path::PathBuf;

struct Json;

impl Plugin for Json {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "json".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            formats: vec![FormatInfo { name: "json".into(), description: "The package as JSON".into() }],
        }
    }

    fn file_name(&self, package: &Package) -> Result<String> {
        expect_format(package, &["json"])?;
        Ok(format!("{}-{}.json", package.name, package.version))
    }

    fn build(&self, package: &Package, host: &Host) -> Result<PathBuf> {
        let file = package.output(&self.file_name(package)?);
        std::fs::write(&file, serde_json::to_vec_pretty(package)?)?;
        host.info(&format!("{} files", package.entries.len()));
        Ok(file)
    }
}

pkgsmith_plugin::export_plugin!(Json);
```

`export_plugin!` writes `pkgsmith_plugin_entry`, catches panics so they become an error, and frees buffers. For a
format options in the script, define a struct with `#[serde(deny_unknown_fields)]` and read it with
`package.options::<MyOptions>()?`. A unit test that calls `build` itself can pass `&Host::silent()`, a host that shows
nothing and holds no key.

[examples/plugins/rust-json](../examples/plugins/rust-json) is this plugin as a crate. The built-in formats in
[plugins/](../plugins) are larger examples. Each one is a crate with a `lib.rs` that implements `Plugin`.

## Installing plugins

Copy a library to a folder on the search path and name it `pkgsmith-plugin-NAME` with the suffix of your system. For one
user on Linux:

```console
$ mkdir -p ~/.local/share/pkgsmith/plugins
$ cp target/release/libpkgsmith_plugin_json.so ~/.local/share/pkgsmith/plugins/pkgsmith-plugin-json.so
```

The built-in plugins come with each release. `cargo install pkgsmith` and `cargo binstall pkgsmith` install the program
only, because cargo cannot install libraries. Run this once afterwards:

```console
$ pkgsmith plugins install
```

It downloads the archive `pkgsmith-TARGET.tar.gz` of the release that matches the program, checks it against the
`SHA256SUMS` file of the release, and copies the plugins into the data folder of the user from step 5 of the search
path. It downloads over https only.

Loading a plugin runs its code, so install plugins only from a source you trust. See
[command line](cli.md#plugins) for the options.
