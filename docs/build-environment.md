# Build environment

A manifest with `jobs` builds in a container. pkgsmith watches what the build installs and packages that. This page
explains each part and its limits.

A container is Linux, so this build makes Linux programs only. For macOS and Windows, see
[macOS and Windows](macos-and-windows.md).

```yaml
jobs:
  build:
    runs-on: fedora:43              # which container
    setup:                          # runs once, cached
      - run: dnf -y install gcc make
    steps:                          # runs on every build
      - run: |
          make
          make install PREFIX=/usr
```

## Requirements

- Podman. pkgsmith uses the Podman API through the `podman-api` crate. It does not run `podman` commands for builds.
- The socket: `$XDG_RUNTIME_DIR/podman/podman.sock` when it answers. Otherwise pkgsmith starts `podman system service`
  with a socket in `$XDG_RUNTIME_DIR/pkgsmith-*/` and stops it when the run ends.
- Docker is not supported. The overlay mount that protects your source folder is a Podman feature.

## Jobs

A job builds a tree of files. It works like a job in a GitHub Actions workflow. The packages are written from the tree,
so one job can feed many packages.

| Key        | Meaning                                                                     |
| ---------- | --------------------------------------------------------------------------- |
| `runs-on`  | The container image. pkgsmith pulls it when it is not on the machine        |
| `steps`    | The scripts that make the files, in order. Required                         |
| `setup`    | Scripts that prepare the image first. They run once and are cached          |
| `matrix`   | The variants of the job, such as `arch`                                     |
| `env`      | Variables for every step. The values are literal text                       |
| `exclude`  | Paths the package leaves out. See [contents](contents.md)                   |

### Which job a package uses

A package target names its job with `needs`. See [matrix and expressions](matrix-and-expressions.md#targets).

- `needs: build`: the package is written from the tree of the job `build`.
- `needs` left out: allowed when the manifest has exactly one job, and an error when it has several.
- `needs: []`: no job. The target packs `contents` only and needs no Podman. The `dmg`, `msi` and `nsis` targets of
  `pkgsmith.yaml` work this way.

### What a job can see

A job sees its own `matrix`, the architecture and `env`. It cannot read the format or the distro of a target, because
a build must not depend on the package it ends up in. That is what lets targets of different distros share one run.

A `matrix` of the job works like the matrix of the manifest. A target takes the run of the job whose cell agrees with the
target on every axis of the job's matrix. Without an `arch` axis, the job runs for the architecture of each target that
needs it.

## Steps

A step is one script.

| Key    | Meaning                                                                       |
| ------ | ----------------------------------------------------------------------------- |
| `run`  | The script. Required                                                          |
| `name` | Shown in the progress output                                                  |
| `if`   | An expression over `matrix.NAME` of the job, `arch` and `env.NAME`            |
| `env`  | Variables for this step, after those of the job. The values are literal text  |

- Every step runs in a **fresh `sh -e`** with the build context at `/src`. It does not see what an earlier step
  exported or where it changed directory. It does see the files that earlier steps wrote.
- `sh` is `dash` on Debian and `ash` on Alpine. A `#!` line is not special: the text always runs with `sh -e`. Call
  `bash script.sh` when you need bash.
- Variables are written as single-quoted text, so quotes, `$` and backticks in a value stay literal. A step that needs a
  value built from another variable sets it with the next method.

### Changing what later steps see

Two files carry changes from one step to the next, like `GITHUB_ENV` and `GITHUB_PATH` in GitHub Actions.

```sh
echo "CARGO_HOME=/opt/cargo" >> "$PKGSMITH_ENV"     # NAME=value, one per line
echo "$HOME/.cargo/bin" >> "$PKGSMITH_PATH"         # one folder per line, put in front of PATH
```

pkgsmith reads both as data, never as shell. The text after the first `=` is the value as it is. A line that is not
`NAME=value` stops the step. The order of precedence is: the job's `env`, then `$PKGSMITH_ENV`, then the step's `env`.

### Setup

The `setup` steps run once per image and architecture. Put slow, stable work here, such as installing compilers and
libraries.

After they end, pkgsmith saves the container as a local image tagged `localhost/pkgsmith-setup:<hash>`. The hash covers
the base image, the architecture and the text of the steps. Change any of them and the next build runs them again.
Otherwise the step shows `CACHED`. Lines that setup steps appended to `$PKGSMITH_ENV` and `$PKGSMITH_PATH` stay for the
steps that follow.

- Files that setup steps write are **not** packaged.
- `--no-cache` runs every setup step again.
- `pkgsmith prune` removes the cached images.
- Builds that need the same setup image wait for the first one to finish it.

### The build steps

They run on top of the setup image.

- The build context is mounted at `/src`, and each step starts there. The context is the folder you pass to
  `pkgsmith build` (default `.`).
- `/src` is a **throw-away overlay**. The build can write files into it, such as a `build/` folder, and your checkout
  stays unchanged. The writes are dropped when the container is removed.
- The steps run as root in the container. They can install to `/usr`, `/etc` and `/opt` directly.

### Environment

| Variable           | Value                                                          |
| ------------------ | -------------------------------------------------------------- |
| `PKGSMITH_NAME`    | `name`                                                         |
| `PKGSMITH_VERSION` | `version`, as written in the manifest                          |
| `PKGSMITH_RELEASE` | `release`                                                      |
| `PKGSMITH_ARCH`    | `amd64` or `arm64`                                             |
| `MATRIX_<KEY>`     | The value of every key of the job's own matrix cell            |

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

## One build, many packages

Two packages share a run when they have all of these in common:

- The same job, and the same cell of its matrix. With no `arch` axis, the same architecture.
- The same steps, after the conditions are decided.
- The same `exclude` list and the same source folder.

The format is not part of that, and neither is the distro. A job that three targets need runs once per architecture:

```yaml
jobs:
  linux:
    runs-on: debian:13
    steps: [...]
  alpine:
    runs-on: alpine:3.22
    steps: [...]
targets:
  - format: rpm
    distro: fedora
    needs: linux
  - format: deb
    distro: debian
    needs: linux
  - format: apk
    distro: alpine
    needs: alpine
```

`pkgsmith plan` shows one `BUILD` letter for the rpm and deb packages of an architecture.

A program built on one distro runs on distros with the same or a newer glibc. Build on the one with the oldest glibc,
and test the packages on the others. A distro with another libc, such as Alpine with musl, needs its own job.

## Concurrency

`-j N` runs up to `N` jobs at the same time. Each distro and architecture is a lane in the progress output. Jobs that wait for a shared build do not use a slot of their own.

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
