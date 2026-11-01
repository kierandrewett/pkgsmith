# Getting started

This page turns a small C program into packages and installs one. You build the program, install it into a prefix, and
describe the packages in a script.

## Requirements

- A Rust toolchain, to build pkgsmith: `cargo build --release --workspace`. The binary is `target/release/pkgsmith`.
  The formats are plugins, and the same command builds them next to the binary, where pkgsmith finds them.
- The tools your own build calls. This example needs a C compiler and `make`. pkgsmith itself runs no build.

pkgsmith runs on Linux, macOS and Windows. It needs no daemon.

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

Make a folder with two files.

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

## Make a prefix

Install the program into a folder named `prefix`. This is your build, not pkgsmith's:

```console
$ make install PREFIX=/usr DESTDIR="$PWD/prefix"
$ find prefix -type f
prefix/etc/hello.conf
prefix/usr/bin/hello
prefix/usr/share/hello/hello.c
```

The prefix is laid out like an installed system. See [the prefix](prefix.md) for the rules, and for CMake, Meson, Cargo
and Python.

## Write the script

Add `pkgsmith.star` next to the Makefile:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A demo that packages a small C program",
    "maintainer": "Your Name <you@example.com>",
    "license": "MIT",
}

expose(tree("/", "/"))

for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(fmt, config_files = ["/etc/hello.conf"], **common)
```

pkgsmith runs this script from top to bottom.

- `expose(tree("/", "/"))` offers the whole prefix to the packages that follow.
- Each `package` call that runs writes one package. The loop writes six, from the same prefix, on any system: the
  plugins write the files of every format.
- `config_files` marks `/etc/hello.conf` as a configuration file, which the package manager keeps on upgrade.

The script is code, so you can use conditions, loops and functions. See [scripting](scripting.md).

## Plan

```console
$ pkgsmith plan
  #  FORMAT     DISTRO  ARCH   FILE
  1  rpm                amd64  hello-1.0.0-1.x86_64.rpm
  2  deb                amd64  hello_1.0.0-1_amd64.deb
  3  archlinux          amd64  hello-1.0.0-1-x86_64.pkg.tar.zst
  4  apk                amd64  hello-1.0.0-r1.apk
  5  tar                amd64  hello-1.0.0-amd64.tar.xz
  6  zip                amd64  hello-1.0.0-amd64.zip

6 packages.
```

`plan` runs the script and writes nothing. It shows each package and the file it would write. The `ARCH` is the
architecture of your system, unless the script sets `arch`.

If `plan` says that no plugin was found, run `pkgsmith plugins`: it lists the folders that pkgsmith searches. See
[plugins](plugins.md).

## Build

```console
$ pkgsmith build
```

pkgsmith prints a line for each package while it writes, and then the list. The packages are in `./dist`:

```console
Packages in dist:
  rpm        hello-1.0.0-1.x86_64.rpm  (4.2 KiB)
  deb        hello_1.0.0-1_amd64.deb  (2.9 KiB)
  archlinux  hello-1.0.0-1-x86_64.pkg.tar.zst  (3.0 KiB)
  apk        hello-1.0.0-r1.apk  (3.0 KiB)
  tar        hello-1.0.0-amd64.tar.xz  (2.2 KiB)
  zip        hello-1.0.0-amd64.zip  (2.8 KiB)
```

`-F rpm,deb` writes only some formats. `-o DIR` changes the folder. `--prefix DIR` names another prefix.

## Look inside

```console
$ pkgsmith files -F rpm
== rpm [arch=amd64, format=rpm, name=hello]
644 root:root /etc/hello.conf
755 root:root /usr/bin/hello
755 root:root /usr/share/hello
644 root:root /usr/share/hello/hello.c
4 files, 12597 bytes installed
```

`files` lists the paths, modes and owners that a package would carry, and writes no package. Use it to check a script
before you build.

## Install a package

Each package installs on the system of its format. Write the packages once, install them anywhere.

| Format      | Command                                                   |
| ----------- | --------------------------------------------------------- |
| `rpm`       | `sudo dnf install ./dist/hello-1.0.0-1.x86_64.rpm`        |
| `deb`       | `sudo dpkg -i ./dist/hello_1.0.0-1_amd64.deb`             |
| `archlinux` | `sudo pacman -U ./dist/hello-1.0.0-1-x86_64.pkg.tar.zst`  |
| `apk`       | `apk add --allow-untrusted ./dist/hello-1.0.0-r1.apk`     |

Then run `hello`.

## Next

- Choose the files yourself, and set install scripts: see [contents](contents.md) and [scripts](scripts.md).
- Add dependencies that differ by distro: see [dependencies](dependencies.md).
- Package a program built for another architecture or libc: see [the prefix](prefix.md#other-architectures-and-c-libraries).
- List every field: see the [script reference](manifest.md).
