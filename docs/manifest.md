# Script reference

A `pkgsmith.star` is a program that describes packages. pkgsmith runs it from top to bottom, and every `package(...)`
call that the run reaches writes one package. The script reads the files of a [prefix](prefix.md). It builds nothing and
runs no command.

This page lists every function, helper and field with its accepted values, its default and an example. To learn how to
put them together, read [scripting](scripting.md).

The script is Starlark, with f-strings and `json`. It runs with no network, no processes and no way to change a file.
It reads only the prefix and its own folder. A name or field that does not exist is an error that names it.

## A complete example

```python
common = {
    "name": "hello",
    "version": arg("VERSION", "1.0.0"),
    "description": """A small program
Prints a greeting.""",
    "maintainer": "Your Name <you@example.com>",
    "homepage": "https://example.org/hello",
    "license": "MIT",
}

for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(fmt, config_files = ["/etc/**"], **common)
```

Make the prefix first (`make install DESTDIR=prefix`), then run `pkgsmith plan`. This writes six packages from one
prefix. See [getting started](getting-started.md).

## Functions

| Function                  | Returns                                           | Page section              |
| ------------------------- | ------------------------------------------------- | ------------------------- |
| `arg(name, default=None)` | A build argument                                  | [arg](#arg)               |
| `host()`                  | A dict that describes the system                  | [host](#host)             |
| `prefix()`, `here()`      | The prefix path and the script folder path        | [prefix and here](#prefix-and-here) |
| `read_file`, `exists`, `is_dir`, `listdir`, `glob` | Read-only access to the prefix | [reading files](#reading-files) |
| `load(file, *names)`      | Names from another script                         | [load](#load)             |
| `include(*globs)`         | Nothing. Keeps only matching files in the packages that follow | [include, exclude and reset](#include-exclude-and-reset) |
| `exclude(*globs)`         | Nothing. Leaves matching files out of the packages that follow | [include, exclude and reset](#include-exclude-and-reset) |
| `reset()`                 | Nothing. Clears exposed files, include and exclude globs | [include, exclude and reset](#include-exclude-and-reset) |
| `expose(*entries)`        | Nothing. Offers only the listed files             | [expose](#expose)         |
| `package(format, **fields)` | Nothing. Writes one package                     | [package](#package)       |
| `file`, `config`, `tree`, `symlink`, `directory`, `ghost` | One content entry | [content helpers](#content-helpers) |

## arg

`arg(name, default=None)` reads a build argument.

| Argument  | Type | Default  | Meaning                                                                      |
| --------- | ---- | -------- | ---------------------------------------------------------------------------- |
| `name`    | text | required | The name of the argument                                                     |
| `default` | any  | `None`   | The result when there is neither a `--build-arg` nor an environment variable |

The result is, in order: the value of `--build-arg NAME=VALUE`, the environment variable `NAME`, then `default`. A value
from the command line or the environment is always text.

```python
VERSION = arg("VERSION", "0.4.2")
```

## host

`host()` describes the system that runs pkgsmith. It returns a dict. Use it to adapt a script, for example to the
architecture or to a dependency name. Do not use it to choose which formats to write: every format is written on every
system.

| Key       | Type | Values                                                                                           |
| --------- | ---- | ------------------------------------------------------------------------------------------------ |
| `os`      | text | `linux`, `macos` or `windows`                                                                    |
| `arch`    | text | `amd64`, `arm64`, or the name the system gives for another processor                             |
| `distro`  | text | The `ID` of `/etc/os-release`, such as `fedora`, `debian`, `arch` or `alpine`. `macos` or `windows` there |
| `like`    | list | The `ID_LIKE` of `/etc/os-release`, one text for each word. Empty when there is none             |
| `version` | text | The `VERSION_ID` of `/etc/os-release`. Empty when there is none                                  |
| `libc`    | text | `glibc` or `musl` on Linux. Empty text elsewhere                                                 |
| `format`  | text | The package format this system installs, as `pkgsmith formats` names it. Empty when unknown      |

```python
arch = "arm64" if host()["arch"] == "arm64" else "amd64"
```

## prefix and here

| Function   | Returns                                                                       |
| ---------- | ----------------------------------------------------------------------------- |
| `prefix()` | The path of the prefix, as text. It is the `--prefix` folder, made absolute   |
| `here()`   | The path of the folder that holds the running script, as text                 |

Use `here()` to name a file next to the script, for example `here() + "/LICENSE"`.

## Reading files

A script can read, and only read, two places: the prefix and the folder of its script. Any other path is an error, and
a symbolic link that leads out of both is an error too.

| Function            | Returns                                                         | If the path is missing or outside |
| ------------------- | --------------------------------------------------------------- | --------------------------------- |
| `read_file(path)`   | The text of a file                                              | An error                          |
| `exists(path)`      | `True` when the path exists                                     | `False`                           |
| `is_dir(path)`      | `True` when the path is a folder                                | `False`                           |
| `listdir(path)`     | The names in a folder, sorted                                   | An error                          |
| `glob(pattern)`     | The paths that match, sorted, such as `/usr/bin/a`              | An empty list                     |

A relative path and a path that starts with `/` both mean the prefix: `usr/bin/hello` and `/usr/bin/hello` are the same
file. To read a file next to the script, give an absolute path that starts with `here()`. `glob` matches inside the
prefix only, and its results are paths from the root of the prefix.

```python
version = read_file("/usr/share/hello/VERSION").strip()
licence = read_file(here() + "/LICENSE")

units = glob("/usr/lib/systemd/system/*.service")
has_docs = exists("/usr/share/doc/hello")
tools = [name for name in listdir("/usr/bin") if name.startswith("hello-")]
```

## load

`load("file.star", "name", ...)` brings names from another `.star` file. The path is relative to the file that holds the
`load`. A loaded file can call every function on this page. Every call that it makes counts, including the ones that
choose files.

```python
load("common.star", "base")
```

See [scripting](scripting.md#sharing-code-with-load).

## include, exclude and reset

`include(*globs)` and `exclude(*globs)` narrow the packages that the script declares after the call. `reset()` clears
what they collected.

| Function           | Effect                                                                               |
| ------------------ | ------------------------------------------------------------------------------------ |
| `include(*globs)`  | Keeps only the files of the package that match. Without any `include`, everything stays |
| `exclude(*globs)`  | Leaves out the files of the package that match                                       |
| `reset()`          | Forgets the exposed files and the collected `include` and `exclude` globs            |

Each argument is text, or a list of text. A glob is a path of the package, as it reads on the installed system:
`/usr/bin/**`, `/etc/hello.conf`, `/usr/share/doc/**`. A plain path such as `/usr/lib/debug` matches itself and
everything below it.

- Calls add up. After `exclude("/a/**")` and `exclude("/b/**")`, both are left out, until `reset()`.
- Each rule applies to the packages declared after the call, and not to earlier ones.
- `include` keeps a folder when a file below it stays. A path must match `include` and must not match `exclude`.
- `reset()` also clears the files that `expose` offered. The next package starts from the whole prefix again, so use it
  between split packages.

```python
common = {"version": "1.0.0", "description": "A small program"}

exclude("/usr/share/doc/**")
package("deb", name = "hello", **common)

reset()
include("/usr/share/doc/**")
package("tar", name = "hello-docs", **common)
```

The deb holds the prefix without the docs. The tar holds only the docs. See
[scripting](scripting.md#leave-files-out-or-keep-only-some).

## expose

`expose(*entries)` offers only the files that you list, for a layout that `include` and `exclude` do not fit. Every
`package` call after it carries them, and they add up until `reset()`. Once something is exposed, a package holds the
exposed files and not the whole prefix. Each argument is one entry from a [content helper](#content-helpers), or a list
of entries.

```python
expose(
    tree("/etc", "/etc"),
    config("etc/app.conf", "/etc/app.conf"),
)
```

The first entry offers the folder `/etc`. The second replaces the one file `/etc/app.conf` with the same file marked as
configuration. An entry with the same `dst` as an earlier one replaces it.

## package

`package(format, **fields)` writes one package. It holds the files exposed so far, or the whole prefix when nothing is
exposed, plus its own `contents`. The collected `include` and `exclude` rules and the fields of the call then cut the
list down. See [contents](contents.md).

`format` is a name from `pkgsmith formats`, for example `rpm`, `deb`, `archlinux`, `apk`, `tar` or `zip`. A format that
no plugin writes is an error that lists the formats installed. These are the fields.

### Identity

| Field         | Type   | Default         | Meaning                                                                      |
| ------------- | ------ | --------------- | ---------------------------------------------------------------------------- |
| `name`        | text   | required        | Lowercase letters, digits and `+ - . _`. Starts with a letter or digit       |
| `version`     | text   | required        | `MAJOR.MINOR.PATCH` or `MAJOR.MINOR.PATCH-prerelease`. See [versions](versions.md) |
| `description` | text   | required        | The first line is the summary. The rest is the long text                     |
| `release`     | text   | `"1"`           | Package revision of one version. A number is accepted                        |
| `epoch`       | number | `0`             | Sorts above every lower epoch. `0` leaves it out. See [versions](versions.md#epoch) |
| `arch`        | text   | the host `arch` | `amd64`, `arm64` or `all`. See [arch](#arch)                                 |
| `maintainer`  | text   | none            | `Name <email>`                                                               |
| `homepage`    | text   | none            | A URL                                                                        |
| `license`     | text   | none            | A licence expression such as `GPL-3.0-or-later`                              |

### Relations

Each field is a list of text. See [dependencies](dependencies.md) for the syntax of each format.

| Field        | Default | Meaning                                         |
| ------------ | ------- | ----------------------------------------------- |
| `depends`    | `[]`    | Packages that must be installed                 |
| `recommends` | `[]`    | Packages installed by default, but not required |
| `suggests`   | `[]`    | Packages that are useful together               |
| `conflicts`  | `[]`    | Packages that cannot be installed together      |
| `replaces`   | `[]`    | Packages this one takes over from               |
| `provides`   | `[]`    | Names this package also answers to              |

### Files

| Field          | Type | Default | Meaning                                                                         |
| -------------- | ---- | ------- | ------------------------------------------------------------------------------- |
| `contents`     | list | `[]`    | Entries added to the exposed files, or to the whole prefix when nothing is exposed |
| `exclude`      | list | `[]`    | Path globs of the package to leave out, such as `"/usr/share/doc/**"`           |
| `include`      | list | `[]`    | Path globs of the package to keep. Everything else goes. Empty keeps everything |
| `config_files` | list | `[]`    | Path globs of files that are configuration files, kept on upgrade               |
| `own_dirs`     | list | unset   | Path globs of the folders the package owns. `[]` owns none. Unset owns all but system folders |

The paths in these lists are paths in the package, as they read on the installed system. A glob such as `/etc/**`
matches a whole folder. A plain path such as `/usr/lib/debug` matches that path and everything below it.

The fields `include` and `exclude` add to the rules that earlier `include(...)` and `exclude(...)` calls collected. They
do not replace them. All three fields apply to this package only. See [contents](contents.md).

### Scripts

`scripts` is a dict. Every key is optional.

| Key            | Runs                           |
| -------------- | ------------------------------ |
| `pre_install`  | Before the files are installed |
| `post_install` | After the files are installed  |
| `pre_remove`   | Before the files are removed   |
| `post_remove`  | After the files are removed    |

A value is text. Text with a line break is the script. Text on one line is the name of a file next to the script, which
pkgsmith reads. See [scripts](scripts.md).

### Output and selection

| Field     | Type | Default         | Meaning                                                                    |
| --------- | ---- | --------------- | -------------------------------------------------------------------------- |
| `options` | dict | `{}`            | The options of this format. See [options](#options)                        |
| `distro`  | text | none            | Names a distro for plugins that adapt, and for `--where distro=NAME`       |
| `file`    | text | none            | The name of the output file, as a template. See [file](#file)              |
| `tags`    | dict | `{}`            | Text labels for `--where`. Keys and values are text                        |

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package(
    "deb",
    depends = ["libc6"],
    exclude = ["/usr/share/doc/**"],
    tags = {"channel": "stable"},
    **common
)
```

### arch

| Value   | Meaning                                                             |
| ------- | ------------------------------------------------------------------- |
| `amd64` | x86-64                                                              |
| `arm64` | 64-bit Arm                                                          |
| `all`   | A package without compiled code, for every architecture             |

`arch` says what the files of the prefix are. pkgsmith does not look inside them to check. If the prefix holds arm64
files, write `arch = "arm64"`, whatever system runs pkgsmith. See [prefix](prefix.md#other-architectures-and-c-libraries).

### options

`options` is a dict for one format. The plugin that writes the format defines the keys, and a key it does not know is an
error. Give each format its own dict:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package("deb", options = {"section": "utils"}, **common)
package("tar", options = {"compression": "zstd", "top_folder": True}, **common)
```

See [formats](formats.md) for the options of every format.

### file

Every format picks a default file name, such as `hello-1.0.0-amd64.tar.xz`. `pkgsmith plan` shows it. A `file`
template replaces it.

| Placeholder                        | Value                                    |
| ---------------------------------- | ---------------------------------------- |
| `{file}`                           | The name the format picked               |
| `{name}`, `{version}`, `{release}`, `{epoch}` | The package fields                    |
| `{arch}`                           | `amd64`, `arm64` or `all`                |
| `{format}`, `{distro}`             | The format and the distro of the package |

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package("tar", file = "{name}_{version}.tar.bz2", options = {"compression": "bzip2"}, **common)
```

The result must be a plain file name: not empty, not starting with a dot, and without `/`. The template names the whole
file, so write the extension yourself, or keep it with `{file}`. Two packages that would write the same file stop the
build before it starts, so give one of them a `file` template.

### tags

`--where KEY=VALUE` selects packages by `format`, `distro`, `arch`, `name` and by the keys of `tags`:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package("deb", tags = {"channel": "nightly"}, **common)
```

```console
$ pkgsmith build --where channel=nightly
```

## Content helpers

Each helper returns one entry. Pass entries to `expose(...)` or to the `contents` field of `package`. See
[contents](contents.md) for what each type does.

| Function                             | Entry                                                            |
| ------------------------------------ | ---------------------------------------------------------------- |
| `file(src, dst, mode, owner, group)` | A file, or the files that a glob matches                         |
| `config(src, dst, mode, owner, group)` | A configuration file, kept on upgrade                          |
| `tree(src, dst, mode, owner, group)` | A folder copied recursively, including the folder itself         |
| `symlink(link_target, dst)`          | A symbolic link at `dst` that points to `link_target`            |
| `directory(dst, mode, owner, group)` | An empty folder that the package owns                            |
| `ghost(dst)`                         | A path the package owns but does not carry. Only `rpm` records it |

| Argument | Type | Default                | Meaning                                                                     |
| -------- | ---- | ---------------------- | --------------------------------------------------------------------------- |
| `src`    | text | required               | Where the content comes from. See below                                     |
| `dst`    | text | required               | An absolute path on the installed system. A path that ends in `/` is a folder for `file` |
| `mode`   | text | the mode of the source | Permissions as octal text, such as `"0755"`. A tree keeps the mode of every file |
| `owner`  | text | `root`                 | User name                                                                   |
| `group`  | text | `root`                 | Group name                                                                  |

A relative `src` is a path in the prefix. An absolute `src` is a path in the prefix, or in the folder of the script
when it starts with `here()`. Anything else is an error, so a script cannot pull in files of the system by accident.

For `symlink`, the first argument is the link target as it reads on the installed system, not a file to read.

```python
common = {"name": "app", "version": "1.0.0", "description": "An app"}

package("tar", contents = [
    file(here() + "/packaging/app.desktop", "/usr/share/applications/app.desktop", mode = "0644"),
    symlink("/usr/lib/app/bin/app", "/usr/bin/app"),
], **common)
```

## Command line

`pkgsmith build`, `plan` and `files` run the script. `--prefix`, `--build-arg`, `--where` and `-F` choose its input
and which packages to write. See [command line](cli.md).
