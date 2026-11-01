# Getting started

This page builds a small C program into packages and installs one.

## Requirements

- A Rust toolchain, to build pkgsmith: `cargo build --release --workspace`. The binary is `target/release/pkgsmith`.
  The formats are plugins, and the same command builds them next to the binary, where pkgsmith finds them.
- The tools your build calls. This example needs a C compiler and `make`.

pkgsmith runs on Linux, macOS and Windows. It needs no daemon. The script builds on the system
that runs pkgsmith, so you get packages for that system.

## Install

There is no published release yet, so build from source:

```console
$ git clone https://github.com/kierandrewett/pkgsmith
$ cd pkgsmith
$ cargo build --release --workspace
$ export PATH="$PWD/target/release:$PATH"
$ pkgsmith plugins
```

`pkgsmith plugins` lists the plugins that loaded. If it lists none, see [plugins](plugins.md#where-pkgsmith-looks).

## The project

Make a folder with three files.

`hello.c`:

```c
#include <stdio.h>
int main(void) { puts("hello from pkgsmith"); return 0; }
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

`pkgsmith.star`:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A demo that packages a small C program",
    "maintainer": "Your Name <you@example.com>",
    "license": "MIT",
}

sh("make")
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

native = host()["format"]
if native:
    package(native, **common)
package("tar", **common)
```

pkgsmith runs this script from top to bottom.

- `sh` adds a command to the build. Each one runs in the folder of the script.
- `$pkgdir` is an empty folder. The files that `make install` puts there become the package.
- `host()["format"]` is the package format of this system: `rpm` on Fedora, `deb` on Debian, `archlinux` on Arch, `apk`
  on Alpine. It is empty when pkgsmith does not know it.
- Each `package` call that runs writes one package. Both calls come after the same commands, so the build runs once.

The script is code, so you can use conditions, loops and functions. See [scripting](scripting.md).

## Plan

```console
$ pkgsmith plan
  #  FORMAT  DISTRO  ARCH   BUILD  FILE
  1  rpm     fedora  amd64  a      hello-1.0.0-1.x86_64.rpm
  2  tar     fedora  amd64  a      hello-1.0.0-amd64.tar.xz

2 packages from 1 builds. Jobs with the same BUILD letter share one build.
```

`plan` runs the script, but not the build. It shows each package and the file it would write. Packages with the same
`BUILD` letter share one build. The rows depend on your system: on Debian you see a deb instead of an rpm.

If `plan` says that no plugin was found, run `pkgsmith plugins`: it lists the folders that pkgsmith searches. See
[plugins](plugins.md).

## Build

```console
$ pkgsmith build
```

On a terminal you see one line for the build, with a spinner, the time and the last lines of its output. When it ends,
the packages are in `./dist`:

```console
Packages in dist:
  rpm  hello-1.0.0-1.x86_64.rpm  (4.2 KiB)
  tar  hello-1.0.0-amd64.tar.xz  (2.1 KiB)
```

## Look inside

```console
$ pkgsmith files --format rpm
== rpm [arch=amd64, distro=fedora, format=rpm, name=hello]
755 root:root /etc
644 root:root /etc/hello.conf
755 root:root /usr
755 root:root /usr/bin
755 root:root /usr/bin/hello
755 root:root /usr/share
755 root:root /usr/share/hello
644 root:root /usr/share/hello/hello.c
8 files, 12604 bytes installed
```

The package holds the three files that `make install` wrote, and the folders that lead to them, with the mode they have
in `$pkgdir`. `files` runs the build and writes no package.

## Install the package

Each package installs on the distribution it was built on.

| Distro | Command                                                   |
| ------ | --------------------------------------------------------- |
| Fedora | `sudo dnf install ./dist/hello-1.0.0-1.x86_64.rpm`        |
| Debian | `sudo dpkg -i ./dist/hello_1.0.0-1_amd64.deb`             |
| Arch   | `sudo pacman -U ./dist/hello-1.0.0-1-x86_64.pkg.tar.zst`  |
| Alpine | `apk add --allow-untrusted ./dist/hello-1.0.0-r1.apk`     |

Then run `hello`.

## Packages for other distros

To get a deb, run the same script on Debian. To get an rpm, run it on Fedora. In CI, use one job for each system. See
[building](building.md#packages-for-other-distros).

## Next

- Mark `/etc/hello.conf` as a configuration file, and set install scripts: see [contents](contents.md) and
  [scripts](scripts.md).
- Add dependencies that differ by distro: see [dependencies](dependencies.md).
- List every field: see the [script reference](manifest.md).
