# macOS and Windows

The `setup` and `build` steps of a manifest run in a Linux container. They do not run on macOS or Windows. pkgsmith
provides no macOS or Windows toolchain, so a build that uses only the tools in the image makes Linux programs. pkgsmith
itself runs natively on Linux, macOS and Windows. This page says how to package a program for macOS or Windows.

## Why

A container is Linux, and the files its build installs are what pkgsmith packages. A cross-compiler that you install in
`setup` can make files for another system, but you must supply and test that toolchain. pkgsmith does not set it up
or check the result.

## What to do

pkgsmith packages files that already exist. It does not care where they came from. Use one of these ways.

- **Compile on the target system, then package there.** Build your program on a Mac or on Windows with its own
  toolchain, as the release workflow of pkgsmith does. Run pkgsmith on the same machine with a manifest that has no `image`. This is the way to prefer, because
  the program is built and tested on the system it runs on.
- **Package on Linux.** Copy the files you compiled for macOS or Windows to a Linux machine, or download them from CI.
  Run pkgsmith there with a manifest that has no `image`. The `dmg` format needs no other tool. A format that runs an
  external program, such as `flatpak`, needs that program on the machine that builds.

A manifest without an `image` runs nothing. It packages the files that `contents` names, so it needs no Podman on any
system. See [manifest](manifest.md) and [contents](contents.md).

## Example

This manifest packs a folder of compiled files into a disk image. It is a short version of `pkgsmith-native.yaml` in
this repository, which packages pkgsmith itself on macOS in the release workflow.

```yaml
name: pkgsmith
version: ${VERSION:-0.1.0}
arch: ${ARCH:-arm64}
description: Build distribution packages from one manifest
contents:
  - { type: tree, src: "${STAGE:-stage}", dst: /pkgsmith }
dmg:
  volume_name: pkgsmith
```

Put the compiled program and its files in `stage/`, then run:

```console
$ pkgsmith build -f pkgsmith-native.yaml -F dmg --build-arg VERSION=0.1.0 --build-arg ARCH=arm64 --build-arg STAGE=stage
```

The package is written to `./dist`. You should see a file such as `pkgsmith-0.1.0-1-arm64.dmg`.

## Formats

| System  | Format | Notes                                                                                          |
| ------- | ------ | ---------------------------------------------------------------------------------------------- |
| macOS   | `dmg`  | A disk image of your files. See [dmg](formats.md#dmg)                                          |
| Windows | `msix` | Written unsigned. Windows installs it only after you sign it. See [msix](formats.md#msix)      |

The `.dmg` and `.msix` writers are pure Rust and run on any system. A program that you compile on Linux is a Linux
program, so do not put it in one of these packages.
