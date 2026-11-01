# macOS and Windows

The `setup` and `build` steps of a manifest run in a Linux container. They do not run on macOS or Windows. pkgsmith
provides no macOS or Windows toolchain, so a build that uses only the tools in the image makes Linux programs.

pkgsmith itself runs natively on Linux, macOS and Windows. This page says how to package a program for macOS or Windows.

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

The `pkgsmith.yaml` of this repository packages pkgsmith for Linux and macOS in one file. The Linux jobs name an `image`
and run a build. The macOS jobs work differently:

- They name no `image`, so they run no `setup` or `build`. The top-level `build` is not used for them.
- They pack the files in `STAGE`. A `tree` entry that only applies to `dmg` adds those files.
- An `if` on the matrix entry drops them when you do not pass `STAGE`. A plain `pkgsmith build` then builds only the
  Linux packages.

```yaml
matrix:
  include:
    - { format: dmg, arch: arm64, distro: macos, if: env.STAGE }
contents:
  - { type: tree, src: "${STAGE:-stage}", dst: /pkgsmith, only: [dmg] }
dmg:
  volume_name: pkgsmith
```

Compile on a Mac, put the program and its files in a folder, and name that folder with `STAGE`:

```console
$ pkgsmith build -F dmg --job arch=arm64 --build-arg STAGE=stage
```

The package is written to `./dist`. You should see a file such as `pkgsmith-0.1.0-1-arm64.dmg`. Use `-F dmg` to build
the disk image only.

## Formats

| System  | Format | Notes                                                                                           |
| ------- | ------ | ----------------------------------------------------------------------------------------------- |
| macOS   | `dmg`  | A disk image of your files. See [dmg](formats.md#dmg)                                           |
| Windows | `msi`  | A Windows Installer package. Written unsigned. See [msi](formats.md#msi)                        |
| Windows | `nsis` | A setup `.exe`. Needs `makensis` on the machine that builds. See [nsis](formats.md#nsis)        |
| Windows | `msix` | Written unsigned. Windows installs it only after you sign it. See [msix](formats.md#msix)       |

The `dmg`, `msi` and `msix` writers are pure Rust and run on any system. `nsis` runs `makensis`.

These formats pack the files you give them and do not check the target. A program that you compile for Linux is a
Linux program, so do not put it in one of these packages. A cross-compiler can make files for another system, but you
must supply that toolchain and test the result.
