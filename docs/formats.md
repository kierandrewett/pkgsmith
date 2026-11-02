# Formats

Each format is a plugin. `pkgsmith formats` lists what is installed. The options of a format go in the `options`
dict of the `package()` call that writes it, for example `options = {"section": "utils"}`. A key that the format does not know is
an error. The tables below list the keys of each format.

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package("deb", options = {"section": "utils", "compression": "gzip"}, **common)
package("tar", options = {"compression": "zstd", "top_folder": True}, **common)
```

A value is a Starlark value: text in quotes, `True` or `False`, numbers, lists and dicts. The tables write the
options of each format as keys of that dict. A format with no options needs no `options`.

Some formats have an option named `prefix`. It is a path inside the package, such as `/usr`. It is not the prefix folder
that pkgsmith reads files from. See [the prefix](prefix.md) for that.

| Format      | File                             | For                                  | Verified with                       |
| ----------- | -------------------------------- | ------------------------------------ | ----------------------------------- |
| `rpm`       | `.rpm`                           | Fedora, openSUSE, RHEL               | `rpm` in Fedora 43, openSUSE        |
| `deb`       | `.deb`                           | Debian, Ubuntu                       | `dpkg` in Debian 13                 |
| `ipk`       | `.ipk`                           | OpenWrt                              | Layout only                         |
| `archlinux` | `.pkg.tar.zst`                   | Arch Linux                           | `pacman` in Arch                    |
| `apk`       | `.apk`                           | Alpine                               | `apk` 2 in Alpine 3.22, `apk` 3 in the latest Alpine |
| `slackware` | `.txz`                           | Slackware                            | `installpkg` in Slackware 15.0      |
| `snap`      | `.snap`                          | snapd                                | `unsquashfs` only, not snapd        |
| `oci`       | `.oci.tar`                       | OCI runtimes                         | `load` and `run` in an OCI tool     |
| `appimage`  | `.AppImage`                      | Linux desktops                       | The runtime, without FUSE           |
| `wheel`     | `.whl`                           | pip                                  | `pip`, `twine` in Python 3.14       |
| `xbps`      | `.xbps`                          | Void Linux                           | `xbps-install` in Void              |
| `gem`       | `.gem`                           | RubyGems                             | `gem install` in the latest Ruby    |
| `npm`       | `.tgz`                           | npm                                  | `npm install` in the latest Node    |
| `conda`     | `.conda`, `.tar.bz2`             | conda, micromamba                    | `micromamba create` from a channel  |
| `nupkg`     | `.nupkg`                         | NuGet                                | `dotnet add package`, local feed    |
| `flatpak`   | `.flatpak`                       | Flatpak                              | `flatpak install` and `run` in Debian 13 |
| `dmg`       | `.dmg`                           | macOS                                | `fsck.hfsplus`, `7z`, `dmg2img`, not macOS |
| `nix`       | `.nix.tar.xz`, `.gz`, `.zst`, `.bz2` | Nix and NixOS, with or without flakes | `nix build`, `nix run`, `nix flake check`, `nix-build`, in `nixos/nix` |
| `nix-cache` | `.nixcache.tar`                  | Nix, as a binary cache               | `nix copy --from file://`, `nix build --substituters`, in `nixos/nix` |
| `nix-export` | `.nixexport`                    | Nix                                  | `nix-store --import`, in `nixos/nix` |
| `msi`       | `.msi`                           | Windows                              | `msitools` read and extract, not Windows |
| `nsis`      | `.exe`                           | Windows                              | `makensis`, run under Wine, not native Windows |
| `msix`      | `.msix`                          | Windows 10 and 11                    | `makemsix` unpack and pack only     |
| `appx`      | `.appx`                          | Windows 10 1607 and later            | `makemsix` unpack and pack, block hashes, not Windows |
| `freebsd`   | `.pkg`                           | FreeBSD                              | Layout only, not the real `pkg`     |
| `tar`       | `.tar.xz`, `.gz`, `.zst`, `.bz2` | Any system                           | `tar`                               |
| `zip`       | `.zip`                           | Any system                           | `unzip`                             |

A format that depends on a runtime or a target OS says so below. Signing is in [signing](signing.md).

## rpm

| Key           | Values                       | Default       | Effect                    |
| ------------- | ---------------------------- | ------------- | ------------------------- |
| `group`       | Text                         | `Unspecified` | The `Group` tag           |
| `compression` | `zstd`, `xz`, `gzip`, `bzip2` | `zstd`       | Compression of the payload |

## deb and ipk

| Format | Key           | Values                | Default    | Effect                                      |
| ------ | ------------- | --------------------- | ---------- | ------------------------------------------- |
| `deb`  | `section`     | Text                  | `misc`     | The `Section` field                         |
| `deb`  | `priority`    | Text                  | `optional` | The `Priority` field                        |
| `deb`  | `compression` | `xz`, `zstd`, `gzip`  | `xz`       | Compression of the data and control parts   |
| `ipk`  | `arch`        | OpenWrt architecture  | From the package `arch` | For example `aarch64_generic`  |

## archlinux

| Key        | Values | Default          | Effect               |
| ---------- | ------ | ---------------- | -------------------- |
| `packager` | Text   | The maintainer   | The `packager` field |

## apk

The format has no options. Install an unsigned package with `apk add --allow-untrusted`; [signing](signing.md) says how
to sign one.

Every regular file and symlink in the data part carries an `APK-TOOLS.checksum.SHA1` pax record, as `abuild` writes
it: the SHA-1 of the file, or of the link target. A link target longer than 100 bytes goes in a pax `linkpath`
record. apk-tools 2 marks a package broken, and `apk add` exits 1, when a file or a link has no checksum.

`scripts/verify-formats.sh` installs the package with apk-tools 2 (Alpine 3.22) and apk-tools 3 (the latest Alpine),
runs it, follows both links, and checks that `apk audit` finds a checksum for every file and link.

## tar and zip

