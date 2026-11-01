# pkgsmith

pkgsmith builds distribution packages from one script. You describe the package once. pkgsmith builds your program
in a container for each distro and writes the rpm, deb, Arch, Alpine, OpenWrt, tar or zip package.

![pkgsmith builds one package for Fedora, Debian, Arch and Alpine in a single run](docs/media/demo.gif)

## Formats

Every format is a plugin: a shared library that pkgsmith loads. You can add your own. See [plugins](docs/plugins.md).

| Format      | File                             | For                        |
| ----------- | -------------------------------- | -------------------------- |
| `rpm`       | `.rpm`                           | Fedora, openSUSE, RHEL     |
| `deb`       | `.deb`                           | Debian, Ubuntu             |
| `archlinux` | `.pkg.tar.zst`                   | Arch Linux                 |
| `apk`       | `.apk`                           | Alpine                     |
| `ipk`       | `.ipk`                           | OpenWrt                    |
| `slackware` | `.txz`                           | Slackware                  |
| `xbps`      | `.xbps`                          | Void Linux                 |
| `freebsd`   | `.pkg`                           | FreeBSD                    |
| `snap`      | `.snap`                          | snapd                      |
| `oci`       | `.oci.tar`                       | podman, docker             |
| `appimage`  | `.AppImage`                      | Linux desktops             |
| `wheel`     | `.whl`                           | pip                        |
| `gem`       | `.gem`                           | RubyGems                   |
| `npm`       | `.tgz`                           | npm                        |
| `conda`     | `.conda`, `.tar.bz2`             | conda, micromamba          |
| `nupkg`     | `.nupkg`                         | NuGet                      |
| `dmg`       | `.dmg`                           | macOS                      |
| `flatpak`   | `.flatpak`                       | Flatpak                    |
| `nix`       | `.nix.tar.xz`                    | Nix, NixOS, flakes         |
| `msi`       | `.msi`                           | Windows                    |
| `nsis`      | `.exe`                           | Windows                    |
| `msix`      | `.msix`                          | Windows 10 and 11          |
| `tar`       | `.tar.xz`, `.gz`, `.zst`, `.bz2` | Any system                 |
| `zip`       | `.zip`                           | Any system                 |

Options and what each format was checked with are in [formats](docs/formats.md).

pkgsmith writes these files itself and does not call `rpmbuild`, `dpkg-deb`, `makepkg` or `abuild`.

## Install

pkgsmith runs on Linux, macOS and Windows. What a build needs depends on the script:

