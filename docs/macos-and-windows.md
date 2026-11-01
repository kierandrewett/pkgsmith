# macOS and Windows

pkgsmith runs natively on Linux, macOS and Windows, and its build runs on the system that runs it. A Linux system
builds Linux programs. pkgsmith provides no cross-compiler for macOS or Windows, so to package a program for them, you
compile it on that system, or bring files that you compiled elsewhere.

## What to do

pkgsmith packages files that already exist. It does not care where they came from. Use one of these ways.

- **Compile on the target system, then package there.** Build your program on a Mac or on Windows with its own
  toolchain, as the release workflow of pkgsmith does. Run pkgsmith on the same machine. This is the way to prefer,
  because the program is built and tested on the system it runs on. A script that calls `sh` runs on macOS as it
  does on Linux. On Windows, `sh` needs a POSIX shell on `PATH`, such as the one in Git for Windows.
- **Package on any system.** Copy the files you compiled for macOS or Windows to another machine, or download them from
  CI. Run pkgsmith there with a script that has no `sh` commands. The pure Rust formats need no other tool. A format that
  runs an external program, such as `nsis` or `flatpak`, needs that program on the machine that builds.

A package with no `sh` before it runs no build. It packages the files that `contents` names. See the
[script reference](manifest.md) and [contents](contents.md).

## Example

The `pkgsmith.star` of this repository packages pkgsmith for each system in one file. On Linux it builds and writes the
native package. For macOS and Windows it works differently:

- The packages come after no `sh` call, so they run no build.
- They pack the files in `STAGE`. A `tree` entry in `contents` adds those files.
- An `if STAGE:` around them drops them when you do not pass `STAGE`. A plain `pkgsmith build` then writes only the
  packages that build.

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}
STAGE = arg("STAGE")

if STAGE:
    package(
        "dmg", distro = "macos",
        contents = [tree(STAGE, "/hello")],
        options = {"volume_name": "hello"},
        **common
    )
```

Compile on a Mac, put the program and its files in a folder, and name that folder with `STAGE`:

```console
$ pkgsmith build -F dmg --build-arg STAGE=stage
```

The package is written to `./dist`. You should see a file such as `hello-1.0.0-1-arm64.dmg`. The `arch` of the package
is the architecture of the machine that runs pkgsmith. To package files for the other one, set `arch = "arm64"` or
`arch = "amd64"` in the call.

## Formats

| System  | Format | Notes                                                                                           |
| ------- | ------ | ----------------------------------------------------------------------------------------------- |
| macOS   | `dmg`  | A disk image of your files. See [dmg](formats.md#dmg)                                           |
| Windows | `msi`  | A Windows Installer package. Written unsigned. See [msi](formats.md#msi)                        |
| Windows | `nsis` | An installer `.exe`. Needs `makensis` on the machine that builds. See [nsis](formats.md#nsis)        |
| Windows | `msix` | Written unsigned. Windows installs it only after you sign it. See [msix](formats.md#msix)       |

The `dmg`, `msi` and `msix` writers are pure Rust and run on any system. `nsis` runs `makensis`.

These formats pack the files you give them and do not check the target. A program that you compile for Linux is a
Linux program, so do not put it in one of these packages.
