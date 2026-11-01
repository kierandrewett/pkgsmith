# Manifest reference

A manifest is one YAML file for one package. The default name is `pkgsmith.yaml`. pkgsmith rejects keys it does not
know, so a typo is an error and not a silent no-op.

Text values accept `${NAME}` variables and `${{ expression }}` expressions. See
[matrix and expressions](matrix-and-expressions.md). [Scripts](scripts.md) and the `setup` and `build` scripts take
expressions only, because `${HOME}` in a script belongs to the shell.

## Package

| Key           | Type   | Default  | Meaning                                                                                  |
| ------------- | ------ | -------- | ---------------------------------------------------------------------------------------- |
| `name`        | text   | required | Package name. Lowercase letters, digits and `+ - . _`. Starts with a letter or digit     |
| `version`     | text   | required | `MAJOR.MINOR.PATCH` or `MAJOR.MINOR.PATCH-prerelease`. See [versions](versions.md)       |
| `release`     | text   | `1`      | Package revision for one version. Reset it to `1` when the version changes               |
| `build_axes`  | list   | none     | The matrix keys the build uses. It sees only these, so jobs equal in them share it. See [build environment](build-environment.md#one-build-for-several-distros) |
| `epoch`       | number | `0`      | Makes the version newer than every version with a lower epoch. `0` leaves it out. See [versions](versions.md#epoch) |
| `arch`        | text   | `amd64`  | `amd64`, `arm64` or `all` for packages without compiled code. A matrix `arch` overrides it |
| `description` | text   | required | The first line is the summary. The rest is the long text                                 |
| `maintainer`  | text   | none     | `Name <email>`                                                                           |
| `homepage`    | text   | none     | URL                                                                                      |
| `license`     | text   | none     | License expression, for example `GPL-3.0-or-later`                                       |
| `env`         | map    | none     | Defaults for `${NAME}` and `${{ env.NAME }}`. The process environment and `--build-arg` win |

## Build

| Key       | Type   | Meaning                                                                                       |
| --------- | ------ | --------------------------------------------------------------------------------------------- |
| `image`   | text   | Container image of the build environment. A `matrix` entry may set its own. See [build environment](build-environment.md) |
| `setup`   | script | Runs once in the image. Not tracked. The result is cached. A `matrix` entry may set its own   |
| `build`   | script | Runs in the image with the build context at `/src`. Files it installs outside `/src` are packaged |
| `exclude` | list   | Paths the package leaves out. Prefixes or globs, for example `/usr/share/doc/**`              |
| `matrix`  | map    | Axes, `include` and `exclude`. See [matrix and expressions](matrix-and-expressions.md)        |
| `include` | list   | Other manifest files to merge underneath this one. See [including other files](#including-other-files) |

`image` needs `build`. A manifest without `image` does not run anything. It packages the files that `contents` names,
read from the folder of the manifest.

## Files

| Key        | Type | Meaning                                                      |
| ---------- | ---- | ------------------------------------------------------------ |
| `contents` | list | Files to add, or to change in the tracked list. See [contents](contents.md) |

## Dependencies and relations

Each key is a list of package names, written in the syntax of the target format. See [dependencies](dependencies.md).

| Key          | Meaning                                         |
| ------------ | ----------------------------------------------- |
| `depends`    | Packages that must be installed                 |
| `recommends` | Packages installed by default, but not required |
| `suggests`   | Packages that are useful together               |
| `conflicts`  | Packages that cannot be installed together      |
| `replaces`   | Packages this one takes over from               |
| `provides`   | Names this package also answers to              |

## Scripts

`scripts` has four keys: `pre_install`, `post_install`, `pre_remove`, `post_remove`. See [scripts](scripts.md).

## Overrides

`overrides` replaces dependency lists and scripts for one format or one distro:

```yaml
depends: [glibc]
overrides:
  deb:
    depends: [libc6]
  opensuse:
    depends: [glibc]
    scripts:
      post_install: |
        echo "openSUSE"
```

The keys of `overrides` are format names or distro names:

- Format names: `tar`, `zip`, `deb`, `rpm`, `archlinux`, `apk`, `ipk`.
- Distro names: `fedora`, `opensuse`, `debian`, `ubuntu`, `archlinux`.

The format applies first and the distro on top of it. A list that is present replaces the whole base list. It does not
merge with it. The distro of a job comes from the matrix axis `distro`, or from `--distro`.

## Format options

A top-level key with the name of a format holds the options of that format, for example `deb:` or `tar:`. The plugin
that writes the format defines the keys. A top-level key that is neither a field of the manifest nor a format of a
loaded plugin is an error. See [plugins](plugins.md#format-options) for the keys of the built-in formats.

```yaml
deb:
  section: utils
tar:
  compression: zstd
  top_folder: true
```

## Including other files

`include` pulls other manifests into this one, so the details every package shares are written once. It is useful when
several manifests describe the same program, for example one per kind of installer.

```yaml
# pkgsmith.yaml
include: [common.yaml, installers.yaml]
description: A small program, as a release archive
```

`include` is a file name or a list of file names. A name is relative to the folder of the file that holds it. The files
can include other files, up to eight levels deep. A loop, a missing file or a value that is not a file name stops the
command with the path of each file on the way.

The files are merged in order, and then the file that holds the `include` is merged on top. So the including file wins.

| Value kind                 | What a later file does to an earlier one                      |
| -------------------------- | ------------------------------------------------------------- |
| A map, such as `tar:`      | Merges key by key. Keys that only the earlier file has stay   |
| Text, a number or a flag   | Replaces it                                                   |
| A list, such as `contents:` or `depends:` | Replaces the whole list. Lists are not joined  |

A list is replaced and not joined, because a joined list cannot remove an entry. To add to a list, repeat the shared
entries in the including file, or keep the shared part in one included file and the rest in another.

Paths inside the manifest, such as `src` of a `contents` entry and script file names, are relative to the folder of the
manifest that you build, not to the included file. [examples/include](../examples/include) shows three files that
include each other.

## A complete example

```yaml
name: bingux-config
version: ${VERSION}
release: 1
description: |
  Configuration command for the Bingux desktop
  Reads, changes and checks the Bingux configuration file.
maintainer: Kieran Drewett <kieran@drewett.dev>
homepage: https://github.com/kierandrewett/bingux
license: GPL-3.0-or-later

matrix:
  distro: [fedora, debian]
  arch: [amd64, arm64]
  include:
    - { distro: fedora, format: rpm, image: "fedora:43", setup: dnf -y install cargo }
    - { distro: debian, format: deb, image: "debian:13", setup: apt-get update -qq && apt-get install -y cargo }

build: |
  cargo build --release --manifest-path crates/config/Cargo.toml
  install -Dm755 target/release/bingux-config /usr/bin/bingux-config
exclude: [/usr/share/doc/**]

depends: [glibc]
overrides:
  deb:
    depends: [libc6]

scripts:
  post_install: |
    echo "bingux-config installed"
```
