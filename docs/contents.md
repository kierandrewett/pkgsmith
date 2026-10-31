# Contents

The files of a package come from two places, and `contents` joins them.

1. **Tracked files.** When the manifest has an `image` and a `build`, every file the build installs outside `/src` is in
   the package. See [build environment](build-environment.md).
2. **`contents` entries.** Each entry adds a file, or replaces the tracked entry that has the same path.

An entry that names the same `dst` as a tracked file replaces it. This is how you mark a tracked file as
configuration, or change its mode, without listing every other file.

## Entry keys

| Key     | Meaning                                                                                       |
| ------- | --------------------------------------------------------------------------------------------- |
| `dst`   | Absolute path on the installed system. Required                                               |
| `src`   | Where the content comes from. Required for `tree` and `symlink`. Not used by `dir` and `ghost`. A `file` or `config` entry without `src` changes the tracked file at `dst` |
| `type`  | `file` (default), `config`, `tree`, `dir`, `symlink`, `ghost`                                 |
| `mode`  | Permissions as octal text, for example `"0755"`. Default: the mode of the source file         |
| `owner` | User name. Default `root`                                                                     |
| `group` | Group name. Default `root`                                                                    |
| `only`  | A list of formats. The entry applies to those formats only, for example `[deb, ipk]`          |
| `if`    | An [expression](matrix-and-expressions.md). The entry applies when it is true                 |

`src` is relative to the folder of the manifest. Variables and expressions work in `src` and `dst`.

## Types

| `type`    | `src` is                                    | Result                                                                                  |
| --------- | ------------------------------------------- | --------------------------------------------------------------------------------------- |
| `file`    | A file, or a glob such as `man/*.1`         | One file at `dst`. For a glob, or when `dst` ends with `/`, `dst` is a folder           |
| `config`  | A file                                      | A file the package manager keeps when the user changed it                               |
| `tree`    | A folder                                    | The folder, copied recursively, including the folder and every folder below it          |
| `dir`     | none                                        | An empty folder that the package owns                                                   |
| `symlink` | The link target as it reads after install   | A symbolic link at `dst`                                                                |
| `ghost`   | none                                        | A path the package owns but does not carry. Only `rpm` records it                       |

How a format records `config`:

| Format      | Record                          |
| ----------- | ------------------------------- |
| `deb`, `ipk`| Listed in `conffiles`           |
| `rpm`       | `%config(noreplace)` flag       |
| `archlinux` | Listed in `backup`              |
| `apk`       | No record. apk protects `/etc` itself |
| `tar`       | No record                       |

Files that a tracked build installs under `/etc` are marked as configuration without an entry.

## Parent folders

A package must not own `/usr` or `/usr/bin`. A folder that no entry names is not part of the `rpm`.

The formats that need folder entries to unpack files (`deb`, `ipk`, `apk`, `archlinux`) add the parents of every file
as folders owned by root with mode `0755`. The package manager does not change the mode of a folder that already
exists.

## Tracked files and excludes

The tracked list comes from the container's changes. A new folder is claimed. A changed folder that already existed is
not. Deleted paths are ignored.

These paths are never packaged:

`/tmp`, `/var/tmp`, `/root`, `/run`, `/proc`, `/sys`, `/dev`, `/var/cache`, `/var/log`, `/var/lib/dnf`,
`/var/lib/rpm`, `/var/lib/pacman`, `/var/lib/dpkg`, `/var/lib/apk`, `/var/lib/zypp`, `/usr/lib/sysimage`,
`/etc/ld.so.cache`, `/etc/mtab`, `/etc/resolv.conf`, `/etc/hosts`, `/etc/hostname`, `/lib/apk`, `/src`

Add your own with `exclude`. An entry is a path prefix or a glob:

```yaml
exclude:
  - /usr/share/doc/**
  - /usr/lib/debug
  - /usr/share/man
```

## Examples

Mark one tracked file as configuration, keep everything else:

```yaml
contents:
  - { dst: /etc/app.conf, type: config }
```

An entry without `src` keeps the content of the tracked file and changes its flags. `mode`, `owner` and `group` work
the same way. It is an error when the build did not install `dst`.

Add a file from the source tree for Debian and OpenWrt only:

```yaml
contents:
  - src: packaging/app.service
    dst: /usr/lib/systemd/system/app.service
    only: [deb, rpm]
```

Link a command name:

```yaml
contents:
  - { type: symlink, src: /usr/lib/app/bin/app, dst: /usr/bin/app }
```

## Without a container

A manifest without `image` lists every file itself. Build the program yourself and stage it with `DESTDIR`, then point
`src` at the stage:

```yaml
contents:
  - src: ${STAGE}/usr/bin/tool
    dst: /usr/bin/tool
    mode: 0755
```

```console
$ make install DESTDIR=/tmp/stage PREFIX=/usr
$ pkgsmith build . --build-arg STAGE=/tmp/stage
```
