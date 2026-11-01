# Scripting

A `pkgsmith.star` is a program in Starlark, a small language with the syntax of Python. pkgsmith runs it top to bottom on
this system, like `makepkg` runs a `PKGBUILD`. Only the code that the run reaches counts: each `package(...)` call that
runs writes one package, and a branch that does not run writes none.

This page shows how to write a script. Every function and field is in the [script reference](manifest.md).

## A script that runs

```python
# pkgsmith.star
sh("make")
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

package("tar", name = "hello", version = "1.0.0", description = "A small program")
```

- `sh` adds a command to the build. It does not run yet.
- `package` packs the commands so far and writes a tar package.

Run `pkgsmith plan` to see what the script writes. A script that calls `package` no time is an error.

## The language

- Starlark has `def`, `for`, `if`, lists, dicts, comprehensions and f-strings. It has no classes, no `while` and no
  `import`.
- A script cannot read files, use the network or start a process. It can compute, read build arguments, ask what system
  it runs on and load other `.star` files.
- A call with a name that does not exist, or a field that a function does not accept, is an error that names it.
- The functions of pkgsmith (`sh`, `expose`, `package`, `host`, `arg` and the content helpers) need no `load`.
- There are no global variables with special meaning. Everything is a function call.

## The order of calls

A package sees the commands and the files that came before it, and nothing after:

```python
sh("make")
expose(file("hello", "/usr/bin/hello", mode = "0755"))
package("tar", name = "hello", version = "1.0.0", description = "A small program")

sh("make docs")
expose(tree("doc", "/usr/share/doc/hello"))
package("zip", name = "hello-docs", version = "1.0.0", description = "Docs for hello")
```

The tar holds `hello`. The zip holds `hello` and the docs, because its build is longer. Packages with the same
commands, name, version, release and arch share one build run. See [building](building.md#one-build-many-packages).

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

`host()` returns a dict that describes this system. Use it to choose dependencies, commands and formats.

```python
h = host()
print(h["distro"], h["arch"], h["format"])
```

| Key       | Values                                                                      |
| --------- | --------------------------------------------------------------------------- |
| `os`      | `linux`, `macos`, `windows`                                                 |
| `arch`    | `amd64`, `arm64`                                                            |
| `distro`  | `fedora`, `debian`, `ubuntu`, `arch`, `alpine`, `macos`, `windows`, and so on |
| `like`    | The `ID_LIKE` words, as a list                                              |
| `version` | The `VERSION_ID`                                                            |
| `libc`    | `glibc`, `musl` or `""`                                                     |
| `format`  | `deb`, `rpm`, `archlinux`, `apk`, `dmg`, `msi` or `""`                      |

`format` is the native package format of this system. It is `""` when pkgsmith does not know it, so test it before you
use it as a format name.

## Shared fields

Put the fields that every package shares in a dict, then pass it with `**`:

```python
common = {
    "name": "hello",
    "version": arg("VERSION", "1.0.0"),
    "description": "A small program",
    "maintainer": "Your Name <you@example.com>",
    "license": "MIT",
}

sh("make && make install PREFIX=/usr DESTDIR=\"$pkgdir\"")

package("deb", depends = ["libc6"], **common)
package("tar", **common)
```

A field that you pass twice is an error, so keep per-package fields out of `common`.

## Conditions

Use `if` to write a package only on some systems. One package for the native format:

```python
h = host()
if h["format"] == "rpm":
    package("rpm", depends = ["glibc"], **common)
elif h["format"] == "deb":
    package("deb", depends = ["libc6"], **common)
elif h["format"] == "apk":
    package("apk", depends = ["musl"], **common)
```

A different value for one system, with a dict lookup:

```python
libc = {"rpm": "glibc", "deb": "libc6", "archlinux": "glibc", "apk": "musl"}.get(host()["format"], "glibc")
```

A different value in one expression:

```python
flags = "-static-pie" if host()["libc"] == "musl" else ""
```

## Loops over formats

A `for` loop writes one package for each item. All of them share the build:

```python
for fmt in ["tar", "zip"]:
    package(fmt, **common)
```

Add a native package to the list when the system has one:

```python
formats = ["tar"]
native = host()["format"]
if native in ["deb", "rpm", "archlinux", "apk"]:
    formats.append(native)

for fmt in formats:
    package(fmt, **common)
```

## Functions

Put what packages share in a function:

```python
def make_package(fmt, **fields):
    package(fmt, name = "tool", version = arg("VERSION", "1.0.0"), description = "A command line tool", **fields)

make_package("tar")
make_package("zip", file = "{name}-{version}-source.zip")
```

A function can also hold build commands. Calling it adds them:

```python
def compile(optimise):
    sh("make OPT=" + optimise)

compile("2")
package("tar", **common)
```

## Split packages

One build can feed several packages. Expose the files for each, then call `package` between the exposures:

```python
common = {"version": "1.0.0", "description": "Part of the hello suite"}

sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

expose(file("hello", "/usr/bin/hello", mode = "0755"))
package("tar", name = "hello", **common)

expose(tree("docs", "/usr/share/doc/hello"))
package("tar", name = "hello-docs", **common)
```

`hello-docs` also holds `/usr/bin/hello`, because `expose` adds to what the script offers. To keep the packages apart,
leave files out of one with `exclude`, or pass the files of each package in `contents` and expose nothing:

```python
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

package("tar", name = "hello", contents = [file("hello", "/usr/bin/hello", mode = "0755")], **common)
package("tar", name = "hello-docs", contents = [tree("docs", "/usr/share/doc/hello")], **common)
```

Files that a command installs into `$pkgdir` go into every package that follows it. Use `exclude` to keep them out of
one:

```python
package("tar", name = "hello", exclude = ["/usr/share/doc/**"], **common)
package("tar", name = "hello-docs", exclude = ["/usr/bin/**"], **common)
```

## Packages without a build

A package with no `sh` before it runs no build. It packs its `contents` only. This is how you package files that you
compiled somewhere else. This one exists only when `STAGE` is set:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}
STAGE = arg("STAGE")

