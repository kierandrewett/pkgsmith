# Plugins

Every package format of pkgsmith is a plugin. pkgsmith itself only reads the manifest, runs the builds and sends the
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

A plugin is a shared library whose file name starts with `pkgsmith-plugin-` and ends with `.so`. The name that
cargo gives a library, `libpkgsmith_plugin_NAME.so`, is accepted too. pkgsmith loads every such file in these folders,
in this order:

1. The folders you give with `--plugin-path DIR`. Repeat the option for more.
2. The folders in `PKGSMITH_PLUGIN_PATH`, separated by `:`.
3. The folder of the `pkgsmith` binary. A `cargo build` of this repository puts the plugins there.
4. `../lib/pkgsmith/plugins` next to the binary. A package that installs `/usr/bin/pkgsmith` finds
   `/usr/lib/pkgsmith/plugins` this way.
5. `$XDG_DATA_HOME/pkgsmith/plugins`, which is `~/.local/share/pkgsmith/plugins` by default.
6. `/usr/local/lib/pkgsmith/plugins`, `/usr/lib/pkgsmith/plugins` and `/usr/lib64/pkgsmith/plugins`.

If two plugins offer the same format, the first one found is used and pkgsmith prints a warning. A plugin that does
not load is skipped with a warning. Without any plugin, `pkgsmith build` stops and says so.

## What a plugin receives

pkgsmith sends one JSON document, a package, for every package it builds. It holds everything the format needs:

| Field | Meaning |
| ----- | ------- |
| `format` | The format to write, one the plugin lists |
| `name`, `version`, `release`, `arch` | Identity. `version` is as written in the manifest. `arch` is `amd64`, `arm64` or `all` |
| `description` | The first line is the summary, the rest is the long text |
| `maintainer`, `homepage`, `license` | Text, or `null` |
| `depends`, `recommends`, `suggests`, `conflicts`, `replaces`, `provides` | Lists of package names, after the `overrides` for this format and distro |
| `scripts` | `pre_install`, `post_install`, `pre_remove`, `post_remove`. The text of each script, which starts with `#!`, or `null` |
| `entries` | The files, see below |
| `options` | The section of the manifest named like the format, for example `deb:`, or `null` |
| `distro` | The distro of the job, or `null` |
| `mtime` | Seconds since the epoch for every time stamp, so equal input gives equal bytes |
| `out` | The folder for the package. It exists |

Each entry of `entries` has `path` (absolute, on the installed system), `type`, `mode`, `owner`, `group` and `config`.
`type` is one of `file` (with `src`, the path of the bytes on this machine, and `size`), `dir`, `symlink` (with
`target`) and `ghost` (owned by the package, not carried in it). Entries come sorted by path.

Run `pkgsmith build -F json` with the `json` plugin from [examples/plugins/rust-json](../examples/plugins/rust-json) to
see a real document.

### Format options

A top-level key of the manifest that has the name of a format is passed to that format as `options`:

```yaml
deb:
  section: utils
  compression: gzip
```

pkgsmith does not know the keys. The plugin decides, and a good plugin refuses a key it does not know. A top-level key
that is neither a manifest field nor a format of a loaded plugin is an error, so `dependz:` does not go unnoticed.

The plugins in this repository read these sections:

| Section     | Keys                                                                                         |
| ----------- | -------------------------------------------------------------------------------------------- |
| `deb`       | `section` (default `misc`), `priority` (`optional`), `compression` (`xz`, `zstd`, `gzip`)    |
| `rpm`       | `group` (`Unspecified`), `compression` (`zstd`, `xz`, `gzip`, `bzip2`)                              |
| `archlinux` | `packager` (default: the maintainer)                                                         |
| `ipk`       | `arch`: the OpenWrt architecture, for example `aarch64_generic`                              |
| `tar`       | `compression` (`xz`, `gzip`, `zstd`, `bzip2`, `none`), `top_folder`: put everything below `{name}-{version}/` |
| `zip`       | `top_folder`                                                                                 |

## The C interface

A plugin exports one function. Everything else is JSON, so any language works.

```c
typedef struct { uint8_t *ptr; size_t len; } PkgsmithBuf;

typedef struct {
    uint32_t abi_version;                    /* 1 */
    void *ctx;                               /* give it back as the first argument of log */
    void (*log)(void *ctx, uint32_t level, const uint8_t *message, size_t len); /* 0 error, 1 warning, 2 info */
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

[examples/plugins/c-listing/listing.c](../examples/plugins/c-listing/listing.c) is a complete plugin in C. It adds a
format that lists the files of a package.

```console
$ cc -shared -fPIC -o pkgsmith-plugin-listing.so listing.c
$ pkgsmith --plugin-path . build -F listing
```

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
format section in the manifest, define a struct with `#[serde(deny_unknown_fields)]` and read it with
`package.options::<MyOptions>()?`.

[examples/plugins/rust-json](../examples/plugins/rust-json) is this plugin as a crate. The built-in formats in
[plugins/](../plugins) are larger examples. Each one is a crate with a `lib.rs` that implements `Plugin`.

## Installing a plugin

Copy the library to a folder on the search path and name it `pkgsmith-plugin-NAME.so`. For one user:

```console
$ mkdir -p ~/.local/share/pkgsmith/plugins
$ cp target/release/libpkgsmith_plugin_json.so ~/.local/share/pkgsmith/plugins/pkgsmith-plugin-json.so
```