| Format | Key           | Values                                | Default | Effect                                         |
| ------ | ------------- | ------------------------------------- | ------- | ---------------------------------------------- |
| `tar`  | `compression` | `xz`, `gzip`, `zstd`, `bzip2`, `none` | `xz`    | The compression and the file extension         |
| `tar`  | `top_folder`  | `True`, `False`                       | `False` | Put everything below `{name}-{version}/`       |
| `zip`  | `top_folder`  | `True`, `False`                       | `False` | Put everything below `{name}-{version}/`       |

## slackware

The package is a tar of the files with `install/slack-desc` and `install/doinst.sh`. Configuration files ship as
`NAME.new` and are moved into place only when no file of that name exists.

The format has only a post-install script. These package fields are ignored with a warning:

- `pre_install`, `pre_remove` and `post_remove`
- `depends`

| Key           | Values                          | Default         | Effect                                      |
| ------------- | ------------------------------- | --------------- | ------------------------------------------- |
| `tag`         | Text without `-`                | The `release`   | The build tag at the end of the file name   |
| `compression` | `xz`, `gzip`, `zstd`, `bzip2`   | `xz`            | Compression, and the extension `.txz` and so on |

## snap

The image is SquashFS with xz. Files keep their install paths below the root of the snap, and an app `command` is
relative to that root. pkgsmith checks the name and version against the snapd rules. It has not been installed under
snapd.

- `post_install` becomes the install hook. `post_remove`, or else `pre_remove`, becomes the remove hook.
- `depends` and `pre_install` have no meaning in a snap and give a warning.

| Key           | Values                              | Default   | Effect                                         |
| ------------- | ----------------------------------- | --------- | ---------------------------------------------- |
| `base`        | `core22`, `core24`, `core20`, `bare` | `core22` | The base snap                                  |
| `confinement` | `strict`, `classic`, `devmode`      | `strict`  | How much the snap is confined                  |
| `grade`       | `stable`, `devel`                   | `stable`  | `devmode` needs `devel`                        |
| `title`       | Text                                | None      | Title for the store                            |
| `summary`     | Text, at most 78 characters         | First line of the description | The summary                |
| `apps`        | Map of name to app                  | None      | The apps, see below                            |

An app has `command` (required), `daemon` (`simple`, `forking`, `oneshot` or `notify`), `plugs` (a list of interfaces)
and `environment` (a map).

```python
package(
    "snap",
    name = "hello", version = "1.0.0", description = "A small program",
    options = {
        "base": "core24",
        "apps": {"hello": {"command": "/usr/bin/hello", "plugs": ["network"]}},
    },
)
```

## oci

The image has one layer: the files, on top of nothing. That suits a static program or a bundle with its own libraries. A
dynamic program also needs its libraries in the package. `pre_install` and `post_install` are ignored, because an
image is not installed.

| Key           | Values                    | Default       | Effect                                       |
| ------------- | ------------------------- | ------------- | -------------------------------------------- |
| `entrypoint`  | List of text              | None          | The program the image runs               |
| `cmd`         | List of text              | None          | Default arguments                            |
| `env`         | Map                       | None          | Environment variables                        |
| `workdir`     | Path                      | None          | The folder the program starts in             |
| `user`        | Name or number, `user:group` | None       | The user the program runs as                 |
| `ports`       | List like `8080/tcp`      | None          | Ports the program listens on                 |
| `labels`      | Map                       | None          | Extra labels                                 |
| `repository`  | Text                      | The name      | The image name                               |
| `tag`         | Text                      | The version   | The image tag                                |
| `compression` | `gzip`, `none`            | `gzip`        | Compression of the layer                     |

Load it with `docker load -i hello-1.0.0-amd64.oci.tar`.

## appimage

