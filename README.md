# pkgsmith

pkgsmith builds distribution packages from one script. The script is a small program. pkgsmith runs it on your system,
builds your program there, and writes the rpm, deb, Arch, Alpine, OpenWrt, tar or zip package.

![pkgsmith builds one package for Fedora, Debian, Arch and Alpine in a single run](docs/media/demo.gif)

## Formats

Every format is a plugin: a shared library that pkgsmith loads. You can add your own. See [plugins](docs/plugins.md).

| Format      | File                             | For                        |
| ----------- | -------------------------------- | -------------------------- |
| `rpm`       | `.rpm`                           | Fedora, openSUSE, RHEL     |
| `deb`       | `.deb`                           | Debian, Ubuntu             |
| `archlinux` | `.pkg.tar.zst`                   | Arch Linux                 |
| `apk`       | `.apk`                           | Alpine                     |
| `ipk`       | `.ipk`                           | OpenWrt                    |
| `slackware` | `.txz`                           | Slackware                  |
| `xbps`      | `.xbps`                          | Void Linux                 |
| `freebsd`   | `.pkg`                           | FreeBSD                    |
| `snap`      | `.snap`                          | snapd                      |
| `oci`       | `.oci.tar`                       | OCI runtimes               |
| `appimage`  | `.AppImage`                      | Linux desktops             |
| `wheel`     | `.whl`                           | pip                        |
| `gem`       | `.gem`                           | RubyGems                   |
| `npm`       | `.tgz`                           | npm                        |
| `conda`     | `.conda`, `.tar.bz2`             | conda, micromamba          |
| `nupkg`     | `.nupkg`                         | NuGet                      |
| `dmg`       | `.dmg`                           | macOS                      |
| `flatpak`   | `.flatpak`                       | Flatpak                    |
| `nix`       | `.nix.tar.xz`                    | Nix, NixOS, flakes         |
| `msi`       | `.msi`                           | Windows                    |
| `nsis`      | `.exe`                           | Windows                    |
| `msix`      | `.msix`                          | Windows 10 and 11          |
| `tar`       | `.tar.xz`, `.gz`, `.zst`, `.bz2` | Any system                 |
| `zip`       | `.zip`                           | Any system                 |

Options and what each format was checked with are in [formats](docs/formats.md).

pkgsmith writes these files itself and does not call `rpmbuild`, `dpkg-deb`, `makepkg` or `abuild`.

## Install

pkgsmith runs on Linux, macOS and Windows. It needs no daemon. The build commands in your
script run on the system that runs pkgsmith, so that system needs your compiler and tools.

There is no published release yet, so build from source:

```sh
git clone https://github.com/kierandrewett/pkgsmith
cd pkgsmith
cargo build --release --workspace
```

This makes `target/release/pkgsmith` and one plugin library per format. pkgsmith loads plugins from the folder of its
binary, so run it from `target/release`. A release also has a `pkgsmith-TARGET.tar.gz` with the program and its plugins for each of these targets:

- `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`
- `aarch64-apple-darwin` and `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`

To install a build for one user on Linux, copy the plugins under the name `pkgsmith-plugin-NAME.so`:

```sh
install -Dm755 target/release/pkgsmith ~/.local/bin/pkgsmith
for f in target/release/libpkgsmith_plugin_*.so; do
    install -Dm755 "$f" ~/.local/share/pkgsmith/plugins/pkgsmith-plugin-${f##*/libpkgsmith_plugin_}
done
pkgsmith plugins
```

On macOS the libraries end in `.dylib` and the folder is `~/Library/Application Support/pkgsmith/plugins`. On Windows
they end in `.dll` and the folder is `%LOCALAPPDATA%\pkgsmith\plugins`. `pkgsmith plugins install` does this copy for
you on every system.

`pkgsmith plugins` lists what loaded. Once a release exists, `cargo binstall pkgsmith` installs the program and
`pkgsmith plugins install` downloads its plugins. See [command line](docs/cli.md#plugins).

## Quick start

`examples/demo` holds a C program and a Makefile. Its `pkgsmith.star` is a script that pkgsmith runs top to bottom:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A demo that packages a small C program",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "license": "MIT",
}

sh("make")
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

native = host()["format"]
if native:
    package(native, **common)
package("tar", **common)
```

Build it:

```sh
cd examples/demo
pkgsmith build        # the packages go to ./dist
```

- `sh` adds a command to the build. Each command runs in the folder of the script, in order.
- `$pkgdir` is an empty folder. What `make install` puts there becomes the package.
- `host()["format"]` is the package format of this system: `rpm` on Fedora, `deb` on Debian, and so on.
- Each `package` call that runs writes one package. On Fedora this script writes an rpm and a tar.

To get a deb, run pkgsmith on Debian. To get an rpm too, run it on Fedora. Use your CI for this: one job for each
system. See [building](docs/building.md#packages-for-other-distros).

To choose the files yourself, use `expose` and the content helpers. See [contents](docs/contents.md). The script is
code, so use `if`, loops and functions for what differs between systems. See [scripting](docs/scripting.md). pkgsmith
packages itself with [pkgsmith.star](pkgsmith.star).

## Commands

| Command           | What it does                                                  |
| ----------------- | ------------------------------------------------------------- |
| `build`           | Runs the script, builds, and writes every package to `dist`   |
| `plan`            | Shows each package and its file name. Runs no build           |
| `files`           | Lists the files each package would carry                      |
| `formats`         | Lists the formats the installed plugins write                 |
| `plugins`         | Lists the loaded plugins and the folders searched for them    |

Run `pkgsmith plan` after you change the script. Packages with the same `BUILD` letter share one build. Every option is
in [command line](docs/cli.md).

## Documentation

| Page                                              | Covers                                          |
| ------------------------------------------------- | ----------------------------------------------- |
| [Getting started](docs/getting-started.md)        | A package from scratch, step by step            |
| [Script reference](docs/manifest.md)              | Every function, field and value                 |
| [Contents](docs/contents.md)                      | Files, trees, links and config files            |
| [Building](docs/building.md)                      | How the build runs, `$pkgdir`, other distros    |
| [macOS and Windows](docs/macos-and-windows.md)    | Compile on the target system, then package      |
| [Scripting](docs/scripting.md)                    | Write a script: `host()`, conditions, loops     |
| [Versions](docs/versions.md)                      | How one version becomes each format's spelling  |
| [Install scripts](docs/scripts.md)                | What runs when, per format                      |
| [Dependencies](docs/dependencies.md)              | Names, version constraints, per-distro names    |
| [Signing](docs/signing.md)                        | Keys, `--sign-key`, and checking a signature    |
| [Formats](docs/formats.md)                        | The options of every format                     |
| [Plugins](docs/plugins.md)                        | Where plugins live, and how to write one        |
| [Command line](docs/cli.md)                       | Every command and option                        |

Every file in [examples](examples) is a complete script.

## Limits

- pkgsmith does not find the shared libraries a program needs. List them in `depends`.
- A build makes packages for the system it runs on. For another distro or architecture, run pkgsmith there. See
  [building](docs/building.md#packages-for-other-distros).
- pkgsmith does not compile for macOS or Windows. Compile there, or compile with your own tools, and package the files
  with a script that has no `sh` commands. See [macOS and Windows](docs/macos-and-windows.md).
- Packages are signed only when you give `--sign-key`. See [signing](docs/signing.md).
- There is no built-in support for a language. Call its build tool with `sh`. [examples/rust-cli.star](examples/rust-cli.star)
  does this for Cargo.

## Build and test

```sh
cargo build --workspace
cargo test --workspace
```

## Licence

MIT
