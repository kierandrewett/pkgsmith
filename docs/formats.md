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
