# pkgsmith

pkgsmith builds distribution packages from one manifest per package. The manifest names the package, the files, the
install scripts and the dependencies. The same file produces every format.

It works like [nFPM](https://nfpm.goreleaser.com/), with a manifest that behaves like a GitHub Actions workflow: a
`matrix` runs the build once per combination, and `${{ }}` expressions and `if:` see the combination.

| Format      | File                       | Status      |
| ----------- | -------------------------- | ----------- |
| `tar`       | `.tar.xz`, `.gz`, `.zst`   | works       |
| `deb`       | `.deb`                     | works       |
| `archlinux` | `.pkg.tar.zst`             | works       |
| `ipk`       | `.ipk` (OpenWrt)           | works       |
| `rpm`       | `.rpm`                     | planned     |
| `apk`       | `.apk` (Alpine)            | planned     |
| `snap`      | `.snap`                    | planned     |
| `flatpak`   | `.flatpak`                 | planned     |
| `nix`       | Nix expression             | planned     |

## Use

```sh
pkgsmith plan  package.yaml                       # what would be built
pkgsmith build package.yaml --set STAGE=/path/to/stage --set VERSION=0.1.0 -o out
pkgsmith build package.yaml --job arch=arm64 --format deb -o out
pkgsmith check package.yaml --format deb          # the files one build would package
pkgsmith formats
```

pkgsmith does not build your program. It packages a staged prefix, a folder laid out like the installed system
(`usr/bin/...`), for example the `DESTDIR` of `make install`.

## The manifest

See [examples/bingux-config.yaml](examples/bingux-config.yaml).

| Key                                     | Meaning                                                                                  |
| --------------------------------------- | ---------------------------------------------------------------------------------------- |
| `name`, `version`, `release`, `arch`    | Package identity. `version` is `MAJOR.MINOR.PATCH` or `MAJOR.MINOR.PATCH-prerelease`      |
| `description`                           | First line is the summary, the rest is the long text                                     |
| `maintainer`, `homepage`, `license`     | Metadata                                                                                 |
| `depends`, `recommends`, `suggests`, `conflicts`, `replaces`, `provides` | Lists of package names                                  |
| `contents`                              | Files, see below                                                                         |
| `scripts`                               | `pre_install`, `post_install`, `pre_remove`, `post_remove`. Inline text, or a file name  |
| `overrides`                             | Lists and scripts per format (`deb`) or per distro (`fedora`). A distro wins             |
| `matrix`                                | Axes, `include` and `exclude`, like `strategy.matrix`                                    |
| `env`                                   | Defaults for `${NAME}` and `${{ env.NAME }}`                                             |

`contents` entries have a `dst` (absolute) and, depending on `type`, a `src`:

| `type`             | Meaning                                                                                 |
| ------------------ | --------------------------------------------------------------------------------------- |
| `file` (default)   | One file, or the files a glob matches. `dst` is a folder for a glob or when it ends `/` |
| `tree`             | A folder copied recursively, including the folder and every folder below it             |
| `dir`              | An empty folder the package owns                                                        |
| `symlink`          | A symbolic link. `src` is the target as it reads on the installed system                |
| `config`           | A file the package manager keeps when the user changed it                               |
| `ghost`            | A file the package owns but does not carry                                              |

An entry may also set `mode` (`"0755"`), `owner`, `group`, `only: [deb, ipk]` and `if: <expression>`.

### Matrix and expressions

The axes `format`, `arch` and `distro` steer the build. Any other axis is a variable.

```yaml
matrix:
  arch: [amd64, arm64]
  format: [deb, archlinux]
  exclude:
    - { format: archlinux, arch: arm64 }
contents:
  - src: ${STAGE}-${{ matrix.arch }}/usr/bin/tool
    dst: /usr/bin/tool
  - src: tool.conf
    dst: /etc/tool.conf
    type: config
    if: format == 'deb'
```

Expressions support `matrix.NAME`, `env.NAME`, `format`, `distro`, `arch`, strings, numbers, `true`, `false`, `==`,
`!=`, `&&`, `||`, `!`, parentheses, and `contains`, `startsWith`, `endsWith`, `format`, `lower` and `upper`. `==` ignores
case. `&&` and `||` return a value, so `${{ matrix.arch == 'arm64' && 'aarch64' || 'x86_64' }}` works.

### Versions

Write one version. pkgsmith spells it for each format so that a nightly sorts after the last release and before the
next one: `0.1.13-nightly.20261031.142.gabc1234` becomes `0.1.13~nightly.20261031.142.gabc1234` for deb,
`0.1.13nightly20261031.142` for Arch and `0.1.13_pre20261031142` for apk.

## Build and test

```sh
cargo build --release
cargo test
```

The packages were checked by installing them: a `.deb` in Debian 13 (`dpkg -i`, `dpkg -V`, `dpkg -r`) and an Arch
package in Arch Linux (`pacman -U`, `pacman -Qkk`, `pacman -R`).
