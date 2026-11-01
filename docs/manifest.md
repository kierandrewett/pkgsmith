# Script reference

A `pkgsmith.star` is a program that pkgsmith runs top to bottom on this system. This page lists every function it can
call, with its arguments, defaults and accepted values. To learn how to write a script, read [scripting](scripting.md).

The default file name is `pkgsmith.star`. `-f FILE` names another. The script is Starlark, with f-strings. It cannot
read files, use the network or start a process. Relative `src` paths in content entries and script file names resolve
against the folder of the script.

Everything is a function call. There are no special global variables. A call with a name or a field that does not exist
is an error that names it, so a typo is not a silent no-op.

## A complete example

```python
common = {
    "name": "bingux-config",
    "version": arg("VERSION", "0.1.0"),
    "description": """Configuration command for the Bingux desktop
Reads, changes and checks the Bingux configuration file.
""",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "homepage": "https://github.com/kierandrewett/bingux",
    "license": "GPL-3.0-or-later",
}

sh("cargo build --release --manifest-path crates/config/Cargo.toml", name = "Build")
expose(
    file("target/release/bingux-config", "/usr/bin/bingux-config", mode = "0755"),
    config("packaging/bingux.conf", "/etc/bingux/bingux.conf"),
)

h = host()
if h["format"] == "rpm":
    package("rpm", depends = ["glibc"], **common)
elif h["format"] == "deb":
    package("deb", depends = ["libc6"], **common)
package("tar", **common)
```

Which packages this writes depends on the system. On Fedora it writes an rpm and a tar. On Debian it writes a deb and a
tar. A branch that does not run writes nothing.

## How a script runs

- pkgsmith runs the script once, top to bottom, on this system.
- `sh(...)` adds a command to the build. Nothing runs yet.
- `expose(...)` adds files to what the script offers.
- `package(...)` writes one package from the commands and the files added so far.
- pkgsmith then runs the builds the packages need and writes the packages. Packages that follow the same commands share
  one build.

A script that writes no package is an error.

## arg

`arg(name, default=None)` reads a build argument.

| Argument  | Type | Default  | Meaning                                                                       |
| --------- | ---- | -------- | ----------------------------------------------------------------------------- |
| `name`    | text | required | The name of the argument                                                      |
| `default` | any  | `None`   | The result when there is neither a `--build-arg` nor an environment variable  |

The result is, in order: the value of `--build-arg NAME=VALUE`, the environment variable `NAME`, then `default`. A value
that comes from the command line or the environment is always text.

```python
VERSION = arg("VERSION", "0.4.2")
```

## host

`host()` describes the system that runs the script. It returns a dict.

| Key       | Type | Values                                                                                          |
| --------- | ---- | ----------------------------------------------------------------------------------------------- |
| `os`      | text | `linux`, `macos` or `windows`                                                                   |
| `arch`    | text | `amd64` or `arm64`                                                                              |
| `distro`  | text | The `ID` of `/etc/os-release`, such as `fedora`, `debian`, `ubuntu`, `arch` or `alpine`. `macos` or `windows` on those systems |
| `like`    | list | The `ID_LIKE` of `/etc/os-release`, one text for each word. Empty when there is none            |
| `version` | text | The `VERSION_ID` of `/etc/os-release`. Empty when there is none                                 |
| `libc`    | text | `glibc`, `musl`, or `""` when pkgsmith cannot tell                                              |
| `format`  | text | The native package format: `deb`, `rpm`, `archlinux` or `apk`. `dmg` on macOS, `msi` on Windows. `""` when unknown |

```python
h = host()
if h["libc"] == "musl":
    sh("make LDFLAGS=-static-pie")
```

## sh

`sh(run, name=None, env={})` adds a command to the build.

| Argument | Type | Default  | Meaning                                                                          |
| -------- | ---- | -------- | -------------------------------------------------------------------------------- |
| `run`    | text | required | The script. It runs in a fresh `sh -e`, in the folder of the script (`$srcdir`)  |
| `name`   | text | `None`   | Shown in the progress output                                                     |
| `env`    | dict | `{}`     | Variables for this command. Names use letters, digits and `_`. The values are literal text |

Commands run in the order of the calls. A package built after a `sh` call runs that command first. See
[building](building.md) for `$srcdir`, `$pkgdir` and how a command passes values to the next one.

```python
sh("cargo build --release --locked", name = "Build", env = {"RUSTFLAGS": "-C target-cpu=native"})
sh('install -Dm755 target/release/app "$pkgdir/usr/bin/app"')
```

