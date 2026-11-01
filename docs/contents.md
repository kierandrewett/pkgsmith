# Contents

The files of a package come from two places, in this order.

1. **`expose`.** Each entry offers files of the [prefix](prefix.md). Every `package` call after it carries the entry.
2. **`contents`.** Each entry of one `package` call adds files for that package only.

An entry that names the same `dst` as an earlier one replaces it. Then the `exclude` and `include` lists of the package
cut the result down, and `config_files` marks configuration files.

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

expose(tree("/", "/"))
package("deb", exclude = ["/usr/share/doc/**"], config_files = ["/etc/**"], **common)
```

## Entries

Each helper returns one entry. Pass the entries to `expose(...)`, or put them in the `contents` list of one `package`
call.

| Function                             | Entry                                            |
| ------------------------------------ | ------------------------------------------------ |
| `file(src, dst, mode, owner, group)` | A file, or the files that a glob matches         |
| `config(src, dst, mode, owner, group)` | A configuration file                           |
| `tree(src, dst, mode, owner, group)` | A folder, copied recursively                     |
| `symlink(link_target, dst)`          | A symbolic link                                  |
| `directory(dst, mode, owner, group)` | An empty folder that the package owns            |
| `ghost(dst)`                         | A path the package owns but does not carry       |

| Argument | Meaning                                                                                              |
| -------- | ---------------------------------------------------------------------------------------------------- |
| `dst`    | Absolute path on the installed system. Required                                                      |
| `src`    | A path in the prefix. For `symlink`, the link target as it reads after install                       |
| `mode`   | Permissions as octal text, such as `"0755"`. Default: the mode of the source file                    |
| `owner`  | User name. Default `root`                                                                            |
| `group`  | Group name. Default `root`                                                                           |

A relative `src` is a path in the prefix, and so is `/usr/bin/hello`. A file next to the script is named with `here()`:
`here() + "/LICENSE"`. Anything else is an error.

## Types

| Entry       | `src` is                                    | Result                                                                           |
| ----------- | ------------------------------------------- | -------------------------------------------------------------------------------- |
| `file`      | A file, or a glob such as `usr/share/man/man1/*.1` | One file at `dst`. For a glob, or when `dst` ends with `/`, `dst` is a folder |
| `config`    | A file                                      | A file the package manager keeps when the user changed it                        |
| `tree`      | A folder                                    | The folder, copied recursively, with the folders and links below it              |
| `directory` | none                                        | An empty folder that the package owns                                            |
| `symlink`   | The link target as it reads after install   | A symbolic link at `dst`                                                         |
| `ghost`     | none                                        | A path the package owns but does not carry. Only `rpm` records it                |

- A glob that matches no file is an error.
- A `tree` keeps the mode of every file and folder as it is in the prefix. `owner` and `group` apply to all of them.
- A `tree` keeps symbolic links as links. Make them relative in the prefix.

## Configuration files

Mark a configuration file in one of two ways. Both give the same result in the package.

- `config(src, dst)` adds a file as a configuration file.
- `config_files` on `package` marks paths that other entries already added. It takes path globs of the package.

```python
common = {"name": "app", "version": "1.0.0", "description": "An app"}

expose(tree("/", "/"))
package("rpm", config_files = ["/etc/app.conf", "/etc/app.d/**"], **common)
```

Use `config_files` when a `tree` brought the files in. How a format records a configuration file:

| Format       | Record                                |
| ------------ | ------------------------------------- |
| `deb`, `ipk` | Listed in `conffiles`                 |
| `rpm`        | `%config(noreplace)` flag             |
| `archlinux`  | Listed in `backup`                    |
| `apk`        | No record. apk protects `/etc` itself |
| `tar`        | No record                             |

## Leaving files out and keeping files

`exclude` lists paths of the package to leave out. `include` lists the paths to keep, and everything else goes. Both take
path globs of the package: `/usr/share/doc/**`. A plain path, such as `/usr/lib/debug`, matches itself and everything
below it. With both set, a path must match `include` and must not match `exclude`.

```python
common = {"name": "app", "version": "1.0.0", "description": "An app"}

expose(tree("/", "/"))
package("deb", exclude = ["/usr/share/doc/**", "/usr/lib/debug", "/usr/share/man"], **common)
package("tar", include = ["/usr/bin/**"], **common)
```

`include` keeps the folders that lead to a kept file. The same prefix feeds both packages above.

## Parent folders

A package must not own `/usr` or `/usr/bin`. The `rpm` carries only the folders that some entry names.

The formats that need folder entries to unpack files (`deb`, `ipk`, `apk`, `archlinux`) add the parents of every file
as folders owned by root with mode `0755`. The package manager does not change the mode of a folder that already
exists.

## Examples

Add a file from the folder of the script, and a link, to the whole prefix:

```python
common = {"name": "app", "version": "1.0.0", "description": "An app"}

expose(
    tree("/", "/"),
    file(here() + "/LICENSE", "/usr/share/licenses/app/LICENSE"),
    symlink("/usr/lib/app/bin/app", "/usr/bin/app"),
)
package("tar", **common)
```

Pick files for one format only. Build the list in the script:

```python
common = {"name": "app", "version": "1.0.0", "description": "An app"}
unit = file("/usr/lib/systemd/system/app.service", "/usr/lib/systemd/system/app.service")

expose(tree("/usr/bin", "/usr/bin"))
package("deb", contents = [unit], **common)
package("rpm", contents = [unit], **common)
package("tar", **common)
```

An empty log folder and a ghost log file:

```python
common = {"name": "app", "version": "1.0.0", "description": "An app"}

expose(tree("/usr/bin", "/usr/bin"))
package(
    "rpm",
    contents = [directory("/var/log/app", mode = "0750"), ghost("/var/log/app/app.log")],
    **common
)
```
