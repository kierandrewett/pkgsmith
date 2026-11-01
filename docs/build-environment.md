# Build environment

A manifest with a `builders` section builds in a container. pkgsmith watches what the build installs and packages that.
This page explains each part and its limits.

A container is Linux, so this build makes Linux programs only. For macOS and Windows, see
[macOS and Windows](macos-and-windows.md).

```yaml
builders:
  linux:                              # the name of the builder
    image: fedora:43                  # which container
    steps:
      - name: Install the tools
        setup: true                   # runs once, cached
        run: dnf -y install gcc make
      - name: Build                   # runs on every build
        run: |
          make
          make install PREFIX=/usr
```

## Requirements

- Podman. pkgsmith uses the Podman API through the `podman-api` crate. It does not run `podman` commands for builds.
- The socket: `$XDG_RUNTIME_DIR/podman/podman.sock` when it answers. Otherwise pkgsmith starts `podman system service`
  with a socket in `$XDG_RUNTIME_DIR/pkgsmith-*/` and stops it when the run ends.
- Docker is not supported. The overlay mount that protects your source folder is a Podman feature.

## Builders

A builder is a container image and the steps that run in it, in order. It works like a job in a GitHub Actions workflow.

| Key       | Meaning                                                                                                   |
| --------- | --------------------------------------------------------------------------------------------------------- |
| `image`   | The container image. pkgsmith pulls it when it is not on the machine                                      |
| `if`      | An expression that picks the jobs that use this builder. Without it, every job does                       |
| `by`      | Matrix keys, besides `arch`, that the steps may read. Each value makes a separate run                      |
| `env`     | Variables every step gets. The values are literal text                                                    |
| `exclude` | Paths the package leaves out. See [contents](contents.md)                                                 |
| `steps`   | The scripts, in order                                                                                     |

### Which builder a job uses

A job uses the builder whose `if` is true.

- No builder: the job has no build. It packages `contents` only, and it needs no Podman. The `dmg`, `msi` and `nsis`
  jobs of `pkgsmith.yaml` work this way.
- One builder: its steps run, and the job packages what they installed.
- Two builders: an error, so a mistake in an `if` cannot quietly change what is built.

The `if` of a builder reads `matrix.NAME`, `distro`, `arch` and `env.NAME`. It cannot read `format`. A build must not
depend on the format it is packaged as.

```yaml
builders:
  linux:
    if: matrix.distro == 'fedora' || matrix.distro == 'debian'
    image: debian:13
    steps: [...]
  alpine:
    if: matrix.distro == 'alpine'
    image: alpine:3.22
    steps: [...]
```

## Steps

A step is one script.

| Key     | Meaning                                                                                                        |
| ------- | -------------------------------------------------------------------------------------------------------------- |
| `run`   | The script. Required                                                                                           |
| `name`  | Shown in the progress output                                                                                   |
| `if`    | An expression. A false step is skipped                                                                         |
| `env`   | Variables for this step. The values are literal text                                                           |
| `setup` | `true` for a leading step that runs once and is cached. See below                                              |

- Every step runs in a **fresh `sh -e`** with the build context at `/src`. It does not see what an earlier step
  exported or where it changed directory. It does see the files that earlier steps wrote. Set `PATH` inside the step
  that needs it.
- `sh` is `dash` on Debian and `ash` on Alpine. A `#!` line is not special: the text always
  runs with `sh -e`. Call `bash -c` or `bash script.sh` when you need bash.
- The `if` of a step reads `arch`, `env.NAME` and the matrix keys in the `by` of its builder.
- Environment values are written as single-quoted text, so quotes, `$` and backticks in them stay literal. A step
  that needs a value built from another variable sets it in its `run`.

### Setup steps

Use the distro you build for as the image, because a program links against the libraries it was built on.

Steps with `setup: true` come first. They run once per image and architecture. Put slow, stable work here, such as
installing compilers and libraries.

After they end, pkgsmith saves the container as a local image tagged `localhost/pkgsmith-setup:<hash>`. The hash covers
the base image, the architecture and the text of the steps. Change any of them and the next build runs them again.
Otherwise the step shows `CACHED`.

- Files that setup steps write are **not** packaged.
- `--no-cache` runs every setup step again.
- `pkgsmith prune` removes the cached images.
- Builds that need the same setup image wait for the first one to finish it.
- A setup step after a step that is not a setup step is an error.

### The other steps

They run on top of the setup image.

- The build context is mounted at `/src`, and each step starts there. The context is the folder you pass to
  `pkgsmith build` (default `.`).
- `/src` is a **throw-away overlay**. The build can write files into it, such as a `build/` folder, and your checkout
  stays unchanged. The writes are dropped when the container is removed.
- The steps run as root in the container. They can install to `/usr`, `/etc` and `/opt` directly.

### Environment

| Variable            | Value                                                                  |
| ------------------- | ---------------------------------------------------------------------- |
| `PKGSMITH_NAME`     | `name`                                                                 |
| `PKGSMITH_VERSION`  | `version`, as written in the manifest                                  |
| `PKGSMITH_RELEASE`  | `release`                                                              |
| `PKGSMITH_ARCH`     | `amd64` or `arm64`                                                     |
| `PKGSMITH_DISTRO`   | The matrix `distro`. Only when `distro` is in `by`                     |
| `MATRIX_<KEY>`      | The matrix value of every key in `by`, and of `arch`                   |

The format and every other matrix value are left out. That is what lets jobs share a run: the steps cannot read what
the jobs do not share.

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

## One build, many formats and distros

Two jobs share a run when they have all of these in common:

- The same builder and the same architecture.
- The same value for every key in `by`.
- The same steps, after the conditions are decided.
- The same `exclude` list and the same source folder.

The format is not part of that. A matrix with `deb`, `tar` and `rpm` jobs on one builder runs the build once and
packages the result three times.

By default `by` is empty, so the distro is not part of it either. The rpm, deb and Arch jobs of this repository use one
builder, and `pkgsmith plan` shows one `BUILD` letter for them:

```yaml
builders:
  linux:
    if: matrix.distro == 'fedora' || matrix.distro == 'debian' || matrix.distro == 'arch'
    image: debian:13
    steps: [...]
  alpine:
    if: matrix.distro == 'alpine'
    image: alpine:3.22
    steps: [...]
```

If a build does depend on the distro, put `distro` in `by`. Then every distro makes its own run, and the steps can read
`matrix.distro` and `PKGSMITH_DISTRO`.

A program built on one distro runs on distros with the same or a newer glibc. Build on the one with the oldest glibc,
and test the packages on the others. A distro with another libc, such as Alpine with musl, needs its own builder.

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