- A target without a `job` packages files that are already on your machine. It needs no container engine on any system.
- A target with a `job` runs the steps of the job in a container. It needs [Podman](https://podman.io/) and its API.
  On Linux a user socket is enough. On macOS run `podman machine init` once and `podman machine start`. On Windows start
  Podman and set `CONTAINER_HOST` to a `tcp://` address that it serves. See [command line](docs/cli.md#container-engine).

There is no published release yet, so build from source:

```sh
git clone https://github.com/kierandrewett/pkgsmith
cd pkgsmith
cargo build --release --workspace
```

This makes `target/release/pkgsmith` and one plugin library per format. pkgsmith loads plugins from the folder of its
binary, so run it from `target/release`. A release also has a `pkgsmith-TARGET.tar.gz` with the program and its plugins for each of these targets:

- `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`
- `aarch64-apple-darwin` and `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`

To install a build for one user on Linux, copy the plugins under the name `pkgsmith-plugin-NAME.so`:

```sh
install -Dm755 target/release/pkgsmith ~/.local/bin/pkgsmith
for f in target/release/libpkgsmith_plugin_*.so; do
    install -Dm755 "$f" ~/.local/share/pkgsmith/plugins/pkgsmith-plugin-${f##*/libpkgsmith_plugin_}
done
pkgsmith plugins
```

On macOS the libraries end in `.dylib` and the folder is `~/Library/Application Support/pkgsmith/plugins`. On Windows
they end in `.dll` and the folder is `%LOCALAPPDATA%\pkgsmith\plugins`. `pkgsmith plugins install` does this copy for
you on every system.

`pkgsmith plugins` lists what loaded. Once a release exists, `cargo binstall pkgsmith` installs the program and
`pkgsmith plugins install` downloads its plugins. See [command line](docs/cli.md#plugins).

## Quick start

`examples/demo` holds a C program, a Makefile and this `pkgsmith.star`:

```python
pkg = package(
    name = "hello",
    version = "1.0.0",
    description = "A demo that packages a small C program",
    maintainer = "Kieran Drewett <kieran@drewett.dev>",
    license = "MIT",
)

build = step(name = "Build", run = "make && make install PREFIX=/usr")

fedora = job(
    runs_on = "fedora:43",
    setup = [step(name = "Install the build tools", run = "dnf -y install gcc make")],
    steps = [build],
)
debian = job(
    runs_on = "debian:13",
    setup = [step(
        name = "Install the build tools",
        run = """
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y gcc make
""",
    )],
    steps = [build],
)
alpine = job(
    runs_on = "alpine:3.22",
    setup = [step(name = "Install the build tools", run = "apk add --no-cache gcc make musl-dev")],
    steps = [build],
)

target(pkg, "rpm", distro = "fedora", job = fedora)
target(pkg, "deb", distro = "debian", job = debian)
target(pkg, "apk", distro = "alpine", job = alpine)
```

Build it:

```sh
cd examples/demo
pkgsmith build        # the packages go to ./dist
```

Each `target` call is one package. A target names the job that builds its files:

- `runs_on` is the container image the job works in.
- `setup` steps run once in that image, and the result is cached.
- `steps` run in the image with your source at `/src`. pkgsmith records the files they install and packages them.
- Targets that need one job share one build, so a job that several distros can use compiles once.

To choose the files yourself, add a `contents` list. See [contents](docs/contents.md). The script is code, so use loops,
functions and conditions for many distros and architectures. See [scripting](docs/scripting.md). pkgsmith packages itself
with [pkgsmith.star](pkgsmith.star).

## Commands

| Command           | What it does                                                  |
| ----------------- | ------------------------------------------------------------- |
| `build`           | Builds every package and writes it to `dist`                  |
| `plan`            | Shows each package and its file name. Starts no container     |
| `files`           | Lists the files each package would carry                      |
| `formats`         | Lists the formats the installed plugins write                 |
| `plugins`         | Lists the loaded plugins and the folders searched for them    |
| `prune`           | Removes the cached setup images                               |

Run `pkgsmith plan` after you change the script. Jobs with the same `BUILD` letter share one build. Every option is in
[command line](docs/cli.md).

## Documentation

| Page                                              | Covers                                          |
| ------------------------------------------------- | ----------------------------------------------- |
| [Getting started](docs/getting-started.md)        | A package from scratch, step by step            |
| [Script reference](docs/manifest.md)              | Every function and field                        |
| [Contents](docs/contents.md)                      | Files, trees, links and config files            |
| [Build environment](docs/build-environment.md)    | Jobs, steps, the tracked build, caching         |
| [macOS and Windows](docs/macos-and-windows.md)    | Compile on the target system, then package      |
| [Scripting](docs/scripting.md)                    | Loops, functions and conditions for many builds |
| [Versions](docs/versions.md)                      | How one version becomes each format's spelling  |
| [Install scripts](docs/scripts.md)                | What runs when, per format                      |
| [Dependencies](docs/dependencies.md)              | Names, version constraints, per-distro names    |
| [Signing](docs/signing.md)                        | Keys, `--sign-key`, and checking a signature    |
| [Formats](docs/formats.md)                        | The options of every format                     |
| [Plugins](docs/plugins.md)                        | Where plugins live, and how to write one        |
| [Command line](docs/cli.md)                       | Every command and option                        |

Every file in [examples](examples) is a complete script. [examples/full.star](examples/full.star) uses every field and
comments each one.

## Limits

- pkgsmith does not find the shared libraries a program needs. List them in `depends`.
- Builds run for `linux/amd64` and `linux/arm64`, and only with Podman. Docker is not supported.
- pkgsmith does not compile for macOS or Windows. Compile there, or compile with your own tools, and package the files
  with targets that have no `job`. See [macOS and Windows](docs/macos-and-windows.md).
- Packages are signed only when you give `--sign-key`. See [signing](docs/signing.md).
- There is no built-in support for a language. Install the toolchain in a `setup` step and run its build in a step.
  [examples/rust-cli.star](examples/rust-cli.star) does this for Cargo.

## Build and test

```sh
cargo build --workspace
cargo test --workspace
```

## Licence

MIT
