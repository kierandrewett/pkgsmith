# pkgsmith

pkgsmith builds distribution packages from one manifest. You describe the package once. pkgsmith builds your program
in a container for each distro and writes the rpm, deb, Arch, Alpine, OpenWrt, tar or zip package.

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
| `tar`       | `.tar.xz`, `.gz`, `.zst`, `.bz2` | Any system                 |
| `zip`       | `.zip`                           | Any system                 |

pkgsmith writes these files itself and does not call `rpmbuild`, `dpkg-deb`, `makepkg` or `abuild`. The packages
installed and ran in Fedora 43, Debian 13, Alpine 3.22 and Arch. The rpm also installed in openSUSE Tumbleweed.

## Install

You need [Podman](https://podman.io/). A user socket is enough.

There is no published release yet, so build from source:

```sh
git clone https://github.com/kierandrewett/pkgsmith
cd pkgsmith
cargo build --release --workspace
```

This makes `target/release/pkgsmith` and the plugins `target/release/libpkgsmith_plugin_*.so`. pkgsmith loads plugins
from the folder of its binary, so run it from `target/release`. To install it for one user, copy the plugins under the
name `pkgsmith-plugin-NAME.so`:

```sh
install -Dm755 target/release/pkgsmith ~/.local/bin/pkgsmith
for f in target/release/libpkgsmith_plugin_*.so; do
    install -Dm755 "$f" ~/.local/share/pkgsmith/plugins/pkgsmith-plugin-${f##*/libpkgsmith_plugin_}
done
pkgsmith plugins
```

`pkgsmith plugins` lists what loaded. Once a release exists, `cargo binstall pkgsmith` installs the program and
`pkgsmith plugins install` downloads its plugins. See [command line](docs/cli.md#plugins).

## Quick start

`examples/demo` holds a C program, a Makefile and this manifest:

```yaml
name: hello
version: 1.0.0
description: A demo that packages a small C program
maintainer: Kieran Drewett <kieran@drewett.dev>
license: MIT

build: |
  make
  make install PREFIX=/usr

matrix:
  include:
    - format: rpm
      distro: fedora
      image: fedora:43
      setup: dnf -y install gcc make
    - format: deb
      distro: debian
      image: debian:13
      setup: |
        apt-get update -qq
        DEBIAN_FRONTEND=noninteractive apt-get install -y gcc make
    - format: apk
      distro: alpine
      image: alpine:3.22
      setup: apk add --no-cache gcc make musl-dev
```

Build it:

```sh
cd examples/demo
pkgsmith build        # the packages go to ./dist
```

Each `matrix` entry is one package:

- `image` is the container the entry builds in.
- `setup` runs once in that image and the result is cached. It can be one command or a full script.
- `build` runs in the image with your source at `/src`. pkgsmith records the files it installs and packages them.
  Entries with the same image and scripts share one build.

To choose the files yourself, add a `contents` list. See [contents](docs/contents.md). pkgsmith packages itself with
[pkgsmith.yaml](pkgsmith.yaml).

## Commands

| Command           | What it does                                                  |
| ----------------- | ------------------------------------------------------------- |
| `build`           | Builds every package and writes it to `dist`                  |
| `plan`            | Shows each package and its file name. Starts no container     |
| `files`           | Lists the files each package would carry                      |
| `formats`         | Lists the formats the installed plugins write                 |
| `plugins`         | Lists the loaded plugins and the folders searched for them    |
| `prune`           | Removes the cached setup images                               |

Run `pkgsmith plan` after you change a matrix. Jobs with the same `BUILD` letter share one build. Every option is in
[command line](docs/cli.md).

## Documentation

| Page                                              | Covers                                          |
| ------------------------------------------------- | ----------------------------------------------- |
| [Getting started](docs/getting-started.md)        | A package from scratch, step by step            |
| [Manifest](docs/manifest.md)                      | Every key                                       |
| [Contents](docs/contents.md)                      | Files, trees, links and config files            |
| [Build environment](docs/build-environment.md)    | Images, setup, the tracked build, caching       |
| [Matrix and expressions](docs/matrix-and-expressions.md) | One manifest for many distros and architectures |
| [Versions](docs/versions.md)                      | How one version becomes each format's spelling  |
| [Install scripts](docs/scripts.md)                | What runs when, per format                      |
| [Dependencies](docs/dependencies.md)              | Names, version constraints, per-distro names    |
| [Signing](docs/signing.md)                        | Keys, `--sign-key`, and checking a signature    |
| [Plugins](docs/plugins.md)                        | Where plugins live, and how to write one        |
| [Command line](docs/cli.md)                       | Every command and option                        |

Every file in [examples](examples) is a complete manifest. [examples/full.yaml](examples/full.yaml) uses every key and
comments each one.

## Limits

- pkgsmith does not find the shared libraries a program needs. List them in `depends`.
- Builds run for `linux/amd64` and `linux/arm64`, and only with Podman. Docker is not supported.
- Packages are signed only when you give `--sign-key`. See [signing](docs/signing.md).
- There is no built-in support for a language. Install the toolchain in `setup` and run its build in `build`.
  [examples/rust-cli.yaml](examples/rust-cli.yaml) does this for Cargo.

## Build and test

```sh
cargo build --workspace
cargo test --workspace
```

## Licence

MIT
