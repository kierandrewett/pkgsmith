# Getting started

This page builds a small C program into packages for four distributions and installs them.

## Requirements

- Linux and [Podman](https://podman.io/). Rootless Podman is enough. It was tested with Podman 5.8.4.
- A Rust toolchain, to build pkgsmith: `cargo build --release --workspace`. The binary is `target/release/pkgsmith`.
  The formats are plugins, and the same command builds them next to the binary, where pkgsmith finds them.

pkgsmith talks to Podman through its API socket. It uses `$XDG_RUNTIME_DIR/podman/podman.sock` when that socket
answers. Otherwise it starts `podman system service` for the length of the run and stops it afterwards. You do not
have to enable `podman.socket`.

## The project

Make a folder with three files.

`hello.c`:

```c
#include <stdio.h>
int main(void) { puts("hello from a tracked build"); return 0; }
```

`Makefile`:

```make
PREFIX ?= /usr/local
hello: hello.c
	cc -O2 -o hello hello.c
install: hello
	install -Dm755 hello $(DESTDIR)$(PREFIX)/bin/hello
	install -Dm644 hello.c $(DESTDIR)$(PREFIX)/share/hello/hello.c
	install -Dm644 /dev/null $(DESTDIR)/etc/hello.conf
```

`pkgsmith.yaml`:

```yaml
name: hello
version: 1.0.0
description: A demo that packages a small C program
maintainer: Your Name <you@example.com>
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
      setup: apt-get update -qq && apt-get install -y gcc make
    - format: archlinux
      distro: arch
      image: archlinux:base
      setup: pacman -Sy --noconfirm gcc make
    - format: apk
      distro: alpine
      image: alpine:3.22
      setup: apk add --no-cache gcc make musl-dev
```

Each `matrix` entry is one package. It names the format, the distro, the image to build in, and the `setup` that image
needs. The `build` is the same for all of them.

## Plan

```console
$ pkgsmith plan
  #  FORMAT     DISTRO  ARCH   IMAGE           BUILD  FILE
  1  rpm        fedora  amd64  fedora:43       a      hello-1.0.0-1.x86_64.rpm
  2  deb        debian  amd64  debian:13       b      hello_1.0.0-1_amd64.deb
  3  archlinux  arch    amd64  archlinux:base  c      hello-1.0.0-1-x86_64.pkg.tar.zst
  4  apk        alpine  amd64  alpine:3.22     d      hello-1.0.0-r1.apk

4 packages from 4 builds.
```

`plan` shows each package and the file it would write, without starting a container. Jobs with the same `BUILD`
letter share one build. If `plan` says that no plugin was found, run `pkgsmith plugins`: it lists the folders that
pkgsmith searches. See [plugins](plugins.md).

## Build

```console
$ pkgsmith build -j 4
```

On a terminal you see one lane per distro. Each lane shows its steps, a spinner, the time of each step, and the last
lines of output of the step that runs. A step whose result is cached shows `CACHED`. The first run downloads the
images and installs the compilers. The next run reuses the setup images and takes a few seconds.

When the build ends, the packages are in `./dist`:

```console
Packages in dist:
  rpm        hello-1.0.0-1.x86_64.rpm  (4.2 KiB)
  deb        hello_1.0.0-1_amd64.deb  (2.7 KiB)
  archlinux  hello-1.0.0-1-x86_64.pkg.tar.zst  (2.8 KiB)
  apk        hello-1.0.0-r1.apk  (3.9 KiB)
```

## Look inside

```console
$ pkgsmith files --format rpm
== rpm [distro=fedora, format=rpm, image=fedora:43]
644 root:root /etc/hello.conf
755 root:root /usr/bin/hello
755 root:root /usr/share/hello
644 root:root /usr/share/hello/hello.c
4 files, 12604 bytes installed
```

Four entries: the three files that `make install` wrote, and the new folder `/usr/share/hello`. The build also
changed `/usr`, `/usr/share` and other folders that already existed. pkgsmith does not claim those.

## Install

Each package installs in the distribution it was built for.

| Distro | Command                                              |
| ------ | ---------------------------------------------------- |
| Fedora | `sudo dnf install ./dist/hello-1.0.0-1.x86_64.rpm`   |
| Debian | `sudo dpkg -i ./dist/hello_1.0.0-1_amd64.deb`        |
| Arch   | `sudo pacman -U ./dist/hello-1.0.0-1-x86_64.pkg.tar.zst` |
| Alpine | `apk add --allow-untrusted ./dist/hello-1.0.0-r1.apk` |

Then run `hello`.

## Next

- Mark `/etc/hello.conf` as a configuration file, and set install scripts: see
  [contents](contents.md) and [scripts](scripts.md).
- Add dependencies: see [dependencies](dependencies.md).
- Build for arm64 as well: see [matrix and expressions](matrix-and-expressions.md).