## expose

`expose(*entries)` makes files that the build produced part of what the script offers. Every `package` call after it
carries them. Each argument is one entry from a [content helper](#content-helpers), or a list of entries.

```python
expose(
    file("target/release/app", "/usr/bin/app", mode = "0755"),
    tree("share", "/usr/share/app"),
)
```

The `src` of an entry is relative to the folder of the script. It is read after the build ran, so it can name a file
that the build made. Files that a command installs into `$pkgdir` are packaged without `expose`.

## package

`package(format, **fields)` packs the commands run so far and the files exposed so far into one package.

`format` is a name from `pkgsmith formats`, for example `rpm`, `deb`, `archlinux`, `apk`, `tar`. These are the fields.

| Field         | Type   | Default      | Meaning                                                                                  |
| ------------- | ------ | ------------ | ---------------------------------------------------------------------------------------- |
| `name`        | text   | required     | Package name. Lowercase letters, digits and `+ - . _`. Starts with a letter or digit     |
| `version`     | text   | required     | `MAJOR.MINOR.PATCH` or `MAJOR.MINOR.PATCH-prerelease`. See [versions](versions.md)       |
| `description` | text   | required     | The first line is the summary. The rest is the long text                                 |
| `release`     | text   | `"1"`        | Package revision for one version. Reset it to `"1"` when the version changes. A number is accepted |
| `epoch`       | number | `0`          | Makes the version newer than every version with a lower epoch. `0` leaves it out. See [versions](versions.md#epoch) |
| `arch`        | text   | the host arch | `amd64`, `arm64` or `all`. See [arch](#arch)                                            |
| `maintainer`  | text   | `None`       | `Name <email>`                                                                           |
| `homepage`    | text   | `None`       | URL                                                                                      |
| `license`     | text   | `None`       | License expression, for example `GPL-3.0-or-later`                                       |
| `depends`     | list   | `[]`         | See [dependencies](dependencies.md)                                                      |
| `recommends`  | list   | `[]`         | See [dependencies](dependencies.md)                                                      |
| `suggests`    | list   | `[]`         | See [dependencies](dependencies.md)                                                      |
| `conflicts`   | list   | `[]`         | See [dependencies](dependencies.md)                                                      |
| `replaces`    | list   | `[]`         | See [dependencies](dependencies.md)                                                      |
| `provides`    | list   | `[]`         | See [dependencies](dependencies.md)                                                      |
| `contents`    | list   | `[]`         | Entries to add to the exposed ones. See [contents](contents.md)                          |
| `options`     | dict   | `{}`         | Options of the format. See [options](#options)                                           |
| `distro`      | text   | the host distro | Names the lane and what a plugin adapts to. `--where distro=NAME` selects it          |
| `file`        | text   | `None`       | The name of the output file. A template. See [naming the output file](#naming-the-output-file) |
| `tags`        | dict   | `{}`         | Text labels for `--where`. Keys and values are text                                      |
| `exclude`     | list   | `[]`         | Paths (globs) in `$pkgdir` to leave out, such as `/usr/share/doc/**`. See [building](building.md#leaving-files-out) |
| `scripts`     | dict   | `{}`         | Install scripts. See [scripts](#scripts)                                                 |

```python
package(
    "deb",
    name = "hello",
    version = "1.0.0",
    description = "A small program",
    depends = ["libc6"],
    tags = {"channel": "stable"},
)
```

### arch

| Value   | Meaning                                                                                        |
| ------- | ---------------------------------------------------------------------------------------------- |
| `amd64` | x86-64                                                                                         |
| `arm64` | 64-bit Arm                                                                                     |
| `all`   | A package without compiled code, for every architecture                                        |

A build runs on this system, so it can make the host architecture or `all`. A package with commands before it and any
other `arch` is an error. To make an arm64 package, run pkgsmith on an arm64 system.

### Dependencies and relations

Each field is a list of package names, written in the syntax of the format.

| Field        | Meaning                                         |
| ------------ | ----------------------------------------------- |
| `depends`    | Packages that must be installed                 |
| `recommends` | Packages installed by default, but not required |
| `suggests`   | Packages that are useful together               |
| `conflicts`  | Packages that cannot be installed together      |
| `replaces`   | Packages this one takes over from               |
| `provides`   | Names this package also answers to              |

### Scripts

`scripts` is a dict. Each key is optional.

| Key            | Runs                                |
| -------------- | ----------------------------------- |
| `pre_install`  | Before the files are installed      |
| `post_install` | After the files are installed       |
| `pre_remove`   | Before the files are removed        |
| `post_remove`  | After the files are removed         |

A value is one of these:

- Text with a line break: the script.
- Text on one line: the name of a file next to the script.
- A function. pkgsmith calls it and records the `sh()` calls in it as the script. Those commands do not join the build.
  They run on the system that installs the package.

```python
def post_install():
    sh("systemctl daemon-reload || true")

package(
    "rpm",
    name = "app",
    version = "1.0.0",
    description = "An app",
    scripts = {
        "pre_install": "getent passwd app >/dev/null || useradd --system app\n",
        "post_install": post_install,
        "post_remove": "packaging/post-remove.sh",
    },
)
```

See [scripts](scripts.md).

### Options

`options` is a dict of options for one format. The plugin that writes the format defines the keys, and a key it does not
know is an error. Give each format its own:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package("deb", options = {"section": "utils"}, **common)
package("tar", options = {"compression": "zstd", "top_folder": True}, **common)
```

See [formats](formats.md) for the options of every format.

### Naming the output file

Every format picks a default file name, such as `hello-1.0.0-amd64.tar.xz`. `pkgsmith plan` shows it. A `file`
template replaces it.

| Placeholder  | Value                                                            |
| ------------ | ---------------------------------------------------------------- |
| `{file}`     | The name the format picked                                       |
| `{name}`, `{version}`, `{release}` | The package fields                         |
| `{arch}`     | `amd64`, `arm64` or `all`                                        |
| `{format}`, `{distro}` | The format and the distro of the package               |

```python
package("tar", file = "{name}_{version}.tar.bz2", options = {"compression": "bzip2"}, **common)
```

The result must be a plain file name: not empty, not starting with a dot, and without `/`. The template names the whole
file, so write the extension yourself, or keep it with `{file}`. To write a literal brace in an f-string, double it.

Two packages that would write the same file stop the build before it starts. Give one of them a `file` template.

Each package builds in a private folder and moves its file to the final name, so the folder `-o` receives only finished
packages.

### Selecting with tags

`--where KEY=VALUE` selects packages by `format`, `distro`, `arch`, `name` and by the keys of `tags`. Use `tags` to name
a group of packages:

```python
package("deb", tags = {"channel": "nightly"}, **common)
```

```console
$ pkgsmith build --where channel=nightly
```

## Content helpers

Each helper returns one entry. Pass entries to `expose(...)` or to the `contents` field of `package`. See
[contents](contents.md) for what each type does.

| Function                                    | Entry                                                                  |
| ------------------------------------------- | ---------------------------------------------------------------------- |
| `file(src, dst, mode, owner, group)`        | A file, or the files that a glob matches                               |
| `config(src, dst, mode, owner, group)`      | A configuration file, kept on upgrade                                  |
| `tree(src, dst, mode, owner, group)`        | A folder copied recursively, including the folder itself               |
| `symlink(link_target, dst)`                 | A symbolic link at `dst` that points to `link_target`                  |
| `directory(dst, mode, owner, group)`        | An empty folder that the package owns                                  |
| `ghost(dst)`                                | A path the package owns but does not carry. Only `rpm` records it      |

| Argument | Type | Default                | Meaning                                                                          |
| -------- | ---- | ---------------------- | -------------------------------------------------------------------------------- |
| `src`    | text | required               | Where the content comes from, relative to the folder of the script. For `config`, `None` marks the file that the build installed at `dst` |
| `dst`    | text | required               | An absolute path on the installed system. A path that ends in `/` is a folder for `file` |
| `mode`   | text | the mode of the source | Permissions as octal text, for example `"0755"`                                  |
| `owner`  | text | `root`                 | User name                                                                        |
| `group`  | text | `root`                 | Group name                                                                       |

```python
expose(
    file("packaging/app.desktop", "/usr/share/applications/app.desktop", mode = "0644"),
    config("packaging/app.conf", "/etc/app.conf"),
    symlink("/usr/lib/app/bin/app", "/usr/bin/app"),
)
```

## load

`load("file.star", "name", ...)` brings names from another `.star` file. The path is relative to the file that holds the
`load`. See [scripting](scripting.md#loading-other-files).

## Command line

`pkgsmith build`, `plan` and `files` run the script. `--build-arg`, `--where` and `-F` choose among its packages. See
[command line](cli.md).
