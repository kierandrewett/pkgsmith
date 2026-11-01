# macOS and Windows

The steps of a job run in a Linux container. They do not run on macOS or Windows. pkgsmith
provides no macOS or Windows toolchain, so a build that uses only the tools in the image makes Linux programs.

pkgsmith itself runs natively on Linux, macOS and Windows. This page says how to package a program for macOS or Windows.

## Why

A container is Linux, and the files its build installs are what pkgsmith packages. A cross-compiler that you install in
a setup step can make files for another system, but you must supply and test that toolchain. pkgsmith does not set it up
or check the result.

## What to do

pkgsmith packages files that already exist. It does not care where they came from. Use one of these ways.

- **Compile on the target system, then package there.** Build your program on a Mac or on Windows with its own
  toolchain, as the release workflow of pkgsmith does. Run pkgsmith on the same machine with targets that have no `job`. This is the way to prefer, because
  the program is built and tested on the system it runs on.
- **Package on Linux.** Copy the files you compiled for macOS or Windows to a Linux machine, or download them from CI.
  Run pkgsmith there with targets that have no `job`. The `dmg` format needs no other tool. A format that runs an
  external program, such as `flatpak`, needs that program on the machine that builds.

A target without a `job` runs nothing. It packages the files that `contents` names, so it needs no Podman on any
system. See the [script reference](manifest.md) and [contents](contents.md).

## Example

The `pkgsmith.star` of this repository packages pkgsmith for Linux, macOS and Windows in one file. The Linux targets
have a job and run its steps. The macOS and Windows targets work differently:

- They have no `job`, so they run no steps.
- They pack the files in `STAGE`. A `tree` entry in `contents` adds those files.
- An `if STAGE:` around them drops them when you do not pass `STAGE`. A plain `pkgsmith build` then builds only the
  Linux packages.

```python
pkg = package(name = "hello", version = "1.0.0", description = "A small program")
STAGE = arg("STAGE")

if STAGE:
    for arch in ["amd64", "arm64"]:
        target(
            pkg, "dmg", distro = "macos", arch = arch,
            contents = [tree(STAGE, "/hello")],
            options = {"volume_name": "hello"},
        )
```

Compile on a Mac, put the program and its files in a folder, and name that folder with `STAGE`:

```console
$ pkgsmith build -F dmg --where arch=arm64 --build-arg STAGE=stage
```

The package is written to `./dist`. You should see a file such as `hello-1.0.0-1-arm64.dmg`. Use `-F dmg` to build
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