if STAGE:
    package(
        "dmg", distro = "macos",
        contents = [tree(STAGE, "/hello")],
        options = {"volume_name": "hello"},
        **common
    )
```

Without `STAGE`, the script writes no package, which is an error. With `--build-arg STAGE=stage` it writes the disk image.
An empty string is false, so `STAGE=` turns the package off. Test `arg("X") != None` to tell a missing value from an
empty one. See [macOS and Windows](macos-and-windows.md).

## Text with f-strings

An f-string puts values in text. It works in every text field, and in the `run` text of `sh`.

```python
VERSION = arg("VERSION", "1.0.0")

sh(f"make VERSION={VERSION} && make install PREFIX=/usr DESTDIR=\"$pkgdir\"")
```

A shell script uses `$` and braces for its own variables. In an f-string, write `{{` and `}}` for a literal brace.
Outside an f-string the shell text stays as written, so keep a script in a normal string when it has `${...}`:

```python
sh("""
for f in build/*.so; do
  install -Dm755 "$f" "$pkgdir/usr/lib/hello/${f##*/}"
done
""")
```

The values in `env` are literal text. pkgsmith never expands them in a shell.

## Loading other files

`load("file.star", "name")` brings names from another `.star` file. The path is relative to the file that holds the
`load`. A loaded file can call the same functions. Every command, exposure and package that it adds counts.

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

sh("make")
base("tar")
```

Load only what you name. A relative `src` in a loaded file still resolves against the folder of the script you build,
not the folder of the loaded file.

## Data with json

`json.encode(value)` and `json.decode(text)` are available. Use them to build text such as a config file from a dict:

```python
settings = json.encode({"port": 8080, "debug": False})
sh(f"echo '{settings}' > hello.json")
```

## Check the result

Run `pkgsmith plan` after every change. It evaluates the script, lists every package and its file name, and runs no
build. Use `--where` and `-F` to look at part of it:

```console
$ pkgsmith plan --where format=deb
$ pkgsmith plan -F tar,zip
```

An error in the script stops the command with the file, the line and the message. To see what a script does on another
system, you must run `plan` there. `host()` always describes the system that runs it.
