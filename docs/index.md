# pkgsmith

pkgsmith builds distribution packages. You write one manifest for a package. pkgsmith runs the build in a
container for each distro, finds the files the build installed, and writes the package.

![One build for Fedora, Debian, Arch and Alpine](media/demo.gif)

```yaml
# pkgsmith.yaml
name: hello
version: 1.0.0
description: A small program
build: |
  make
  make install PREFIX=/usr
matrix:
  include:
    - format: rpm
      distro: fedora
      image: fedora:43
      setup: dnf -y install gcc make
```

```console
$ pkgsmith build
```

This builds `hello` in a Fedora container, takes every file that `make install` wrote outside the source folder,
and writes `hello-1.0.0-1.x86_64.rpm` to `./dist`.

## Why

A project that ships to several distributions usually keeps a spec file, a `PKGBUILD`, a `debian/` folder and more.
They list the same files, the same dependencies and the same scripts, each in its own format. pkgsmith keeps that
information in one place.

| You write                                 | pkgsmith does                                                              |
| ----------------------------------------- | -------------------------------------------------------------------------- |
| A `build`, and an `image` and `setup` for each distro | Runs them in a container and tracks which files the build installed |
| `name`, `version`, `depends`, `scripts`   | Writes them in the way each format wants them                              |
| A `matrix` of distros, architectures, formats | Builds every combination, side by side, with `-j`                      |

## How it works

1. **Read.** pkgsmith reads the manifest and expands the matrix into jobs, like a GitHub Actions workflow.
2. **Set up.** For a job with an `image`, it runs the `setup` script in a container once and keeps the result as an
   image. The next run starts from that image.
3. **Build.** It runs the `build` script on top of that image. The source folder is mounted at `/src` as a
   throw-away overlay, so the build can write there without changing your checkout.
4. **Track.** The container's changes against its image are the files the build installed. pkgsmith exports them.
5. **Package.** pkgsmith sends the file list and the metadata to the plugin that writes the format.

Builds that need the same image, setup, script and environment run once, even when you ask for five formats.

## Formats are plugins

Every format is a shared library named `pkgsmith-plugin-*.so`. pkgsmith loads the ones it finds and each one adds
formats. The plugins in this repository write `rpm`, `deb`, `ipk`, `archlinux`, `apk`, `tar` and `zip`. You can write
your own in Rust, C or any language that exports a C function. See [plugins](plugins.md).

## Read next

- [Getting started](getting-started.md): build a package from scratch.
- [Manifest reference](manifest.md): every key.
- [Contents](contents.md): choose the files yourself.
- [Build environment](build-environment.md): images, setup, build, tracking, caching.
- [Matrix and expressions](matrix-and-expressions.md): one manifest for many distros and architectures.
- [Versions](versions.md), [install scripts](scripts.md) and [dependencies](dependencies.md).
- [Plugins](plugins.md): where pkgsmith looks for them, and how to write one.
