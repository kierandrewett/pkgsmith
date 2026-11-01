# pkgsmith

pkgsmith writes distribution packages from files you already built. You install your program into a folder, the
**prefix**, and write a short script that describes the packages made from it. pkgsmith writes the rpm, deb, Arch,
Alpine, OpenWrt, tar, zip and other packages from that one prefix. It builds nothing and runs no command.

## How it compares

pkgsmith is in the same space as [nfpm](https://nfpm.goreleaser.com) and [fpm](https://fpm.readthedocs.io): tools that
turn files into packages without building anything.

- **A script, not YAML.** The package description is a Starlark script, so a list of formats is a loop and a dependency
  name that differs by format is a dict. The script cannot run commands or reach the network.
- **More formats.** Twenty-four formats today, listed below. nfpm writes six (deb, rpm, apk, ipk, Arch Linux and msix).
- **Signing.** Packages are signed with gpg, an OpenPGP key file or an RSA key. See [signing](docs/signing.md).
- **Newer.** nfpm and fpm are older and widely used. pkgsmith has been checked against the package managers listed in
  [formats](docs/formats.md), and some formats only by layout. fpm also converts between package types. pkgsmith does
  not.

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

pkgsmith runs on Linux, macOS and Windows. It needs no daemon and no build tools: it reads a prefix.

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

Build your program with your own tools, and install it into a prefix. For a project with a Makefile:

```sh
make install PREFIX=/usr DESTDIR="$PWD/prefix"
```

Describe the packages in `pkgsmith.star` next to it:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A small C program",
    "maintainer": "Your Name <you@example.com>",
    "license": "MIT",
}

expose(tree("/", "/"))

for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(fmt, **common)
```

Write the packages:

```sh
pkgsmith plan         # what would be written
pkgsmith build        # the packages go to ./dist
```

- `expose(tree("/", "/"))` offers the whole prefix to the packages that follow.
- Each `package` call writes one package. The loop writes six from the same prefix, on any system: the plugins write
  the files of every format. Use `-F` to write only some.
- pkgsmith reads `./prefix` by default. `--prefix DIR` names another folder.

To get arm64 packages, make an arm64 prefix and write `arch = "arm64"`. See
[the prefix](docs/prefix.md#other-architectures-and-c-libraries).

To choose the files yourself, use `expose` and the content helpers. See [contents](docs/contents.md). The script is
code, so use `if`, loops and functions for what differs between packages. See [scripting](docs/scripting.md). pkgsmith
packages itself with [pkgsmith.star](pkgsmith.star).

## Commands

| Command   | What it does                                                 |
| --------- | ------------------------------------------------------------ |
| `build`   | Runs the script and writes every package to `dist`           |
| `plan`    | Shows each package and its file name. Writes nothing         |
| `files`   | Lists the files each package would carry                     |
| `formats` | Lists the formats the installed plugins write                |
| `plugins` | Lists the loaded plugins and the folders searched for them   |

Run `pkgsmith plan` after you change the script. Every option is in [command line](docs/cli.md).

## Documentation

| Page                                           | Covers                                              |
| ---------------------------------------------- | --------------------------------------------------- |
| [Getting started](docs/getting-started.md)     | A package from scratch, step by step                |
| [The prefix](docs/prefix.md)                   | Make a prefix with make, CMake, Meson, Cargo, Python |
| [Scripting](docs/scripting.md)                 | Write a script: `expose`, loops, `arg()`, `load()`  |
| [Script reference](docs/manifest.md)           | Every function, field and value                     |
| [Contents](docs/contents.md)                   | Files, trees, links and config files                |
| [Versions](docs/versions.md)                   | How one version becomes each format's spelling      |
| [Install scripts](docs/scripts.md)             | What runs when, per format                          |
| [Dependencies](docs/dependencies.md)           | Names, version constraints, per-format names        |
| [Signing](docs/signing.md)                     | Keys, `--sign-key`, and checking a signature        |
| [Formats](docs/formats.md)                     | The options of every format                         |
| [macOS and Windows](docs/macos-and-windows.md) | Build on that system, then pack the prefix          |
| [Plugins](docs/plugins.md)                     | Where plugins live, and how to write one            |
| [Command line](docs/cli.md)                    | Every command and option                            |

Every file in [examples](examples) is a complete script.

## Limits

- pkgsmith builds nothing. It does not compile, run `make` or fetch sources. You make the prefix.
- pkgsmith does not find the shared libraries a program needs. List them in `depends`.
- pkgsmith does not look inside the files. You say which architecture they are for with `arch`, and which C library they
  use by the prefix you pack. A glibc prefix is not a musl prefix, so an Alpine package needs a prefix built on musl.
  See [the prefix](docs/prefix.md#other-architectures-and-c-libraries).
- macOS and Windows programs are built on those systems, then packed from a prefix. See
  [macOS and Windows](docs/macos-and-windows.md).
- Packages are signed only when you give `--sign-key`. See [signing](docs/signing.md).

## Build and test

```sh
cargo build --workspace
cargo test --workspace
```

## Licence

MIT
