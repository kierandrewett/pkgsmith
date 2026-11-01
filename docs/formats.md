# Formats

Each format is a plugin. `pkgsmith formats` lists what is installed. The options of a format go in a section of the
manifest with the name of the format, for example `deb:`. A key that the format does not know is an error.

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

| Section | Key           | Values                | Default    | Effect                                      |
| ------- | ------------- | --------------------- | ---------- | ------------------------------------------- |
| `deb`   | `section`     | Text                  | `misc`     | The `Section` field                         |
| `deb`   | `priority`    | Text                  | `optional` | The `Priority` field                        |
| `deb`   | `compression` | `xz`, `zstd`, `gzip`  | `xz`       | Compression of the data and control parts   |
| `ipk`   | `arch`        | OpenWrt architecture  | From `arch` | For example `aarch64_generic`              |

## archlinux

| Key        | Values | Default          | Effect               |
| ---------- | ------ | ---------------- | -------------------- |
| `packager` | Text   | The maintainer   | The `packager` field |

## tar and zip

| Section | Key           | Values                                | Default | Effect                                         |
| ------- | ------------- | ------------------------------------- | ------- | ---------------------------------------------- |
| `tar`   | `compression` | `xz`, `gzip`, `zstd`, `bzip2`, `none` | `xz`    | The compression and the file extension         |
| `tar`   | `top_folder`  | `true`, `false`                       | `false` | Put everything below `{name}-{version}/`       |
| `zip`   | `top_folder`  | `true`, `false`                       | `false` | Put everything below `{name}-{version}/`       |

## slackware

The package is a tar of the files with `install/slack-desc` and `install/doinst.sh`. Configuration files ship as
`NAME.new` and are moved into place only when no file of that name exists.

The format has only a post-install script. These manifest fields are ignored with a warning:

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

```yaml
snap:
  base: core24
  apps:
    hello:
      command: /usr/bin/hello
      plugs: [network]
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
| `platform_tag`    | For example `manylinux_2_17_x86_64`     | `any` for `arch: all`, else required | Tag in the file name  |
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
| `root`                   | Folder, or `""` for the package root          | `VFS/ProgramFilesX64/<name>`      | Where `/` of the manifest goes           |
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

The manifest needs a maintainer and a homepage (`www`). The file name is `NAME-VERSION_RELEASE.pkg`, and `release` must
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

| Manifest       | pkg phase        |
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
The version cannot hold `-`. A package with `arch: all` is `noarch`.

- `post_install`, `pre_install` and `pre_remove` become the `post-link`, `pre-link` and `pre-unlink` scripts. conda has
  no `post_remove`, so it is ignored with a warning.
- Absolute symbolic links below the prefix are rewritten as relative links, as conda-build does.
- The manifest `depends` is ignored with a warning, unless the `depends` option is set.

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
author, so the manifest needs a `maintainer`. The version is kept as written, as a SemVer 2 pre-release.

- NuGet has no architecture. A package that is not `arch: all` warns, and files go in without a runtime folder. Put
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
