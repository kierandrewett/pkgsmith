# pkgsmith

pkgsmith writes distribution packages from one script. You build your program and install it into a prefix. A short
`pkgsmith.star` script describes the package. pkgsmith writes the rpm, deb, Arch, Alpine, OpenWrt, tar or zip package.
It builds nothing itself, and it runs no container.

![pkgsmith writes an rpm, a deb and a tar from one prefix in a single run](docs/media/demo.gif)

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
| `oci`       | `.oci.tar`                       | podman, docker             |
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

pkgsmith runs on Linux, macOS and Windows. It needs nothing but its own program and plugins. It does not need a container
engine, and it does not build your program.

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

`examples/demo` holds a C program, a Makefile and this script:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A demo that packages a small C program",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "license": "MIT",
}

for format in ["rpm", "deb", "apk", "tar"]:
    package(format, **common)
```

Build the program into a prefix, then write the packages:

```sh
cd examples/demo
make install PREFIX=/usr DESTDIR="$PWD/prefix"    # your own build, into ./prefix
pkgsmith build                                    # the packages go to ./dist
```

How it works:

- The script runs from top to bottom. Each `package()` call that runs writes one package.
- A package holds the whole prefix. `include()` and `exclude()` narrow it, and `reset()` starts again from the whole
  prefix, so one prefix can make several packages.
- One script writes every format. A format is a file that a plugin writes, so no format needs its own system.
- The script can read the prefix and ask about the system with `host()`, but it cannot change anything or run a command.

To choose the files yourself, see [contents](docs/contents.md). pkgsmith packages itself with
[pkgsmith.star](pkgsmith.star).

## Commands

| Command           | What it does                                                  |
| ----------------- | ------------------------------------------------------------- |
| `build`           | Builds every package and writes it to `dist`                  |
| `plan`            | Shows each package and its file name                          |
| `files`           | Lists the files each package would carry                      |
| `formats`         | Lists the formats the installed plugins write                 |
| `plugins`         | Lists the loaded plugins and the folders searched for them    |

Run `pkgsmith plan` after you change a script to see which packages it writes. Every option is in
[command line](docs/cli.md).

## Documentation

| Page                                              | Covers                                          |
| ------------------------------------------------- | ----------------------------------------------- |
| [Getting started](docs/getting-started.md)        | A package from scratch, step by step            |
| [Script reference](docs/manifest.md)              | Every function and package field                |
| [Contents](docs/contents.md)                      | Files, trees, links and config files            |
| [Scripting](docs/scripting.md)                    | Loops, `host()`, `arg()`, include and exclude   |
| [Prefix](docs/prefix.md)                          | Making the prefix, and what a script may read   |
| [macOS and Windows](docs/macos-and-windows.md)    | Compile on the target system, then package      |
| [Versions](docs/versions.md)                      | How one version becomes each format's spelling  |
| [Install scripts](docs/scripts.md)                | What runs when, per format                      |
| [Dependencies](docs/dependencies.md)              | Names, version constraints, per-distro names    |
| [Signing](docs/signing.md)                        | Keys, `--sign-key`, and checking a signature    |
| [Formats](docs/formats.md)                        | The options of every format                     |
| [Plugins](docs/plugins.md)                        | Where plugins live, and how to write one        |
| [Command line](docs/cli.md)                       | Every command and option                        |

Every file in [examples](examples) is a complete script. [examples/full.star](examples/full.star) uses every function and
comments each one.

## Limits

- pkgsmith does not build your program. Make the prefix with your own build, for example `make install DESTDIR=prefix`.
- pkgsmith does not find the shared libraries a program needs. List them in `depends`.
- A prefix holds one architecture and one C library. For arm64, or for Alpine (musl), build there and run pkgsmith on
  that prefix. See [prefix](docs/prefix.md).
- Packages are signed only when you give `--sign-key`. See [signing](docs/signing.md).
- pkgsmith is in the same space as [nfpm](https://nfpm.goreleaser.com), which writes deb, rpm, apk, ipk, Arch Linux and
  msix packages from a YAML file. pkgsmith uses a script instead of YAML and writes more formats, but it is newer.

## Build and test

```sh
cargo build --workspace
cargo test --workspace
```

## Licence

MIT
