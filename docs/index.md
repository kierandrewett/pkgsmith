# pkgsmith

pkgsmith builds distribution packages. You write one script for a package. pkgsmith runs it on your system, builds your
program there, and writes the package in the format you ask for.

![One script, one package for each system](media/demo.gif)

```python
# pkgsmith.star
sh("make")
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

package("rpm", name = "hello", version = "1.0.0", description = "A small program")
```

```console
$ pkgsmith build
```

This runs `make` in the folder of the script, takes every file that `make install` put in `$pkgdir`, and writes
`hello-1.0.0-1.x86_64.rpm` to `./dist`.

## Why

A project that ships to several distributions usually keeps a spec file, a `PKGBUILD`, a `debian/` folder and more.
They list the same files, the same dependencies and the same scripts, each in its own format. pkgsmith keeps that
information in one place.

| You write                                         | pkgsmith does                                                  |
| ------------------------------------------------- | -------------------------------------------------------------- |
| `sh(...)` for each build command                  | Runs them on this system, in order                             |
| `expose(...)` for the files the build made        | Offers them to every package that follows                      |
| `package(format, ...)` with `name`, `version`, `depends`, `scripts` | Writes them in the way each format wants them  |
| `if` and `for`, with `host()`                     | Writes only the packages that fit this system                  |

## How it works

1. **Read.** pkgsmith runs the script, top to bottom. Each `package` call that runs adds one package to the list.
2. **Build.** It runs the `sh` commands that each package needs, in the folder of the script. Packages that follow the
   same commands share one build.
3. **Collect.** The files in `$pkgdir` and the files you `expose` are the files of the package.
4. **Package.** pkgsmith sends the file list and the metadata to the plugin that writes the format.

For another distro or architecture, run pkgsmith on that system, for example in a CI
job. See [building](building.md#packages-for-other-distros).

## Formats are plugins

Every format is a shared library named `pkgsmith-plugin-*.so`. pkgsmith loads the ones it finds, and each one adds
formats. You can write your own in Rust, C or any language that exports a C function. See [plugins](plugins.md).

The plugins in this repository write:

- `rpm`, `deb` and `ipk`
- `archlinux` and `apk`
- `tar` and `zip`
- and more: see [formats](formats.md)

## Read next

- [Getting started](getting-started.md): build a package from scratch.
- [Scripting](scripting.md): how to write a script.
- [Script reference](manifest.md): every function, field and value.
- [Building](building.md): how the build runs, `$pkgdir`, and packages for other distros.
- [Contents](contents.md): choose the files yourself.
- [Versions](versions.md), [install scripts](scripts.md) and [dependencies](dependencies.md).
- [macOS and Windows](macos-and-windows.md): compile there, then package.
- [Plugins](plugins.md): where pkgsmith looks for them, and how to write one.