An AppImage is a runtime followed by a SquashFS image. pkgsmith does not build the runtime: give a runtime file from the
[type2-runtime releases](https://github.com/AppImage/type2-runtime/releases) that matches the architecture. The package
bundles its libraries, so `depends` and the install scripts are ignored with a warning.

| Key                  | Values                        | Default            | Effect                                     |
| -------------------- | ----------------------------- | ------------------ | ------------------------------------------ |
| `runtime`            | Path to the runtime file      | Required           | The start of the file                      |
| `icon`               | A `.png` or `.svg` file       | Required           | The icon. The spec needs one               |
| `exec`               | Path inside the AppDir        | `usr/bin/NAME`     | The program that `AppRun` starts           |
| `compression`        | `zstd`, `gzip`                | `zstd`             | Compression of the SquashFS                |
| `desktop.name`       | Text                          | The package name   | `Name=` of the desktop file                |
| `desktop.comment`    | Text                          | The summary        | `Comment=`                                 |
| `desktop.categories` | Text that ends with `;`       | `Utility;`         | `Categories=`                              |
| `desktop.terminal`   | `true`, `false`               | `false`            | `Terminal=`                                |

Run it without FUSE with `./hello.AppImage --appimage-extract-and-run`.

## wheel

A wheel is a zip that pip unpacks into site-packages. Give the install path that corresponds to site-packages. Where a
file goes depends on its path:

- Below `site_packages`: the root of the wheel.
- Directly in `/usr/bin` or `/usr/local/bin`: a script.
- Anywhere else: the data folder of the wheel, which pip installs under the Python prefix and not at `/`.

Symbolic links are an error. `depends` holds distro names, so it is ignored with a warning: use `requires_dist` for
Python requirements.

A pre-release such as `1.2.3-nightly.20261031.142.gabc1234` is not valid PEP 440. It becomes the dev release
`1.2.3.dev202610310142`, which sorts before `1.2.3`.

| Key               | Values                                  | Default                       | Effect                       |
| ----------------- | --------------------------------------- | ----------------------------- | ---------------------------- |
| `site_packages`   | Absolute install path                   | Required                      | Maps files to the wheel root |
| `python_tag`      | For example `py3`, `cp313`              | `py3`                         | Tag in the file name         |
| `abi_tag`         | For example `none`, `abi3`              | `none`                        | Tag in the file name         |
| `platform_tag`    | For example `manylinux_2_17_x86_64`     | `any` for `arch = "all"`, else required | Tag in the file name  |
| `requires_python` | For example `>=3.9`                     | None                          | `Requires-Python`            |
| `classifiers`     | List of text                            | None                          | `Classifier` lines           |
| `requires_dist`   | List like `requests>=2`                 | None                          | `Requires-Dist` lines        |
| `entry_points`    | Map of group to map of name to `module:function` | None                 | `entry_points.txt`           |

pip does not check the hashes in `RECORD` when it installs, so a changed file in a wheel installs without an error.
`python -m wheel unpack` does check them.

## dmg

A `.dmg` is an Apple disk image: a UDIF image that holds one HFS+ volume with your files. pkgsmith writes both parts
itself, in pure Rust, so no tool is needed to build one. The volume writer follows Apple's public
[TN1150](https://developer.apple.com/library/archive/technotes/tn1150/) and was written without reading GPL code.

A disk image has no install path. Files sit on the volume in the same layout as the installed system, for example
`/usr/bin/hi`. Use `prefix` to put them in a folder, such as `/Applications`.

- File names must be ASCII. HFS+ compares other names after Unicode normalisation and case folding, which pkgsmith does
  not implement, so any other name stops the build with the path in the error.
- Two names in one folder that differ only in ASCII case stop the build.
- A symbolic link is written as an HFS+ symbolic link: a file whose data fork is the target, with the BSD link type bits
  and the Finder type `slnk`. The target is written as it is, so an absolute target such as `/Applications` works. It must
  be ASCII.
- Dependencies, scripts, `description`, `license`, owners and the config flag have no place in a disk image. Each one
  warns. Every file is written with uid 0 and gid 0, and the dates are the build time.

| Key           | Values                                  | Default            | Effect                                              |
| ------------- | --------------------------------------- | ------------------ | --------------------------------------------------- |
| `prefix`      | An absolute path                        | None               | A folder on the volume that receives every entry    |
| `volume_name` | 1 to 31 bytes, no `:`                   | The package name   | The name of the volume in the Finder                |
| `compression` | `zlib`, `none`                          | `zlib`             | The compression of the blocks in the image          |

Checked with `scripts/verify-formats.sh dmg` on a volume with 320 files in one folder, a 5 MiB file, an empty file, an
executable, a name with a space and a 200-character name:

- `dmg2img` reads the image.
- `fsck.hfsplus` reports the volume as OK.
- `7z` extracts the files, and each one is byte for byte equal to its source.
- A volume that holds a relative and an absolute (`/Applications`) symbolic link: `fsck.hfsplus` reports it OK and `7z` extracts both as links
  (`scripts/verify-formats.sh dmg`, latest Fedora).

Not checked:

- Mounting on macOS with `hdiutil`, and the Finder.
- Volumes of more than 2^32 blocks.
- The `prefix` option with an invalid value.

## flatpak

A `.flatpak` is a single-file OSTree bundle. pkgsmith does not write the OSTree objects itself. It lays out the app tree
and runs the `flatpak` program in a temporary folder, so `flatpak` must be on `PATH` when you build. Without it the
build stops and says so.

The bundle holds only the app. The runtime is fetched from `runtime_repo` when a user installs the bundle. These
`flatpak` commands run, in this order:

- `build-finish`
- `build-export`
- `build-bundle`

- `app_id` and `runtime_version` are required. pkgsmith does not guess a runtime version.
- `arch` must be `amd64` or `arm64`. `all` stops the build, because a bundle has one architecture.
- Files below `prefix` go to `files/` of the app. A file outside `prefix` stops the build.
- Dependencies, scripts, owners and the fields `maintainer`, `homepage` and `license` have no place in a bundle. Each one warns.

| Key               | Values                                          | Default                                 | Effect                                        |
| ----------------- | ----------------------------------------------- | --------------------------------------- | --------------------------------------------- |
| `app_id`          | Reverse-DNS ID with three or more parts         | None. Required                          | The application ID and the bundle name        |
| `runtime_version` | Text such as `24.08`                            | None. Required                          | The version of the runtime                    |
| `runtime`         | Reverse-DNS ID                                  | `org.freedesktop.Platform`              | The runtime the app runs on                   |
| `sdk`             | Reverse-DNS ID                                  | `runtime` with `.Platform` as `.Sdk`    | The matching SDK                              |
| `command`         | A program name                                  | The package name                        | The program to start. It must be `prefix/bin/NAME` |
| `branch`          | Text                                            | `stable`                                | The branch of the ref                         |
| `finish_args`     | List of options that start with `--`            | None                                    | Passed to `flatpak build-finish`, such as `--share=network` |
| `runtime_repo`    | A `.flatpakrepo` URL, or `""`                   | The Flathub `.flatpakrepo`              | Where `flatpak install` finds the runtime. `""` leaves it out |
| `prefix`          | An absolute path                                | `/usr`                                  | The folder that becomes `files/`              |

Checked on `x86_64` by `scripts/verify-formats.sh flatpak` in the latest Debian, and by hand in Debian 13 with Flatpak 1.16.6: `flatpak build-import-bundle` accepts the file, and
`flatpak install --bundle` followed by `flatpak run` works.

Not checked:

- `aarch64` bundles.
- Installing without adding the Flathub remote first.
- A graphical app.
- `finish_args` beyond the keys written to the metadata.

## nix

Three formats write Nix packages. They differ in what Nix does with the files, so pick by how the user installs.

| Format       | File                            | Holds                                              | The user runs                                            |
| ------------ | ------------------------------- | -------------------------------------------------- | -------------------------------------------------------- |
| `nix`        | `NAME-VERSION-ARCH.nix.tar.xz`  | A flake, `package.nix`, `default.nix` and `files/` | `nix build`, `nix run`, `nix profile install`, `callPackage` |
| `nix-cache`  | `NAME-VERSION-ARCH.nixcache.tar` | A binary cache with the files as one store path   | `nix copy --from file://DIR STOREPATH`, or a substituter |
| `nix-export` | `NAME-VERSION-ARCH.nixexport`   | The same store path as the stream of `nix-store --export` | `nix-store --import < FILE`                      |

pkgsmith builds nothing in any of them. The files are the content of the prefix, with the option `prefix` (default `/usr`)
removed: `/usr/bin/hi` is `bin/hi` in the store path. An entry outside the prefix, such as `/etc/hi.conf`, keeps its whole
path below the store path (`etc/hi.conf`), because a Nix package cannot write outside its store path. Nix keeps only the
executable bit of the owner and no owner or group, so the other mode bits are not carried.

### nix

The archive unpacks to `NAME-VERSION/`:

| File           | What it is                                                                                          |
| -------------- | --------------------------------------------------------------------------------------------------- |
| `package.nix`  | A function in the style of nixpkgs, `{ lib, stdenv, zlib, ... }: stdenv.mkDerivation { ... }`        |
| `flake.nix`    | `packages.<system>.NAME` and `.default`, and `overlays.default`, over a `nixpkgs` input              |
| `default.nix`  | `pkgs.callPackage ./package.nix { }`, for `nix-build` without flakes                                |
| `files/`       | The content of the store path                                                                      |

```console
$ nix build --no-write-lock-file 'https://example.org/hi-1.0.0-amd64.nix.tar.xz'
$ nix run --no-write-lock-file 'https://example.org/hi-1.0.0-amd64.nix.tar.xz'
$ nix profile install --no-write-lock-file 'https://example.org/hi-1.0.0-amd64.nix.tar.xz#hi'
$ nix-build NAME-VERSION                      # from the unpacked folder, without flakes
```

The derivation copies `files/` into `$out` and builds nothing. Then the fixup phase of `stdenv` runs, so `patchShebangs` and
the other hooks do their usual work. The arguments of `package.nix` are the libraries of the package, so
`pkgs.callPackage ./package.nix { openssl = pkgs.openssl_3; }` and `.override` change them.

- A program that was linked on another distro looks for `/lib64/ld-linux-x86-64.so.2` and its libraries in the usual places.
  `autoPatchelfHook` points it at the interpreter and libraries of nixpkgs. pkgsmith turns it on when it finds a
  dynamically linked ELF file in the package, and says so. Name the libraries in `inputs`. The build fails and names the
  library that it could not find, which is how Nix wants a missing dependency to show.
- `license` is written with `lib.getLicenseFromSpdxId`, so nixpkgs maps the SPDX id. `A OR B` and `A AND B` give a list.
  Text that is not an SPDX expression becomes `{ shortName = "..."; }`.
- `meta.sourceProvenance` is `binaryNativeCode` when the package holds an ELF file. `meta.mainProgram` is the option, or
  the package name when `bin/NAME` exists. `meta.platforms` and the systems of the flake come from `arch` (`amd64` is
  `x86_64-linux`) and from `distro` (`macos` gives `-darwin`). `arch = "all"` is every system of
  `lib.systems.flakeExposed`.
- A systemd unit, desktop file or D-Bus service names `/usr/bin/hi`, which does not exist in Nix. Files that match
  `rewrite` and contain `prefix/` get `substituteInPlace ... --replace-fail` in `postInstall`, so the path names `$out`.
- `depends`, `scripts`, `conflicts` and the other lists of the script have no meaning for Nix and give a warning. A
  configuration file is an ordinary read-only file. See [the research note](research/nix.md) for what a NixOS module
  would need.
- The flake has one input, `nixpkgs`, and no `flake.lock`: pkgsmith cannot look up a revision, so a remote flake needs
  `--no-write-lock-file`, or `nix flake lock` on the unpacked folder. Pin nixpkgs with the option `nixpkgs`, for example
  `github:NixOS/nixpkgs/nixos-25.05`, or on the command line with `--override-input nixpkgs ...`.

| Option           | Values                                          | Default                                 | Effect                                                                       |
| ---------------- | ----------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------- |
| `prefix`         | An absolute path, or `""`                        | `/usr`                                  | Entry paths are taken relative to it                                         |
| `compression`    | `xz`, `gzip`, `zstd`, `bzip2`, `none`           | `xz`                                    | The compression of the archive                                               |
| `inputs`         | List of nixpkgs attributes, such as `openssl`, `xorg.libX11`, `stdenv.cc.cc.lib` | None | `buildInputs`, and an argument of `package.nix` for the first part       |
| `native_inputs`  | List of nixpkgs attributes                      | None                                    | `nativeBuildInputs`                                                          |
| `runtime_inputs` | List of nixpkgs attributes                      | None                                    | `runtimeDependencies`: libraries that a program loads with `dlopen`          |
| `patchelf`       | `true`, `false`                                 | `true` when a dynamic ELF file is found | Adds `autoPatchelfHook`, and uses `stdenv` instead of `stdenvNoCC`           |
| `ignore_missing` | List of library names, or `["*"]`               | None                                    | `autoPatchelfIgnoreMissingDeps`                                              |
| `strip`          | `true`, `false`                                 | `false`                                 | Leave the strip step of the fixup phase on. Off keeps the files as built     |
| `rewrite`        | List of globs relative to the store path        | Units in `lib/systemd/{system,user}`, `share/applications/*.desktop`, `share/dbus-1/**/*.service` | Text files in which `prefix/` becomes `$out/`. `[]` turns it off |
| `main_program`   | A file name below `bin/`                        | The package name, if `bin/NAME` exists  | `meta.mainProgram`                                                           |
| `platforms`      | List of Nix systems, such as `x86_64-linux`     | From `arch` and `distro`                | `meta.platforms` and the systems of the flake                                |
| `unfree`         | `true`, `false`                                 | `false`                                 | `free = false` in the license, and the flake allows this one package         |
| `nixpkgs`        | A flake reference                               | `github:NixOS/nixpkgs/nixos-unstable`   | The `nixpkgs` input of `flake.nix`                                           |

A glob has `*` for part of one name, `**` for any folders and `?` for one character.

### nix-cache and nix-export

These make the files a real store object, which `nix-store --add` would also give. A store path is `/nix/store/DIGEST-NAME`.
Nix computes the digest of a built path from its derivation. There is none here, so the path is content-addressed: the
digest is the SHA-256, folded to 160 bits and written in Nix32, of the text
`source[:REFERENCE...]:sha256:HEX-OF-NAR-HASH:/nix/store:NAME`. `NAME` is `NAME-VERSION` of the package. The same files
always give the same path, and Nix rejects a path whose digest does not fit its content. A content-addressed path needs no
signature.

```console
$ pkgsmith build -F nix-cache,nix-export -o out
hi/amd64 │ store path /nix/store/qi29kyz3kznscq57z46c076zys884mik-hi-1.0.0
$ mkdir cache && tar -xf out/hi-1.0.0-amd64.nixcache.tar -C cache
$ nix copy --from file://$PWD/cache /nix/store/qi29kyz3kznscq57z46c076zys884mik-hi-1.0.0
$ nix-store --import < out/hi-1.0.0-amd64.nixexport
```

- `nix-cache` unpacks to `nix-cache-info`, `DIGEST.narinfo` and `nar/FILEHASH.nar.xz`. Several packages unpack into one
  folder, which is then one cache. Serve the folder over HTTP and name it in `substituters`, or use `file://`. The
  `.narinfo` has `CA: fixed:r:sha256:...`, so Nix accepts the path without `trusted-public-keys`. pkgsmith writes no
  `Sig:` line: signing a cache with a key is not done.
- `nix-export` is one file. `nix-store --import` adds the path without a `CA` field, so a later `nix copy` of that path to
  a store that checks signatures needs a signature. The import itself needs a user that may add unsigned paths, such as
  root or a trusted user.
- The path is printed when the package is written. The user needs it, because it is what the cache and the import
  provide.
- Files that spell a store path of another package, such as an RPATH or a shebang, are a reference of the store object.
  Nix keeps the referenced path with it, so the other path must be in the cache or the store. pkgsmith warns and names
  them.
- `compression` is the compression of the NAR inside a cache (`nix-cache`), where the tar around it is plain, and of the
  whole file for `nix-export`, where the default is `none`. Nix reads `xz`, `gzip`, `zstd`, `bzip2` and `none` in a cache.

| Option            | Values                                    | Default      | Effect                                                                         |
| ----------------- | ----------------------------------------- | ------------ | ------------------------------------------------------------------------------ |
| `prefix`          | An absolute path, or `""`                  | `/usr`       | Entry paths are taken relative to it                                           |
| `compression`     | `xz`, `gzip`, `zstd`, `bzip2`, `none`     | `xz` (`none` for `nix-export`) | See above                                                        |
| `store_dir`       | An absolute path                          | `/nix/store` | The store directory. It is part of every digest, and a store that uses another one cannot take the path |
| `references`      | List of store paths                       | None         | Paths that the files may refer to. One is a reference when its digest occurs in the files |
| `scan_references` | `true`, `false`                           | `true`       | Also take every `STORE_DIR/DIGEST-NAME` that the files spell as a reference    |

The files of a prefix that a distro built have no store paths in them. A program from such a prefix finds its loader in
`/lib64`, so it runs on a system that has one and Nix, and not on NixOS. Use the `nix` format when the program must run on
NixOS.

### What was checked

By `scripts/verify-formats.sh nix`, in the latest `nixos/nix` (Nix 2.35.4) as root, with network access for nixpkgs:

- `nix`: the flake builds from a `tarball+file` address and from the unpacked folder, with `nix build`, `nix run`,
  `nix flake check`, `nix profile install` and `nix-build` of `default.nix`. A program that Debian compiled against glibc and
  zlib runs after `autoPatchelfHook`, and `patchelf` shows the interpreter below `/nix/store`. A systemd unit names the new
  path. `meta.mainProgram` and the licenses evaluate.
- `nix-cache`: `nix copy --from file://` takes the path without `--no-check-sigs`, `nix path-info` shows the `ca` field,
  `nix store verify` passes, and `nix build --substituters file://` rebuilds a deleted path. A narinfo with another digest
  is refused. A closure of three paths, in which the third names the other two, copies with its references.
- `nix-export`: `nix-store --import` adds the path and it runs.

Not checked: a multi-user Nix with a daemon and an untrusted user, macOS, `aarch64`, an HTTP cache, a cache signed with a
key, a binary linked against libraries that nixpkgs lacks, and a NixOS system that uses the package.

## msi

An `.msi` is a Windows Installer package. It is a database of tables, a Summary Information stream and the files in one
embedded cabinet. pkgsmith writes all three in pure Rust, so no tool is needed to build one. The package is written
unsigned.

Every folder is a component whose key path is the folder, and every file is a component of its own. A newer package with the
same name and upgrade code replaces an older one: the `Upgrade` table finds older versions, and `RemoveExistingProducts`
removes them after the new files are installed.

- The version is `MAJOR.MINOR.PATCH`, with parts of at most 255, 255 and 65535. The release and a pre-release text are
  not written. Each warns.
- File names must be ASCII and must not have characters that Windows refuses. Two names that differ only in case stop
  the build. A name that is not a valid 8.3 name gets a short name, for example `A-RATH~1.CON|a-rather-long-name.conf`.
- Paths are taken relative to `prefix`. `/usr/bin/hi.exe` goes to `bin\hi.exe` in the install folder.
- The package needs at least one file, and at most 32767.
- An MSI refers to a string by its number in a string pool. Two bytes hold 65,535 numbers, and every file adds several
  strings (its key, its component, its names). When a package needs more, pkgsmith sets the flag of the string pool and
  writes every reference with three bytes, as Windows Installer allows. A package of 30,000 files, and the 24,000 files
  of an Electron app with a Python runtime, need it. A small package keeps the two byte form. The `msi` crate cannot do
  this, so pkgsmith carries a patched copy of it in `vendor/msi`; see `vendor/msi/PATCHES.md`.
- Dependencies, scripts, `license`, symbolic links, ghost entries, owners and modes have no place in an MSI. Each one
  warns. A configuration file is installed as an ordinary file.

| Key            | Values                              | Default                                              | Effect                                              |
| -------------- | ----------------------------------- | ---------------------------------------------------- | --------------------------------------------------- |
| `scope`        | `machine`, `user`                   | `machine`                                            | `machine` installs for all users and needs elevation. `user` installs for the current user |
| `install_dir`  | A folder, written with `\` or `/`   | The package name, and `Programs\NAME` for `user`     | The folder below Program Files or the local application data folder |
| `manufacturer` | Text                                | The maintainer                                       | The manufacturer Windows shows. One of the two is required |
| `upgrade_code` | A GUID                              | Derived from the package name                        | Every version of a package shares it                |
| `prefix`       | An absolute Unix path               | `/usr`                                               | Entry paths are taken relative to it. A path outside it stops the build |
| `shortcuts`    | List of entry paths                 | None                                                 | One Start Menu shortcut for each, in a folder named after the package. The folder goes with the package |

With `scope = "machine"`, the files go to `ProgramFiles64Folder` for `amd64` and `arm64`, and to `ProgramFilesFolder` for
`all`.

The product code, the component codes and the package code come from the name, version and architecture, so equal input
gives equal codes.

Checked with `scripts/verify-formats.sh msi`:

- `msiinfo` lists the tables and the Summary Information.
- `msidump` writes the tables, and every reference between them resolves: files to components, components to folders,
  folders to parents, and the feature and `CreateFolder` rows to components and folders.
- `msiextract` extracts the cabinet, and every file is byte for byte equal to its source. That includes a 1.5 MiB file,
  an empty file and names with spaces.
- A `Shortcut` row names a file key, a component and a folder, and the `RemoveFile` row that removes its folder, that all resolve.
- A package of 30,000 files, in 600 folders, with long names: `msiinfo` and `msidump` read every table, every reference
  resolves, and `msiextract` gives back every path with its content.

A test of the `msi` plugin (`cargo test -p pkgsmith-plugin-msi`) builds the same 30,000 files, and reads the database with
the `msi` crate. A small package (three files, compared once by hand) gives the same bytes as before the three byte form existed.

By hand, the exported tables were read for both scopes. Also by hand, under Wine 11 on Linux (not part of the suite): the
24,000 files of the PiKVM Desktop for Windows (350 MiB as an MSI, 1.1 GiB installed) were installed with
`wine msiexec /i package.msi /qn`, and every file arrived. `pikvm-agent.exe --help` ran from the installed folder.
The Start Menu shortcut was created, and `wine msiexec /x` removed the files, the shortcut and its folder.

Not checked:

- Installing and uninstalling with `msiexec` on Windows. Wine is not Windows Installer.
- Validation with the ICE rules. A `user` package will fail ICE38, which wants a registry key as the key path of each
  component in the user profile.
- The sequence numbers beyond the Microsoft documentation. `FindRelatedProducts` at 25 is a convention, and the
  documentation only requires it to come before `RemoveExistingProducts`.

## nsis

An `.exe` made by NSIS is a self-extracting Windows installer that adds the program to Add/Remove Programs. NSIS has no
practical writer in Rust, so pkgsmith writes an `.nsi` script and runs `makensis`.

`makensis` must be on `PATH` when you write the package. Without it pkgsmith stops and names the packages that carry it: `nsis` on
Debian and Arch, and `mingw32-nsis` on Fedora.

NSIS itself can build installers that run on any Windows from 95 to 11, according to its own documentation. The script
that pkgsmith writes is narrower:

- It is a Unicode installer. NSIS documents a narrower range for Unicode installers than for its generic support list,
  and pkgsmith has not pinned the minimum.
- The default install folder is `$PROGRAMFILES64`, so it needs 64-bit Windows unless you set `install_dir`.
- No Windows version has been tested, so the oldest one that works is unknown.

Each file gets its own `File` line, so the uninstaller removes exactly the files of the package. Configuration files are
copied only when they are missing, and the uninstaller keeps them.

- `execution_level = "admin"` asks Windows for elevation, installs for all users and registers in `HKLM`. `user` installs
  for the current user and registers in `HKCU`.
- The version has four parts: the three numbers of `version` and the `release`. A part above 65535 stops the build.
- A symbolic link to an installed file is installed as a copy, with a warning. Any other link is skipped, with a warning.
- Dependencies, scripts, `license` and ghost entries have no place in an NSIS installer. Each one warns.
- If `post_install_exec` fails, the installer runs the uninstaller silently, removes the install folder and stops with
  an error. It cannot undo what the command itself did outside the install folder.

| Key                 | Values                                           | Default                                                   | Effect                                             |
| ------------------- | ------------------------------------------------ | --------------------------------------------------------- | -------------------------------------------------- |
| `install_dir`       | NSIS constant and folders, written with backslashes      | `$PROGRAMFILES64\NAME`, or `$LOCALAPPDATA\Programs\NAME` for `user` | Where the files go                       |
| `prefix`            | An absolute Unix path                            | `/usr`                                                    | Entry paths are taken relative to it. A path outside it stops the build |
| `compression`       | `lzma`, `zlib`, `bzip2`                          | `lzma`                                                    | The compression of the installer                   |
| `execution_level`   | `admin`, `user`                                  | `admin`                                                   | Who the program is installed for                   |
| `shortcuts`         | List of entry paths                              | None                                                      | One Start Menu shortcut for each. The package name must be one folder name |
| `post_install_exec` | One command line                                 | None                                                      | Runs in the install folder after the install. `$` is literal |

Checked under Wine 11 on Linux:

- A silent install, a byte-for-byte comparison of the payload, and a silent uninstall that leaves no folder and no registry key.
- A Start Menu shortcut, and names with `$` and quotes.
- The roll back when `post_install_exec` exits with a non-zero code. No files, folder or registry key are left.

Not checked: native Windows, the elevation prompt, and a real Windows program. The installer is unsigned.

## msix

An `.msix` is a zip with an `AppxManifest.xml`, a block map with a SHA-256 for each 64 KiB block, and the files below
`VFS/ProgramFilesX64/<name>`. The package is written unsigned. Windows refuses to install an unsigned package, so sign
it with a certificate whose subject equals `publisher`, for example with `signtool`, before it goes to a user.

- `arch` must be `amd64` or `arm64`. `all` stops the build, because an MSIX is for one architecture.
- The `name` is the identity name. It uses letters, digits, dots and dashes, 3 to 50 characters.
- The version has four parts: the three numbers of `version` and the `release`.
- Dependencies, scripts, symbolic links, ghost entries, modes and owners cannot be expressed. Each one warns.

| Key                      | Values                                        | Default                           | Effect                                   |
| ------------------------ | --------------------------------------------- | --------------------------------- | ---------------------------------------- |
| `executable`             | Path of a `.exe` entry. Required              | None                              | The program the package starts           |
| `logo`                   | Path of a PNG entry. Required                 | None                              | The tile and store logo. Use 150 px or more |
| `publisher`              | Subject such as `CN=Example Ltd`. Required    | None                              | Must equal the subject of the signing certificate |
| `root`                   | Folder, or `""` for the package root          | `VFS/ProgramFilesX64/<name>`      | Where `/` of the package goes           |
| `publisher_display_name` | Text                                          | The `CN=` value of `publisher`    | The publisher name Windows shows         |
| `display_name`           | Text                                          | The package name                  | The app name Windows shows               |
| `min_windows`            | Version                                       | `10.0.17763.0`                    | The oldest Windows version               |
| `max_tested`             | Version                                       | `10.0.22621.0`                    | The newest Windows version tested        |

Checked with `makemsix unpack` and `makemsix pack`, which validates the manifest schema, and with a script that hashes
every block and compares it with the block map. Not checked: installing on Windows, signing, and a real Windows program.

## appx

An `.appx` is the older name of the same container as an `.msix`: a zip with an `AppxManifest.xml`, a block map and the
files below `VFS/ProgramFilesX64/<name>`. pkgsmith writes both with one writer, so everything in [msix](#msix) applies
to `appx` too: the options, the rules for `name`, `version` and `arch`, the warnings, and the need to sign the package
before Windows installs it. The `appx` plugin sets `options` in the same way as `msix`.

Two things differ from `msix`:

- The file ends in `.appx`, for example `hello_1.2.3.1_x64.appx`.
- `min_windows` defaults to `10.0.14393.0` (Windows 10 1607, the Anniversary Update), the first Windows 10 version that
  runs a desktop program from a package. An `msix` defaults to `10.0.17763.0`. Set `min_windows` to change either.

Use `appx` when a tool or a Windows version needs the `.appx` extension. Use `msix` for Windows 10 1809 and later.

```python
package(
    "appx", name = "hello", version = "1.2.3", arch = "amd64", description = "Says hi",
    options = {
        "executable": "/usr/bin/hello.exe",
        "logo": "/usr/share/hello/logo.png",
        "publisher": "CN=Example Ltd",
    },
)
```

Checked like `msix`: `makemsix unpack` gives back every file unchanged, `makemsix pack` accepts the unpacked folder, and a
script hashes every block and compares it with the block map. Not checked: installing on Windows, signing, and a real
Windows program. Windows 10 1607 itself was not tried, so `10.0.14393.0` comes from Microsoft's documentation and not
from a run.

## freebsd

A FreeBSD package is a tar with `+COMPACT_MANIFEST` and `+MANIFEST` first, then the files below the prefix. pkgsmith
followed the `pkg` source for the manifest keys and checksums, but **has not run the real `pkg`** on the result, because
there is no FreeBSD here.

The package needs a maintainer and a homepage (`www`). The file name is `NAME-VERSION_RELEASE.pkg`, and `release` must
be a number.

- pkg needs an origin for every dependency, so each name in `depends` needs an entry in `dep_origins`, or the build
  stops. The version requirement is left out, because a pkgsmith dependency has none.
- `recommends`, `suggests`, `replaces` and ghost entries are ignored with a warning.

| Key            | Values                                    | Default           | Effect                                     |
| -------------- | ----------------------------------------- | ----------------- | ------------------------------------------ |
| `abi_version`  | The FreeBSD major version, such as `14`   | Required          | Sets `abi` to `FreeBSD:14:amd64`           |
| `origin`       | `category/port`                           | `misc/NAME`       | The origin of the package                  |
| `prefix`       | A path                                    | `/usr/local`      | The install prefix                         |
| `categories`   | List of text                              | The origin's category | The categories                         |
| `compression`  | `zstd`, `xz`, `gzip`, `bzip2`, `none`     | `zstd`            | Compression of the whole file              |
| `dep_origins`  | Map of dependency name to `category/port` | None              | The origin of each dependency              |

Scripts map to the pkg phases:

| Script         | pkg phase        |
| -------------- | ---------------- |
| `pre_install`  | `pre-install`    |
| `post_install` | `post-install`   |
| `pre_remove`   | `pre-deinstall`  |
| `post_remove`  | `post-deinstall` |

## xbps

An xbps package is a tar of the files with two property lists and two scripts. The revision in the file name comes from
`release`, which must be a positive integer. The version cannot hold `-` or `_`.

- A dependency without a version is written `name>=0`, because xbps refuses a bare name.
- `pre_install` and `post_install` share one `INSTALL` script, and `pre_remove` and `post_remove` share one `REMOVE`.
  xbps calls each with `pre` or `post` as the first argument.
- `recommends`, `suggests` and ghost files are ignored with a warning.

| Key           | Values                                  | Default | Effect                                                   |
| ------------- | --------------------------------------- | ------- | -------------------------------------------------------- |
| `compression` | `zstd`, `xz`, `gzip`, `bzip2`, `none`   | `zstd`  | Compression of the outer tar                             |
| `libc`        | `glibc`, `musl`                         | `glibc` | `musl` adds `-musl` to the architecture, as Void does    |

Checked in the Void glibc image with the real tools:

- `xbps-rindex -a`, `xbps-install` and `xbps-remove`
- The install script, `xbps-query -f` and `xbps-reconfigure`

Not checked: a real install on a musl system, `aarch64`, `noarch` and upgrades.

## gem

A gem is an uncompressed tar with `metadata.gz`, `data.tar.gz` and `checksums.yaml.gz`. Files under `prefix` go into the
gem. A file directly in `executable_dir` becomes an executable. A file anywhere else stops the build.

- The version `1.2.3-nightly.X` becomes `1.2.3.pre.nightly.X`, the form of `Gem::Version`.
- `depends` holds distro names, so it is ignored with a warning. Use `dependencies` for gems.
- Install and remove scripts, `recommends`, `suggests`, `conflicts`, `replaces` and `provides` are ignored with a warning.

| Key                     | Values                                  | Default                                  | Effect                                     |
| ----------------------- | --------------------------------------- | ---------------------------------------- | ------------------------------------------ |
| `prefix`                | Absolute path                           | `/usr/share/gems/gems/NAME-VERSION`      | Files below it go into the gem             |
| `executable_dir`        | Absolute path                           | `/usr/bin`                               | Files in it become `bin/NAME`              |
| `require_paths`         | List of relative paths                  | `lib`                                    | The `require_paths` of the spec            |
| `required_ruby_version` | A requirement such as `>= 2.7`          | Any Ruby                                 | The Ruby the gem needs                     |
| `dependencies`          | Map of gem name to requirement          | None                                     | Runtime dependencies                       |
| `platform`              | `ruby`, or a platform such as `x86_64-linux` | `ruby`                              | The platform of the gem                    |

A byte-for-byte comparison with a gem from `gem build` was not made. `gem install` and `gem spec` accept the result, and
a changed `data.tar.gz` or `metadata.gz` is refused by RubyGems.

## npm

An npm package is a gzip tar with everything below `package/`, and a generated `package.json`. Every file must be
under `prefix`, or the build stops. Modes are normalised the way `npm pack` does it: 0755 for a program, else 0644.

- `post_install` and `pre_install` become the `postinstall` and `preinstall` lifecycle scripts. npm runs no scripts on
  removal, so `pre_remove` and `post_remove` are ignored with a warning.
- `depends` is ignored with a warning. Use `dependencies` for npm ranges.
- Symbolic links and empty folders are dropped, as `npm pack` does, with a warning.
- The first description line becomes `description`. The rest becomes `README.md`.

| Key            | Values                                  | Default                              | Effect                                      |
| -------------- | --------------------------------------- | ------------------------------------ | ------------------------------------------- |
| `prefix`       | Absolute path                           | `/usr/lib/node_modules/NAME`         | The folder that becomes the package root    |
| `scope`        | Name without `@`                        | None                                 | Publishes as `@SCOPE/NAME`                  |
| `bin`          | Map of command to path under `prefix`   | None                                 | The `bin` field. The targets get mode 0755  |
| `main`         | Path under `prefix`                     | npm's default                        | The `main` field                            |
| `dependencies` | Map of name to semver range             | None                                 | Written as given                            |

A real `npm publish`, `dependencies` resolution and the `cpu` field on arm64 were not checked.

## conda

A `.conda` file is a zip of two zstd tars, `pkg-` for the files and `info-` for the metadata. Files go below `prefix`.
The version cannot hold `-`. A package with `arch = "all"` is `noarch`.

- `post_install`, `pre_install` and `pre_remove` become the `post-link`, `pre-link` and `pre-unlink` scripts. conda has
  no `post_remove`, so it is ignored with a warning.
- Absolute symbolic links below the prefix are rewritten as relative links, as conda-build does.
- The `depends` field of the package is ignored with a warning, unless the `depends` option is set.

| Key            | Values                                 | Default        | Effect                                           |
| -------------- | -------------------------------------- | -------------- | ------------------------------------------------ |
| `archive`      | `conda`, `tar.bz2`                     | `conda`        | The package archive                              |
| `prefix`       | Absolute path                          | `/usr`         | The folder that becomes the conda prefix         |
| `build_number` | Number                                 | The `release`  | The build number                                 |
| `build_string` | Letters, digits, `_`, `.`, `+`         | The build number | The build string                               |
| `depends`      | List of match specs, such as `zlib >=1.2` | None        | The `depends` of `index.json`                    |

Checked with micromamba against a channel indexed by `conda-index`, for `amd64` only, with both archives. A package
from `conda-build` was not compared.

## nupkg

A `.nupkg` is a zip with the nuspec, the Open Packaging Conventions parts and the files below one folder. NuGet needs an
author, so the package needs a `maintainer`. The version is kept as written, as a SemVer 2 pre-release.

- NuGet has no architecture. A package that is not `arch = "all"` warns, and files go in without a runtime folder. Put
  per-platform files under `runtimes/RID/` with `folder` and `prefix`.
- `depends` holds distro names, so it is ignored with a warning. Use `dependencies`.
- Scripts, symbolic links, `recommends`, `suggests`, `conflicts`, `replaces` and `provides` are ignored with a warning.

| Key            | Values                                                         | Default | Effect                                      |
| -------------- | -------------------------------------------------------------- | ------- | ------------------------------------------- |
| `folder`       | `lib`, `content`, `contentFiles`, `build`, `tools`, `runtimes` | `tools` | The top folder of the files                 |
| `prefix`       | Path prefix                                                    | None    | Removed from each path. Files outside it stop the build |
| `dependencies` | List of `id`, `version`, `target_framework`                    | None    | The `<dependencies>` of the nuspec          |

Checked with `dotnet add package` and `dotnet nuget push` against local folder feeds. Not checked: packages with several
target frameworks, `contentFiles`, and a comparison with `dotnet pack` output.
