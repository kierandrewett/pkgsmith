# Scripting

A `pkgsmith.star` is a program in Starlark, a small language with the syntax of Python. pkgsmith runs it from top to
bottom. Only the code that the run reaches counts: each `package(...)` call that runs writes one package, and a branch
that does not run writes none.

This page shows how to write a script. Every function and field is in the [script reference](manifest.md). The files
that a script packs come from a [prefix](prefix.md) that you made with your own build.

## A script that runs

```python
# pkgsmith.star
expose(tree("/", "/"))
package("tar", name = "hello", version = "1.0.0", description = "A small program")
```

- `expose` adds the whole prefix to what the script offers. `tree("/", "/")` means the prefix root, installed at `/`.
- `package` writes a tar package from what was exposed so far.

Run `pkgsmith plan` to see what the script writes. A script that makes no `package` call is an error.

## The language

- Starlark has `def`, `for`, `if`, lists, dicts, comprehensions and f-strings. It has no classes, no `while` and no
  `import`. `json.encode` and `json.decode` are available.
- A script cannot use the network or start a process. It reads only the prefix and its own folder, and it cannot change
  either.
- A call with a name that does not exist, or a field that a function does not accept, is an error that names it.
- The functions of pkgsmith need no `load`. There are no global variables with special meaning.

## expose and package

`expose` adds files to what the script offers. `package` writes a package from what was exposed so far. A package sees
the exposures that came before it and none that come after:

```python
common = {"version": "1.0.0", "description": "A small program"}

expose(tree("/usr/bin", "/usr/bin"))
package("tar", name = "hello", **common)

expose(tree("/usr/share/doc", "/usr/share/doc"))
package("zip", name = "hello-docs", **common)
```

