# pkgsmith

pkgsmith builds distribution packages from one manifest. You write the package name, the files, the install scripts
and the dependencies once. pkgsmith builds your program inside a container for each distro and writes a `.rpm`,
`.deb`, `.pkg.tar.zst`, `.apk`, `.ipk`, `.tar.xz` or `.zip`.

![pkgsmith builds one package for Fedora, Debian, Arch and Alpine in a single run](docs/media/demo.svg)

The recording shows `examples/demo`: a manifest, a Makefile and a C file. One `pkgsmith build` makes four containers
(Fedora, Debian, Arch, Alpine), runs `make install` in each, and writes five packages. The cast is in
[docs/media/demo.cast](docs/media/demo.cast). Play it with `asciinema play docs/media/demo.cast`.

The manifest works like a GitHub Actions workflow. A `matrix` runs the build once for each combination, and
`${{ }}` expressions and `if:` see that combination.

## Quick start

You need [Podman](https://podman.io/) running (the user socket is enough) and a Rust toolchain to build pkgsmith.

```sh
cargo install --path .
cd examples/demo
pkgsmith build        # packages go to ./dist
```

`examples/demo/pkgsmith.yaml`:

```yaml
name: hello
version: 1.0.0
description: A tracked-build demo
maintainer: Kieran Drewett <kieran@drewett.dev>
license: MIT

matrix:
  include:
    - { distro: fedora, image: "fedora:43", format: rpm }
    - { distro: debian, image: "debian:13", format: deb }
    - { distro: debian, image: "debian:13", format: tar }
    - { distro: arch, image: "archlinux:base", format: archlinux }
    - { distro: alpine, image: "alpine:3.22", format: apk }

image: ${{ matrix.image }}
setup: |                    # runs once per image, and the result is cached
  . /etc/os-release
  case "$ID" in
    fedora) dnf -y install gcc make ;;
    debian) apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y gcc make ;;
    arch)   pacman -Sy --noconfirm gcc make ;;
    alpine) apk add --no-cache gcc make musl-dev ;;
  esac
build: |                    # pkgsmith records the files this installs and packages them
  make
  make install PREFIX=/usr
```

There is no `contents` list. pkgsmith watches what `make install` writes and packages that. To choose the files
yourself, see [docs/contents.md](docs/contents.md). More in [docs/getting-started.md](docs/getting-started.md).

## Commands

```sh
pkgsmith plan                           # a table of what would be built, no containers started
pkgsmith plan --json                    # the same for scripts
pkgsmith build                          # build everything
pkgsmith build -j 4                     # four builds at the same time
pkgsmith build --format deb --job arch=arm64
pkgsmith build --build-arg VERSION=0.4.2 -o out
pkgsmith files                          # list the files each package would carry
pkgsmith formats
pkgsmith prune                          # remove the cached set-up images
```

Run `pkgsmith plan` first when you change a matrix. It shows each file name, and a `BUILD` letter that marks the
jobs that share one build, so one `make` serves both a `.deb` and a `.tar.xz`.

## Formats

| Format      | File                      | Status  |
| ----------- | ------------------------- | ------- |
| `rpm`       | `.rpm`                    | works   |
| `deb`       | `.deb`                    | works   |
| `archlinux` | `.pkg.tar.zst`            | works   |
| `apk`       | `.apk` (Alpine)           | works   |
| `ipk`       | `.ipk` (OpenWrt)          | works   |
| `tar`       | `.tar.xz`, `.gz`, `.zst`, `.bz2` | works |
| `zip`       | `.zip`                    | works   |

pkgsmith writes each format itself. It does not call `rpmbuild`, `dpkg-deb`, `makepkg` or `abuild`. The packages were
checked by installing them: rpm on Fedora 43 and openSUSE Tumbleweed, deb on Debian 13, Arch with `pacman -Qkk`, apk on
Alpine with apk-tools 3.

## Documentation

* [Getting started](docs/getting-started.md)
* [Manifest reference](docs/manifest.md)
* [Contents: files, trees, links and config files](docs/contents.md)
* [Build environment: images, set-up, the tracked build](docs/build-environment.md)
* [Matrix and expressions](docs/matrix-and-expressions.md)
* [Versions and nightly builds](docs/versions.md)
* [Install scripts](docs/scripts.md)
* [Dependencies](docs/dependencies.md)

Every file in [examples/](examples/) is a complete manifest. [examples/full.yaml](examples/full.yaml) uses every key
and has a comment on each one.

## Limits

* pkgsmith does not find the shared libraries a program needs. List them in `depends`.
* Builds run for `linux/amd64` and `linux/arm64`.
* Package signing is not done yet.

## Build and test

```sh
cargo build --release
cargo test
```

## Licence

MIT
