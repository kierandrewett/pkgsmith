# pkgsmith

pkgsmith builds distribution packages. You write one manifest for a package. pkgsmith runs the build in a
container, finds the files the build installed, and writes the package in each format you ask for.

```yaml
# pkgsmith.yaml
name: hello
version: 1.0.0
description: A small program
image: fedora:43
setup: dnf -y install gcc make
build: |
  make
  make install PREFIX=/usr
```

```console
$ pkgsmith build .
```

The command builds `hello` in a Fedora container, takes every file that `make install` wrote outside the
source folder, and writes `hello-1.0.0-1.x86_64.rpm` to `./dist`.

## Why

A project that ships to several distributions usually keeps a spec file, a `PKGBUILD`, a `debian/` folder and more.
They list the same files, the same dependencies and the same scripts, each in its own format. pkgsmith keeps that
information in one place.

| You write                                 | pkgsmith does                                                              |
| ----------------------------------------- | -------------------------------------------------------------------------- |
| An `image`, a `setup` and a `build`       | Runs them in a container and tracks which files the build installed        |
| `name`, `version`, `depends`, `scripts`   | Writes them in the way each format wants them                              |
| A `matrix` of distros, architectures, formats | Builds every combination, side by side, with `-j`                      |

## How it works

1. **Read.** pkgsmith reads the manifest and expands the matrix into jobs, like a GitHub Actions workflow.
2. **Set up.** For a job with an `image`, it runs the `setup` script in a container once and keeps the result as an
   image. The next run starts from that image.
3. **Build.** It runs the `build` script on top of that image. The source folder is mounted at `/src` as a
   throw-away overlay, so the build can write there without changing your checkout.
4. **Track.** The container's changes against its image are the files the build installed. pkgsmith exports them.
5. **Package.** Each format writer turns the file list and the metadata into a package file.

Builds that need the same image, setup, script and environment run once, even when you ask for five formats.

## Formats

| Format      | File                | Status  | Page                                  |
| ----------- | ------------------- | ------- | ------------------------------------- |
| `tar`       | `.tar.xz`           | works   | [tar](formats/tar.md)                 |
| `deb`       | `.deb`              | works   | [deb](formats/deb.md)                 |
| `rpm`       | `.rpm`              | works   | [rpm](formats/rpm.md)                 |
| `archlinux` | `.pkg.tar.zst`      | works   | [archlinux](formats/archlinux.md)     |
| `apk`       | `.apk` (Alpine)     | works   | [apk](formats/apk.md)                 |
| `ipk`       | `.ipk` (OpenWrt)    | works   | [ipk](formats/ipk.md)                 |
| `snap`      | `.snap`             | planned | [planned formats](formats/planned.md) |
| `flatpak`   | `.flatpak`          | planned | [planned formats](formats/planned.md) |
| `nix`       | Nix expression      | planned | [planned formats](formats/planned.md) |

## Read next

- [Getting started](getting-started.md): build a package from scratch and install it.
- [Manifest reference](manifest.md): every key.
- [Build environment](build-environment.md): images, setup, build, tracking, caching.
- [Matrix and expressions](matrix-and-expressions.md): one manifest for many distros and architectures.
- [Command line](cli.md): every command and option.
- [Architecture](architecture.md): how the pieces fit, and how to add a format.
- [Roadmap](roadmap.md): what is not done.

![The progress output of a build with four lanes](media/build.cast)

A recording of a four-lane build is in [media/build.cast](media/build.cast). Play it with
`asciinema play docs/media/build.cast`.
