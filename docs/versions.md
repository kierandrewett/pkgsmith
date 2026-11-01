# Versions

Write one version in the script. pkgsmith spells it in the way each format orders versions.

A version is `MAJOR.MINOR.PATCH`, or `MAJOR.MINOR.PATCH-prerelease`. The pre-release text may contain letters, digits,
dots and hyphens. Anything else is an error: `1.2`, `v1.2.3`, `1.2.3-` and `1.2.x` are refused.

## Why pre-releases need care

A nightly build must sort **after** the last release and **before** the release it leads up to. Package managers
disagree on what makes a version older. In deb and rpm, `~` sorts before everything, including the end of the string.
pacman has no such character. Alpine uses version suffixes such as `_pre`. A hyphen is not allowed in some of them.

## Examples

Release `0.1.12`:

| Format      | Version in the file name | Note                                  |
| ----------- | ------------------------ | ------------------------------------- |
| `tar`, `zip`| `0.1.12`                 |                                       |
| `deb`, `ipk`| `0.1.12-1`               | `-1` is the `release`                 |
| `rpm`       | `0.1.12` release `1`     | `hello-0.1.12-1.x86_64.rpm`           |
| `archlinux` | `0.1.12-1`               | `pkgver-pkgrel`                       |
| `apk`       | `0.1.12-r1`              | `-rN` is the `release`                |

Nightly `0.1.13-nightly.20261031.142.gabc1234`:

| Format      | Version                                    | Rule                                              |
| ----------- | ------------------------------------------ | ------------------------------------------------- |
| `tar`, `zip`| `0.1.13-nightly.20261031.142.gabc1234`     | As written                                        |
| `deb`, `ipk`| `0.1.13~nightly.20261031.142.gabc1234-1`   | `-` becomes `~`                                   |
| `rpm`       | `0.1.13~nightly.20261031.142.gabc1234`     | `-` becomes `~`                                   |
| `archlinux` | `0.1.13nightly20261031.142-1`              | The commit text goes. A name joins the number after it |
| `apk`       | `0.1.13_pre20261031142-r1`                 | Suffix `_pre` with the digits of the build        |

## Ordering

For one package, these are in increasing order in every format:

```
0.1.12  <  0.1.13~nightly.20261031.142  <  0.1.13~nightly.20261101.150  <  0.1.13
```

For Arch, the rule drops the commit identifier (`g` and hexadecimal text of 7 to 40 digits), because pacman compares
runs of letters and digits and the commit text would break the order that the date and the commit count already give.

## In the script

```python
expose(tree("/", "/"))
package(
    "tar",
    name = "hello",
    version = arg("VERSION", "0.4.2"),
    release = "1",
    description = "A small program",
)
```

```console
$ pkgsmith build . --build-arg VERSION=0.4.3-nightly.20261031.142.gabc1234
```

`release` counts rebuilds of one version. Raise it when you fix the packaging and not the program. Set it back to `1`
when the version changes.

`tar` and `zip` get the version as written.

## Epoch

An epoch makes a version sort above every version with a lower epoch, whatever the numbers are. Use it only to repair
a version that went the wrong way, for example after a project changed its numbering.

```python
expose(tree("/", "/"))
package("rpm", name = "hello", version = "0.1.12", epoch = 1, description = "A small program")
```

| Format      | Where the epoch goes                                              |
| ----------- | ----------------------------------------------------------------- |
| `rpm`       | The `Epoch` tag. The file name has no epoch                       |
| `deb`       | `1:0.1.12-1` in the `Version` field. The file name has no epoch   |
| `archlinux` | `1:0.1.12-1` in `pkgver`, and in the file name, as `makepkg` does |
| others      | Ignored                                                           |

A dependency on a version with an epoch carries the epoch in the syntax of its format: `gnoblin = 1:0.1.12-1` for `rpm`.

