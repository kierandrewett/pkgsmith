# Contents

The files of a package come from three places, in this order.

1. **`$pkgdir`.** Each file that the build commands install into `$pkgdir` is in the package. See
   [building](building.md#the-folders).
2. **`expose`.** Each entry offers a file that the build made, or a file from the source tree. Every `package` call
   after it carries the entry.
3. **`contents`.** Each entry of one `package` call adds a file for that package only.

An entry that names the same `dst` as an earlier one replaces it. This is how you mark an installed file as
configuration, or change its mode, without listing every other file.

## Entries

Each helper returns one entry. Pass the entries to `expose(...)`, or put them in the `contents` list of one `package` call.

| Function                                        | Entry                                                       |
| ----------------------------------------------- | ----------------------------------------------------------- |
| `file(src, dst, mode, owner, group)`            | A file, or the files that a glob matches                    |
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
if host()["format"] in ["deb", "rpm"]:
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

## Parent folders

A package must not own `/usr` or `/usr/bin`. A folder that no entry names is not part of the `rpm`.

The formats that need folder entries to unpack files (`deb`, `ipk`, `apk`, `archlinux`) add the parents of every file
as folders owned by root with mode `0755`. The package manager does not change the mode of a folder that already
exists.

## Installed files and excludes

The files and folders in `$pkgdir` are all packaged, with the mode they have there. Nothing outside `$pkgdir` is
packaged. The build runs as you, so install into `$pkgdir` and not into the real `/usr`.

Leave paths out with the `exclude` list of `package`. An entry is a path prefix or a glob, relative to the root of
`$pkgdir`:

```python
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')
package("deb", exclude = ["/usr/share/doc/**", "/usr/lib/debug", "/usr/share/man"], **common)
```

## Examples

Mark one installed file as configuration, keep everything else:

```python
expose(config(None, "/etc/app.conf"))
```

An entry with `src=None` keeps the content of the installed file and changes its flags. It is an error when the build
did not install `dst`.

Add a file from the source tree for Debian and Fedora only:

```python
common = {"name": "app", "version": "1.0.0", "description": "An app"}
unit = file("packaging/app.service", "/usr/lib/systemd/system/app.service")

package("deb", contents = [unit], **common)
package("rpm", contents = [unit], **common)
package("tar", **common)
```

Link a command name:

```python
expose(symlink("/usr/lib/app/bin/app", "/usr/bin/app"))
```

## Without a build

A package with no `sh` before it lists every file itself. Build the program yourself and stage it with `DESTDIR`, then
point `src` at the stage:

```python
STAGE = arg("STAGE", "/tmp/stage")

package(
    "tar",
    name = "tool",
    version = "1.0.0",
    description = "A tool",
    contents = [file(STAGE + "/usr/bin/tool", "/usr/bin/tool", mode = "0755")],
)
```

```console
$ make install DESTDIR=/tmp/stage PREFIX=/usr
$ pkgsmith build . --build-arg STAGE=/tmp/stage
```
