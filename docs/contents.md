# Contents

The files of a package come from two places, and `contents` joins them.

1. **Tracked files.** When a target has a job, each file its steps install outside `/src` is in
   the package. See [build environment](build-environment.md).
2. **`contents` entries.** Each entry adds a file, or replaces the tracked entry that has the same path.

An entry that names the same `dst` as a tracked file replaces it. This is how you mark a tracked file as
configuration, or change its mode, without listing every other file.

## Entries

Each helper returns one entry. Put the entries in the `contents` list of `package`, or of one `target`.

| Function                                        | Entry                                                       |
| ----------------------------------------------- | ----------------------------------------------------------- |
| `file(src, dst, mode, owner, group, config=False)` | A file, or the files that a glob matches                 |
| `config(src, dst, mode, owner, group)`          | A configuration file                                        |
| `tree(src, dst, mode, owner, group)`            | A folder, copied recursively                                |
| `symlink(link_target, dst)`                     | A symbolic link                                             |
| `directory(dst, mode, owner, group)`            | An empty folder that the package owns                       |
| `ghost(dst)`                                    | A path the package owns but does not carry                  |

| Argument | Meaning                                                                                                 |
| -------- | ------------------------------------------------------------------------------------------------------- |
| `dst`    | Absolute path on the installed system. Required                                                         |
| `src`    | Where the content comes from, relative to the folder of the script. For `symlink`, the link target. `None` changes the tracked file at `dst` |
| `mode`   | Permissions as octal text, for example `"0755"`. Default: the mode of the source file                   |
| `owner`  | User name. Default `root`                                                                               |
| `group`  | Group name. Default `root`                                                                              |

To apply an entry to some formats only, build the list in the script:

```python
extra = []
if fmt in ["deb", "rpm"]:
    extra.append(file("packaging/app.service", "/usr/lib/systemd/system/app.service"))
```

## Types

| Entry       | `src` is                                    | Result                                                                                  |
| ----------- | ------------------------------------------- | --------------------------------------------------------------------------------------- |
| `file`      | A file, or a glob such as `man/*.1`         | One file at `dst`. For a glob, or when `dst` ends with `/`, `dst` is a folder           |
| `config`    | A file                                      | A file the package manager keeps when the user changed it                               |
| `tree`      | A folder                                    | The folder, copied recursively, including the folder and every folder below it          |
| `directory` | none                                        | An empty folder that the package owns                                                   |
| `symlink`   | The link target as it reads after install   | A symbolic link at `dst`                                                                |
| `ghost`     | none                                        | A path the package owns but does not carry. Only `rpm` records it                       |

How a format records a configuration file:

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

Add your own with the `exclude` list of the job. An entry is a path prefix or a glob:

```python
build = job(
    runs_on = "debian:13",
    steps = [step("make install PREFIX=/usr")],
    exclude = ["/usr/share/doc/**", "/usr/lib/debug", "/usr/share/man"],
)
```

## Examples

Mark one tracked file as configuration, keep everything else:

```python
contents = [config(None, "/etc/app.conf")]
```

An entry with `src=None` keeps the content of the tracked file and changes its flags. `mode`, `owner` and `group` work
the same way. It is an error when the build did not install `dst`.

Add a file from the source tree for Debian and Fedora only:

```python
pkg = package(name = "app", version = "1.0.0", description = "An app")
unit = file("packaging/app.service", "/usr/lib/systemd/system/app.service")

for fmt in ["deb", "rpm", "tar"]:
    contents = [unit] if fmt in ["deb", "rpm"] else []
    target(pkg, fmt, contents = contents)
```

Link a command name:

```python
contents = [symlink("/usr/lib/app/bin/app", "/usr/bin/app")]
```

## Without a container

A target without a `job` lists every file itself. Build the program yourself and stage it with `DESTDIR`, then point
`src` at the stage:

```python
pkg = package(name = "tool", version = "1.0.0", description = "A tool")
stage = arg("STAGE", "/tmp/stage")

target(pkg, "tar", contents = [file(stage + "/usr/bin/tool", "/usr/bin/tool", mode = "0755")])
```

```console
$ make install DESTDIR=/tmp/stage PREFIX=/usr
$ pkgsmith build . --build-arg STAGE=/tmp/stage
```
