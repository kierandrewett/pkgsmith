# Build environment

A manifest with an `image` builds in a container. pkgsmith watches what the build installs and packages that.
This page explains each step and its limits.

```yaml
image: fedora:43                      # which container
setup: dnf -y install gcc make        # runs once, cached
build: |                              # runs on every build
  make
  make install PREFIX=/usr
```

## Requirements

- Podman. pkgsmith uses the Podman API through the `podman-api` crate. It does not run `podman` commands for builds.
- The socket: `$XDG_RUNTIME_DIR/podman/podman.sock` when it answers. Otherwise pkgsmith starts `podman system service`
  with a socket in `$XDG_RUNTIME_DIR/pkgsmith-*/` and stops it when the run ends.
- Docker is not supported. The overlay mount that protects your source folder is a Podman feature.

## `image`

Any container image. Use the distro you build for, because a program links against the libraries of the distro it was
built on. Set it at the top of the manifest when every build uses one image. Set it in a `matrix` entry when the images
differ:

```yaml
matrix:
  include:
    - { format: rpm, distro: fedora, image: "fedora:43", setup: dnf -y install gcc make }
    - { format: apk, distro: alpine, image: "alpine:3.22", setup: apk add --no-cache gcc make musl-dev }
```

A matrix entry may also set `setup` and `build`. A value in the entry replaces the top-level one for that package.

pkgsmith pulls an image that is not on the machine.

## `setup`

The setup script runs once per image and architecture. Put slow, stable work here: installing compilers and
libraries. After it ends, pkgsmith saves the container as a local image tagged
`localhost/pkgsmith-setup:<hash>`. The hash covers the base image, the architecture and the text of the script. Change
any of them and the next build runs the script again. Otherwise the step shows `CACHED`.

- Files that `setup` writes are **not** packaged.
- `--no-cache` runs every setup script again.
- `pkgsmith prune` removes the cached images.
- Builds that need the same setup image wait for the first one to finish it.

## `build`

The build script runs on top of the setup image.

- The build context is mounted at `/src`, and the script starts there. The context is the folder you pass to
  `pkgsmith build` (default `.`).
- `/src` is a **throw-away overlay**. The build can write files into it, such as a `build/` folder, and your checkout
  stays unchanged. The writes are dropped when the container is removed.
- The script runs as root in the container. It can install to `/usr`, `/etc` and `/opt` directly.

### How a script runs

| Script text                         | How it runs                          |
| ----------------------------------- | ------------------------------------ |
| Starts with `#!`, for example `#!/bin/bash` | As written, with that interpreter |
| Anything else                       | With `sh -e`, so the first failing command stops it |

`sh` is `dash` on Debian and `ash` on Alpine. Start the script with `#!/bin/bash` when you need bash.

### Environment

| Variable            | Value                                                                  |
| ------------------- | ---------------------------------------------------------------------- |
| `PKGSMITH_NAME`     | `name`                                                                 |
| `PKGSMITH_VERSION`  | `version`, as written in the manifest                                  |
| `PKGSMITH_RELEASE`  | `release`                                                              |
| `PKGSMITH_ARCH`     | `amd64` or `arm64`                                                     |
| `PKGSMITH_DISTRO`   | The matrix `distro`, or empty                                          |
| `MATRIX_<AXIS>`     | Every matrix value except `format`. `MATRIX_DISTRO`, `MATRIX_IMAGE`, `MATRIX_TRIPLE` ... |

The format is left out on purpose. A build must not depend on it, so the formats of one distro share one build.

### Architecture

A job with `arch: arm64` runs the container for `linux/arm64`. On an x86_64 host that needs `qemu-user` binfmt
support. Check with `podman run --rm --arch arm64 alpine uname -m`.

## Tracking

When the build script ends, pkgsmith asks Podman for the container's changes against its image. Each change is
`A` (added), `C` (changed) or `D` (deleted).

1. Deleted paths are ignored.
2. Paths under an excluded location are dropped. See [contents](contents.md) for the list and for `exclude`.
3. The container is exported as a tar stream, and only the kept members are unpacked into the work folder
   (`.pkgsmith/<name>-<hash>/stage` by default).
4. A new folder (`A`) becomes a folder entry of the package. A folder that already existed (`C`) does not.
5. Files under `/etc` are marked as configuration.

Then `contents` entries are applied on top. See [contents](contents.md).

### What this means for your build

- Install into the real prefix, for example `make install PREFIX=/usr`. You do not need `DESTDIR`.
- Do not write temporary files outside `/src` and `/tmp`. Everything else counts as installed.
- A file the build only reads is not packaged. A file the build rewrites with identical content may still show as
  changed.
- Owner and group are `root:root` unless a `contents` entry says otherwise. The tracked list does not carry the
  numeric ids.
- If the build installs nothing outside `/src`, pkgsmith stops and says so.

## One build, many formats

Two jobs share a build when they have the same image, setup, build script, `exclude` list, source folder and
build environment. Formats are not part of that. A matrix with `deb`, `tar` and `rpm` jobs on one image runs the build
once and packages the result three times.

## Concurrency

`-j N` runs up to `N` jobs at the same time. Each distro and architecture is a lane in the progress output. See
[command line](cli.md). Jobs that wait for a shared build do not use a slot of their own.

## Troubleshooting

| Message                                                    | Cause and fix                                                         |
| ---------------------------------------------------------- | --------------------------------------------------------------------- |
| `cannot start podman; is podman installed?`                | Install Podman. Check `podman info`                                   |
| `podman did not answer on its API socket within 10 seconds`| `podman system service` failed to start. Run it by hand to see why    |
| `the setup script failed`                                  | The last 40 lines of its output are printed under the progress block  |
| `the build installed no files outside /src`                | The build wrote only into `/src`. Install to `/usr` or `/opt`          |
| `no tracked file to change; name a src to add this file`   | A `contents` entry without `src` names a path the build did not install |
| A package contains too much                                | Add `exclude` entries. `pkgsmith files` shows the list                |
| Setup runs every time                                      | The script text changes between runs, for example with a timestamp    |