The tar holds `/usr/bin`. The zip holds `/usr/bin` and the docs, because exposures add up. To give each package only its
own files, use [split packages](#split-packages).

## Write every format from one script

A format is a file that a plugin writes. It needs no system of its own, so one script writes all of them from the same
prefix. Put the formats in a list and loop:

```python
common = {
    "name": "hello",
    "version": arg("VERSION", "1.0.0"),
    "description": "A small program",
    "maintainer": "Your Name <you@example.com>",
    "license": "MIT",
}

expose(tree("/", "/"))

for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(fmt, **common)
```

Run `pkgsmith formats` for the names. To write only some of them for one run, use `-F rpm,deb` on the command line,
and leave the script as it is.

## Shared fields

Put the fields that every package shares in a dict, then pass it with `**`:

```python
common = {
    "name": "hello",
    "version": arg("VERSION", "1.0.0"),
    "description": "A small program",
}

expose(tree("/", "/"))
package("deb", depends = ["libc6"], **common)
package("rpm", depends = ["glibc"], **common)
package("tar", **common)
```

A field that you pass twice is an error, so keep per-package fields out of `common`.

## Fields that differ by format

Some fields differ by format, mostly dependency names. Keep the differences in a dict and look them up:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

libc = {"rpm": "glibc", "deb": "libc6", "archlinux": "glibc", "apk": "musl"}

expose(tree("/", "/"))
for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(fmt, depends = [libc[fmt]] if fmt in libc else [], **common)
```

The `glibc` names fit a prefix linked against glibc. That prefix does not run on Alpine, so write the `apk` from a prefix
built on musl. See [the prefix](prefix.md#other-architectures-and-c-libraries).

## Build arguments

`arg(name, default=None)` returns the value of `--build-arg NAME=VALUE`. If there is none, it returns the environment
variable `NAME`. If there is none, it returns `default`.

```python
VERSION = arg("VERSION", "0.1.0")
```

```console
$ pkgsmith build --build-arg VERSION=0.2.0
$ VERSION=0.2.0 pkgsmith build
```

The value is always text, or `None` when there is no default and nothing is set. An empty value is false in a condition.

## The system you run on

`host()` returns a dict that describes the system that runs pkgsmith: `os`, `arch`, `distro`, `like`, `version`, `libc`
and `format`. The values are in the [script reference](manifest.md#host).

Use it for facts about the prefix that depend on where it was made. The usual one is the architecture:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A small program",
    "arch": host()["arch"],
}

expose(tree("/", "/"))
package("deb", **common)
```

`arch` already defaults to the host architecture, so write it only to name it, or to override it with `"arm64"` or
`"all"`. Do not use `host()` to decide which formats to write. A deb is a file, and any system can write one.

## Conditions

An `if` writes a package only when its condition holds. Use it for facts about your project:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

expose(tree("/", "/"))
package("tar", **common)

if exists("/usr/lib/systemd/system/hello.service"):
    package("deb", depends = ["systemd"], **common)

if arg("RELEASE") == "1":
    package("zip", **common)
```

A condition can read the prefix: `exists`, `is_dir`, `listdir`, `glob` and `read_file` are available. See
[reading the prefix](#reading-the-prefix).

## Reading the prefix

The prefix is a folder, so the script can look at it and let it decide. Read a version from a file that the build wrote:

```python
version = read_file("/usr/share/hello/VERSION").strip()

expose(tree("/", "/"))
package("tar", name = "hello", version = version, description = "A small program")
```

Find files by pattern, and list what a folder holds:

```python
programs = glob("/usr/bin/*")
plugins = listdir("/usr/lib/hello") if is_dir("/usr/lib/hello") else []

expose(tree("/", "/"))
package(
    "tar",
    name = "hello",
    version = "1.0.0",
    description = "A small program with " + str(len(programs)) + " programs",
)
```

`glob` gives paths such as `/usr/bin/hello`. `listdir` gives names such as `hello`, sorted. A path outside the prefix and
the folder of the script is an error. See [what a script may read](prefix.md#what-a-script-may-read).

## Split packages

One prefix can feed several packages. List the files of each package in its own `contents`, and expose nothing:

```python
common = {"version": "1.0.0", "description": "Part of the hello suite"}

package("tar", name = "hello", contents = [tree("/usr/bin", "/usr/bin")], **common)
package("tar", name = "hello-docs", contents = [tree("/usr/share/doc", "/usr/share/doc")], **common)
```

Or expose the whole prefix once, and cut each package down with `include` or `exclude`:

```python
common = {"version": "1.0.0", "description": "Part of the hello suite"}

expose(tree("/", "/"))
package("tar", name = "hello", exclude = ["/usr/share/doc/**"], **common)
package("tar", name = "hello-docs", include = ["/usr/share/doc/**"], **common)
```

`include` keeps only the paths that match, with the folders that lead to them. `exclude` removes paths. When both are
set, a path must match `include` and must not match `exclude`.

## Functions

Put what packages share in a function:

```python
def make_package(fmt, **fields):
    package(fmt, name = "tool", version = arg("VERSION", "1.0.0"), description = "A command line tool", **fields)

expose(tree("/", "/"))
make_package("tar")
make_package("zip", file = "{name}-{version}-source.zip")
```

## Text with f-strings

An f-string puts values in text. It works in every text field:

```python
VERSION = arg("VERSION", "1.0.0")

expose(tree("/", "/"))
package("tar", name = "hello", version = VERSION, description = f"Hello {VERSION}, a small program")
```

Write `{{` and `}}` for a literal brace in an f-string.

## Sharing code with load

`load("file.star", "name")` brings names from another `.star` file. The path is relative to the file that holds the
`load`. A loaded file can call the same functions, and every `expose` and `package` call that it makes counts.

```python
# common.star
def base(fmt, **fields):
    package(
        fmt,
        name = "hello",
        version = arg("VERSION", "1.0.0"),
        description = "A small program",
        license = "MIT",
        **fields
    )
```

```python
# pkgsmith.star
load("common.star", "base")

expose(tree("/", "/"))
base("tar")
base("zip")
```

Load only the names you use. A relative `src` is always a path in the prefix, wherever the call is written.

## Check the result

Run `pkgsmith plan` after every change. It runs the script, lists every package and its file name, and writes nothing.
Use `--where` and `-F` to look at part of it:

```console
$ pkgsmith plan --where format=deb
$ pkgsmith plan -F tar,zip
```

`pkgsmith files` goes one step further and lists the files of each package. An error in the script stops the command
with the file, the line and the message.
