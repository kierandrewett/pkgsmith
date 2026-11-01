# Formats

Each format is a plugin. `pkgsmith formats` lists what is installed. The options of a format go in the `options`
dict of the target that writes it, for example `options = {"section": "utils"}`. A key that the format does not know is
an error. The tables below list the keys of each format.

```python
pkg = package(name = "hello", version = "1.0.0", description = "A small program")

target(pkg, "deb", distro = "debian", options = {"section": "utils", "compression": "gzip"})
target(pkg, "tar", options = {"compression": "zstd", "top_folder": True})
```

A value is a Starlark value: text in quotes, `True` or `False`, numbers, lists and dicts. The tables write the
options of each format as keys of that dict. A format with no options needs no `options`.

| Format      | File                             | For                                  | Verified with                       |
| ----------- | -------------------------------- | ------------------------------------ | ----------------------------------- |
| `rpm`       | `.rpm`                           | Fedora, openSUSE, RHEL               | `rpm` in Fedora 43, openSUSE        |
| `deb`       | `.deb`                           | Debian, Ubuntu                       | `dpkg` in Debian 13                 |
| `ipk`       | `.ipk`                           | OpenWrt                              | Layout only                         |
| `archlinux` | `.pkg.tar.zst`                   | Arch Linux                           | `pacman` in Arch                    |
| `apk`       | `.apk`                           | Alpine                               | `apk` in Alpine 3.22                |
| `slackware` | `.txz`                           | Slackware                            | `installpkg` in Slackware 15.0      |
| `snap`      | `.snap`                          | snapd                                | `unsquashfs` only, not snapd        |
| `oci`       | `.oci.tar`                       | podman, docker, containerd           | `podman load` and `run`             |
| `appimage`  | `.AppImage`                      | Linux desktops                       | The runtime, without FUSE           |
| `wheel`     | `.whl`                           | pip                                  | `pip`, `twine` in Python 3.14       |
| `xbps`      | `.xbps`                          | Void Linux                           | `xbps-install` in Void              |
| `gem`       | `.gem`                           | RubyGems                             | `gem install` in the latest Ruby    |
| `npm`       | `.tgz`                           | npm                                  | `npm install` in the latest Node    |
| `conda`     | `.conda`, `.tar.bz2`             | conda, micromamba                    | `micromamba create` from a channel  |
| `nupkg`     | `.nupkg`                         | NuGet                                | `dotnet add package`, local feed    |
| `flatpak`   | `.flatpak`                       | Flatpak                              | `flatpak install` and `run` in Debian 13 |
| `dmg`       | `.dmg`                           | macOS                                | `fsck.hfsplus`, `7z`, `dmg2img`, not macOS |
| `nix`       | `.nix.tar.xz`, `.gz`, `.zst`, `.bz2` | Nix and NixOS, with or without flakes | `nix-build`, and `nix build` and `nix profile install` of the flake, in `nixos/nix` |
| `msi`       | `.msi`                           | Windows                              | `msitools` read and extract, not Windows |
| `nsis`      | `.exe`                           | Windows                              | `makensis`, run under Wine, not native Windows |
| `msix`      | `.msix`                          | Windows 10 and 11                    | `makemsix` unpack and pack only     |
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
pkg = package(name = "hello", version = "1.0.0", description = "A small program")

