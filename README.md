# pkgsmith

pkgsmith builds distribution packages from one manifest. You write the package name, the files, the install scripts
and the dependencies once. pkgsmith builds your program in a container for each distro and writes the package.

![pkgsmith builds one package for Fedora, Debian, Arch and Alpine in a single run](docs/media/demo.gif)

## Formats

| Format      | File                             | Status  |
| ----------- | -------------------------------- | ------- |
| `rpm`       | `.rpm`                           | works   |
| `deb`       | `.deb`                           | works   |
| `archlinux` | `.pkg.tar.zst`                   | works   |
| `apk`       | `.apk` (Alpine)                  | works   |
| `ipk`       | `.ipk` (OpenWrt)                 | works   |
| `tar`       | `.tar.xz`, `.gz`, `.zst`, `.bz2` | works   |
| `zip`       | `.zip`                           | works   |

pkgsmith writes each format itself. It does not call `rpmbuild`, `dpkg-deb`, `makepkg` or `abuild`. The packages were
checked by installing them: rpm on Fedora 43 and openSUSE Tumbleweed, deb on Debian 13, Arch with `pacman -Qkk`, apk on
Alpine with apk-tools 3.

## Install

You need [Podman](https://podman.io/) to build. A user socket is enough.

```sh
cargo binstall pkgsmith     # a release binary for Linux x86_64 or aarch64
cargo install pkgsmith      # or build it from source
```

## Quick start

```sh
git clone https://github.com/kierandrewett/pkgsmith
cd pkgsmith/examples/demo
pkgsmith build              # the packages go to ./dist
```

`examples/demo/pkgsmith.yaml` is the manifest in the recording above. A manifest has the package details, one `build`
for all distros, and a `matrix` with one entry for each package:

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

* `image` is the container the entry builds in.
* `setup` runs once in that image, and the result is cached. It can be one command or a full script.
* `build` runs in the image with your source folder at `/src`. pkgsmith records the files it installs and packages
  them. Entries that share an image and scripts share one build.
* To pick the files yourself, add a `contents` list. See [docs/contents.md](docs/contents.md).

pkgsmith packages itself. See [pkgsmith.yaml](pkgsmith.yaml).

## Commands

```sh
pkgsmith plan                           # a table of what would be built; starts no containers
pkgsmith plan --json                    # the same, for scripts
pkgsmith build                          # build every package
pkgsmith build -j 4                     # four builds at a time
pkgsmith build --format deb --job arch=arm64
pkgsmith build --build-arg VERSION=0.4.2 -o out
pkgsmith files                          # build, then list the files each package would carry
pkgsmith formats
pkgsmith prune                          # remove the cached setup images
```

Run `pkgsmith plan` after you change a matrix. It shows the file each job writes, and a `BUILD` letter that marks the
jobs that share one build.

## Documentation

* [Getting started](docs/getting-started.md)
* [Manifest reference](docs/manifest.md)
* [Contents: files, trees, links and config files](docs/contents.md)
* [Build environment: images, setup, the tracked build](docs/build-environment.md)
* [Matrix and expressions](docs/matrix-and-expressions.md)
* [Versions and nightly builds](docs/versions.md)
* [Install scripts](docs/scripts.md)
* [Dependencies](docs/dependencies.md)

Every file in [examples/](examples/) is a complete manifest. [examples/full.yaml](examples/full.yaml) uses every key
and has a comment on each one.

## Limits

* pkgsmith does not find the shared libraries a program needs. List them in `depends`.
* Builds run for `linux/amd64` and `linux/arm64`.
* Packages are not signed.
* There is no built-in support for a language. Install the toolchain in `setup` and run its build in `build`.
  [examples/rust-cli.yaml](examples/rust-cli.yaml) does this for Cargo.

## Build and test

```sh
cargo build --release
cargo test
```

## Licence

MIT
