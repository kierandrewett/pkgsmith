# pkgsmith

pkgsmith writes distribution packages from files you already built. You install your program into a folder, the
**prefix**, and write a short script that describes the packages made from it. pkgsmith writes them in every format you
list. It builds nothing and runs no command.

```console
$ make install PREFIX=/usr DESTDIR="$PWD/prefix"    # your build, your tools
$ pkgsmith build                                    # reads ./prefix, writes ./dist
```

```python
# pkgsmith.star
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(fmt, **common)
```

This writes six packages, such as `hello-1.0.0-1.x86_64.rpm`, to `./dist`, from the same prefix.

## How it compares

pkgsmith is in the same space as [nfpm](https://nfpm.goreleaser.com) and [fpm](https://fpm.readthedocs.io): tools that
turn files into packages without building anything. All three take files you already built.

- **A script, not a YAML file.** nfpm describes a package in YAML. pkgsmith uses a script in Starlark, so a list of
  formats is a `for` loop, a dependency name that differs by format is a dict, and a version can come from a file in the
  prefix. The script cannot run commands or reach the network.
- **More formats.** Besides rpm, deb, Arch, Alpine and OpenWrt, pkgsmith writes Slackware, Void, FreeBSD, snap, OCI,
  AppImage, wheel, gem, npm, conda, NuGet, dmg, Flatpak, Nix, msi, NSIS, msix, appx, tar and zip. See [formats](formats.md).
  Every format is a plugin, and you can write your own.
- **Signing.** pkgsmith signs with gpg, an OpenPGP key file or an RSA key. See [signing](signing.md).
- **No external tools.** pkgsmith writes the files itself, and does not call `rpmbuild`, `dpkg-deb` or `makepkg`. Two
  formats need a tool: `nsis` runs `makensis`, and `flatpak` runs `flatpak`.

nfpm and fpm are older and widely used, and pkgsmith is new. Not every format has been checked with the package manager
of a real target system. [Formats](formats.md) says which checks ran for each. fpm can also read other package types
and convert them. pkgsmith does not.

## Why

A project that ships to several distributions usually keeps a spec file, a `PKGBUILD`, a `debian/` folder and more.
They list the same files, the same dependencies and the same scripts, each in its own format. pkgsmith keeps that
information in one place.

| You write                                               | pkgsmith does                                       |
| ------------------------------------------------------- | --------------------------------------------------- |
| A build that installs into a prefix                     | Nothing. The build is yours                         |
| `include(...)` and `exclude(...)`, to narrow it         | Packs the whole prefix by default, then cuts it down |
| `package(format, ...)` with `name`, `version`, `depends`, `scripts` | Writes them the way each format wants them |
| `if` and `for`, with `arg()` and the prefix             | Writes only the packages that you describe          |

## How it works

1. **Read.** pkgsmith runs the script, top to bottom. Each `package` call that runs adds one package to the list.
2. **Collect.** A package holds the whole prefix. `include` and `exclude` narrow it, `expose` offers only the files you
   list, and the `contents` of the call add files. pkgsmith reads the files from the prefix.
3. **Package.** pkgsmith sends the file list and the metadata to the plugin that writes the format.

A package is a file, so every system can write every format. What depends on a system is the content of the prefix:
its architecture and its C library. See [the prefix](prefix.md#other-architectures-and-c-libraries).

## Formats are plugins

Every format is a shared library named `pkgsmith-plugin-*.so`. pkgsmith loads the ones it finds, and each one adds
formats. You can write your own in Rust, C or any language that exports a C function. See [plugins](plugins.md).

## Read next

- [Getting started](getting-started.md): write a package from scratch.
- [The prefix](prefix.md): make a prefix with make, CMake, Meson, Cargo or Python.
- [Scripting](scripting.md): how to write a script.
- [Script reference](manifest.md): every function, field and value.
- [Contents](contents.md): choose the files yourself.
- [Versions](versions.md), [install scripts](scripts.md) and [dependencies](dependencies.md).
- [Formats](formats.md): the options of every format.
- [Signing](signing.md): keys and checking a signature.
- [macOS and Windows](macos-and-windows.md): build there, then pack the prefix.
- [Command line](cli.md): every command and option.
- [Plugins](plugins.md): where pkgsmith looks for them, and how to write one.
- [Nix research note](research/nix.md): the options for Nix packages, and why the `nix` formats look as they do.