target(
    pkg, "snap",
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
| `entrypoint`  | List of text              | None          | The program the container runs               |
| `cmd`         | List of text              | None          | Default arguments                            |
| `env`         | Map                       | None          | Environment variables                        |
| `workdir`     | Path                      | None          | The folder the program starts in             |
| `user`        | Name or number, `user:group` | None       | The user the program runs as                 |
| `ports`       | List like `8080/tcp`      | None          | Ports the program listens on                 |
| `labels`      | Map                       | None          | Extra labels                                 |
| `repository`  | Text                      | The name      | The image name                               |
| `tag`         | Text                      | The version   | The image tag                                |
| `compression` | `gzip`, `none`            | `gzip`        | Compression of the layer                     |

Load it with `podman load -i hello-1.0.0-amd64.oci.tar`.

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

A `.dmg` is an Apple disk image: a UDIF container that holds one HFS+ volume with your files. pkgsmith writes both parts
itself, in pure Rust, so no tool is needed to build one. The volume writer follows Apple's public
[TN1150](https://developer.apple.com/library/archive/technotes/tn1150/) and was written without reading GPL code.

A disk image has no install path. Files sit on the volume in the same layout as the installed system, for example
`/usr/bin/hi`. Use `prefix` to put them in a folder, such as `/Applications`.

- File names must be ASCII. HFS+ compares other names after Unicode normalisation and case folding, which pkgsmith does
  not implement, so any other name stops the build with the path in the error.
- Two names in one folder that differ only in ASCII case stop the build.
- Symbolic links stop the build, because pkgsmith cannot write HFS+ symbolic links.
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

A Nix package is an archive that unpacks to `NAME-VERSION/`. That folder holds `default.nix`, `flake.nix` and `files/`. The
derivation builds nothing. It copies `files/` into the store path, so the files must be built already, as for the other
formats. A Nix package for a distro build links against that distro's libraries, so see `patchelf` below.

Build it with `nix-build`, or use the flake, which works on the archive itself. Replace the address with where you
host the file:

```console
$ nix-build NAME-VERSION
$ nix build --no-write-lock-file 'https://example.org/hi-1.0.0-amd64.nix.tar.xz'
$ nix profile install --no-write-lock-file 'https://example.org/hi-1.0.0-amd64.nix.tar.xz#hi'
```

Put `#NAME` after the address of `nix profile install`, so that the profile entry is called `NAME`. Then `nix profile remove NAME` removes it.

The flake takes `nixpkgs` as an input and does not pin it, so a remote flake needs `--no-write-lock-file`. The flake
needs the `nix-command` and `flakes` features. `default.nix` does not need them.

- Entries below `prefix` lose it: `/usr/bin/hi` is `$out/bin/hi`. An entry outside the prefix goes below `$out` with its
  whole path and warns, because a Nix package cannot write outside its store path.
- Nix keeps only the executable bit of a mode and ignores owners. A symbolic link with an absolute target warns.
- Distro dependency names mean nothing to Nix, so `depends` and the other lists warn. Scripts, ghost entries and the
  maintainer warn too.
- `license` is written as a `lib.licenses` attribute when pkgsmith knows the SPDX id, and by its id alone otherwise.

| Key             | Values                                    | Default                                    | Effect                                            |
| --------------- | ----------------------------------------- | ------------------------------------------ | ------------------------------------------------- |
| `prefix`        | An absolute path                          | `/usr`                                     | Entry paths are taken relative to it              |
| `inputs`        | List of nixpkgs attributes, such as `openssl` | None                                   | `buildInputs`                                     |
| `native_inputs` | List of nixpkgs attributes                | None                                       | `nativeBuildInputs`                               |
| `patchelf`      | `true`, `false`                           | `false`                                    | Adds `autoPatchelfHook`, which points compiled programs at the libraries of `inputs` |
| `platforms`     | List such as `x86_64-linux`               | From `arch`. `all` is every system         | `meta.platforms`, and the systems of the flake    |
| `nixpkgs`       | A flake reference                         | `github:NixOS/nixpkgs/nixos-unstable`      | The `nixpkgs` input of `flake.nix`                |
| `compression`   | `xz`, `gzip`, `zstd`, `bzip2`, `none`     | `xz`                                       | The compression of the archive                    |

Checked in `nixos/nix` with `scripts/verify-formats.sh nix`:

- `nix-build` builds the package. The store path holds the program with its executable bit, the symbolic link and the
  other files.
- `nix build` and `nix profile install` of the flake, from a `tarball+file` address, give the same store path and a
  program that runs.

Not checked: `patchelf` with a real compiled program, a package with `inputs`, `aarch64`, macOS, and a binary cache.

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
- Dependencies, scripts, `license`, symbolic links, ghost entries, owners and modes have no place in an MSI. Each one
  warns. A configuration file is installed as an ordinary file.

| Key            | Values                              | Default                                              | Effect                                              |
| -------------- | ----------------------------------- | ---------------------------------------------------- | --------------------------------------------------- |
| `scope`        | `machine`, `user`                   | `machine`                                            | `machine` installs for all users and needs elevation. `user` installs for the current user |
| `install_dir`  | A folder, written with `\` or `/`   | The package name, and `Programs\NAME` for `user`     | The folder below Program Files or the local application data folder |
| `manufacturer` | Text                                | The maintainer                                       | The manufacturer Windows shows. One of the two is required |
| `upgrade_code` | A GUID                              | Derived from the package name                        | Every version of a package shares it                |
| `prefix`       | An absolute Unix path               | `/usr`                                               | Entry paths are taken relative to it. A path outside it stops the build |

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

By hand, the exported tables were read for both scopes.

Not checked:

- Installing and uninstalling with `msiexec` on Windows.
- Validation with the ICE rules. A `user` package will fail ICE38, which wants a registry key as the key path of each
  component in the user profile.
- The sequence numbers beyond the Microsoft documentation. `FindRelatedProducts` at 25 is a convention, and the
  documentation only requires it to come before `RemoveExistingProducts`.

## nsis

An `.exe` made by NSIS is a self-extracting Windows installer that adds the program to Add/Remove Programs. NSIS has no
practical writer in Rust, so pkgsmith writes an `.nsi` script and runs `makensis`.

`makensis` must be on `PATH` when you build. Without it the build stops and names the packages that carry it: `nsis` on
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
| `archive`      | `conda`, `tar.bz2`                     | `conda`        | The package container                            |
| `prefix`       | Absolute path                          | `/usr`         | The folder that becomes the conda prefix         |
| `build_number` | Number                                 | The `release`  | The build number                                 |
| `build_string` | Letters, digits, `_`, `.`, `+`         | The build number | The build string                               |
| `depends`      | List of match specs, such as `zlib >=1.2` | None        | The `depends` of `index.json`                    |

Checked with micromamba against a channel indexed by `conda-index`, for `amd64` only, with both containers. A package
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
