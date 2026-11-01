# pkgsmith

pkgsmith builds distribution packages. You write one manifest for a package. pkgsmith runs the build in a
container for each distro, finds the files the build installed, and writes the package.

![One build for Fedora, Debian, Arch and Alpine](media/demo.gif)

```yaml
# pkgsmith.yaml
name: hello
version: 1.0.0
description: A small program
targets:
  - format: rpm
    distro: fedora
jobs:
  fedora:
    runs-on: fedora:43
    setup:
      - run: dnf -y install gcc make
    steps:
      - run: make && make install PREFIX=/usr
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
| A `job` for each build environment: a `runs-on` image and its `steps` | Runs them in a container and tracks which files the build installed |
| `name`, `version`, `depends`, `scripts`   | Writes them in the way each format wants them                              |
| A `matrix` of distros, architectures, formats | Builds every combination, side by side, with `-j`                      |

## How it works

1. **Read.** pkgsmith reads the manifest and expands the matrix into jobs, like a GitHub Actions workflow.
2. **Set up.** For a target that needs a job, it runs the `setup` steps in a container once and keeps the result as an
   image. The next run starts from that image.
3. **Build.** It runs the `steps` on top of that image. The source folder is mounted at `/src` as a
   throw-away overlay, so the build can write there without changing your checkout.
4. **Track.** The container's changes against its image are the files the build installed. pkgsmith exports them.
5. **Package.** pkgsmith sends the file list and the metadata to the plugin that writes the format.

Targets that need the same job run its steps once, even when you ask for five formats or several distros.

## Formats are plugins

Every format is a shared library named `pkgsmith-plugin-*.so`. pkgsmith loads the ones it finds, and each one adds
formats. You can write your own in Rust, C or any language that exports a C function. See [plugins](plugins.md).

The plugins in this repository write:

- `rpm`, `deb` and `ipk`
- `archlinux` and `apk`
- `tar` and `zip`

## Read next

- [Getting started](getting-started.md): build a package from scratch.
- [Manifest reference](manifest.md): every key.
- [Contents](contents.md): choose the files yourself.
- [Build environment](build-environment.md): jobs, steps, tracking, caching.
- [Matrix and expressions](matrix-and-expressions.md): one manifest for many distros and architectures.
- [Versions](versions.md), [install scripts](scripts.md) and [dependencies](dependencies.md).
- [Plugins](plugins.md): where pkgsmith looks for them, and how to write one.
