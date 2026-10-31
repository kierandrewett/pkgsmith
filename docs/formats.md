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

Scripts map to the pkg phases: `pre_install` to `pre-install`, `post_install` to `post-install`, `pre_remove` to
`pre-deinstall` and `post_remove` to `post-deinstall`.

## xbps

An xbps package is a tar with `props.plist`, `files.plist` and the files, plus an `INSTALL` and a `REMOVE` script. The
revision in the file name comes from `release`, which must be a positive integer, and the version cannot hold `-` or `_`.

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
