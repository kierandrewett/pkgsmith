# Scripting

A `pkgsmith.star` is a program in Starlark, a small language with the syntax of Python. The script declares packages.
pkgsmith runs it, collects the packages it declared, and builds them. Use ordinary code for what varies: a loop for
every architecture and distro, a function for what packages share, an `if` for a package that is optional.

This page shows how to write that code. The functions and fields are in the [script reference](manifest.md).

## A script that runs

```python
# pkgsmith.star
pkg = package(
    name = "hello",
    version = "1.0.0",
    description = "A small program",
)

build = job(
    runs_on = "fedora:43",
    setup = [step("dnf -y install gcc make")],
    steps = [step("make && make install PREFIX=/usr")],
)

target(pkg, "rpm", distro = "fedora", job = build)
```

Run `pkgsmith plan` to see what the script declares. A script that declares no `target` is an error.

## The language

- Starlark has `def`, `for`, `if`, lists, dicts, comprehensions and f-strings. It has no classes, no `while` and no
  `import`.
- A script cannot read files, use the network or start a process. It can only compute, read build arguments and load
  other `.star` files.
- A call with a name that does not exist, or a field that a function does not accept, is an error that names it.
- The functions of pkgsmith (`package`, `target`, `job`, `step` and the content helpers) need no `load`.

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

## Functions

Put what packages share in a function. A function that returns a package keeps every target short:

```python
def base(name, description):
    return package(
        name = name,
        version = arg("VERSION", "1.0.0"),
        description = description,
        maintainer = "Your Name <you@example.com>",
        license = "MIT",
    )

tool = base("tool", "A command line tool")
tool_docs = base("tool-docs", "Documentation for tool")

target(tool, "tar")
target(tool_docs, "tar")
```

## Loops: architectures and distros

There is no matrix. Write the loop yourself. Each pass makes one or more targets.

### Every architecture

```python
pkg = package(name = "hello", version = "1.0.0", description = "A small program")
build = job(runs_on = "debian:13", steps = [step("make && make install PREFIX=/usr")])

for arch in ["amd64", "arm64"]:
    target(pkg, "deb", distro = "debian", job = build, arch = arch)
```

This declares two packages. A job without `arch` runs for the architecture of each target, so the build runs once for
each architecture. An arm64 build on an x86_64 host needs `qemu-user`. See
[architecture](build-environment.md#architecture).

### Every distro and architecture

Keep one row for each distro. Loop over the rows and then over the architectures:

```python
pkg = package(name = "hello", version = "1.0.0", description = "A small program")

def make_job(image, install):
    return job(
        runs_on = image,
        setup = [step(install)],
        steps = [step("make && make install PREFIX=/usr")],
    )

distros = [
    # distro, format, image, install command, libc package
    ("fedora", "rpm", "fedora:43", "dnf -y install gcc make", "glibc"),
    ("debian", "deb", "debian:13", "apt-get update -qq && apt-get install -y gcc make", "libc6"),
    ("alpine", "apk", "alpine:3.22", "apk add --no-cache gcc make musl-dev", "musl"),
]

for distro, fmt, image, install, libc in distros:
    build = make_job(image, install)
    for arch in ["amd64", "arm64"]:
        target(pkg, fmt, distro = distro, job = build, arch = arch, depends = [libc])
```

This declares six packages. `plan` shows six rows and six builds, one for each distro and architecture.

### A list of targets

A list comprehension is a loop that makes a list. A target call does not return anything, so use a plain `for` to
declare targets. Use a comprehension for the data:

```python
arches = ["amd64", "arm64"]
names = {"amd64": "x86_64", "arm64": "aarch64"}
triples = [names[arch] + "-linux-gnu" for arch in arches]
```

## One build for many packages

Targets whose jobs are equal share one run. Declare the job once, outside the loop, and use it for every target:

```python
pkg = package(name = "hello", version = "1.0.0", description = "A small program")
glibc = job(runs_on = "debian:13", steps = [step("make && make install PREFIX=/usr")])

for arch in ["amd64", "arm64"]:
    target(pkg, "rpm", distro = "fedora", job = glibc, arch = arch, depends = ["glibc"])
    target(pkg, "deb", distro = "debian", job = glibc, arch = arch, depends = ["libc6"])
```

`plan` shows one `BUILD` letter for the rpm and the deb of each architecture. Two jobs are equal when their image,
architecture, setup, steps, environment and excludes are equal, and their source folder is the same. The format and the
distro do not matter. See [build environment](build-environment.md#one-build-many-packages).

## Conditions

Use `if`. These forms are the ones a script needs most.

A target for one architecture only:

```python
pkg = package(name = "hello", version = "1.0.0", description = "A small program")
build = job(runs_on = "archlinux:base", steps = [step("make && make install PREFIX=/usr")])

for arch in ["amd64", "arm64"]:
    # The archlinux image has no arm64 build.
    if arch == "amd64":
        target(pkg, "archlinux", distro = "arch", job = build, arch = arch)
```

A different value for one format:

```python
def libc(fmt):
    if fmt == "deb":
        return "libc6"
    if fmt == "apk":
        return "musl"
    return "glibc"
```

A different value in one expression:

```python
triple = "aarch64-linux-gnu" if arch == "arm64" else "x86_64-linux-gnu"
```

## Optional targets

A target that depends on a build argument is an `if` on `arg`. This one exists only when `STAGE` is set:

```python
pkg = package(name = "hello", version = "1.0.0", description = "A small program")
STAGE = arg("STAGE")

target(pkg, "tar", contents = [file("README.md", "/usr/share/doc/hello/README.md")])

if STAGE:
    target(
        pkg, "dmg", distro = "macos",
        contents = [tree(STAGE, "/hello")],
        options = {"volume_name": "hello"},
    )
```

Without `STAGE`, `pkgsmith plan` lists only the tar. With `--build-arg STAGE=stage` it lists the disk image too.
A target without a `job` runs no build. It packs its `contents` and needs no Podman. See
[macOS and Windows](macos-and-windows.md).

An empty string is false, so `STAGE=` turns the target off. Test `arg("X") != None` when you need to tell a missing
value from an empty one.

## Text with f-strings

An f-string puts values in text. It works in every text field, and in the `run` text of a step.

```python
VERSION = arg("VERSION", "1.0.0")
arch = "arm64"

label = f"hello-{VERSION}-{arch}"
build = step(f"make VERSION={VERSION} && make install PREFIX=/usr")
```

A shell script uses `$` and braces for its own variables. In an f-string, write `{{` and `}}` for a literal brace.
Outside an f-string the shell text stays as written, so keep the script in a normal string when it has `${...}`:

```python
step("""
for f in build/*.so; do
  install -Dm755 "$f" "/usr/lib/hello/${f##*/}"
done
""")
```

The values in `env` are literal text. pkgsmith never expands them in a shell.

## Loading other files

`load("file.star", "name")` brings names from another `.star` file. The path is relative to the file that holds the
`load`. A loaded file can use the same functions, and the targets it declares count.

```python
# common.star
def base(name):
    return package(
        name = name,
        version = arg("VERSION", "1.0.0"),
        description = "Part of the hello suite",
        license = "MIT",
    )

debian_job = job(runs_on = "debian:13", steps = [step("make && make install PREFIX=/usr")])
```

```python
# pkgsmith.star
load("common.star", "base", "debian_job")

target(base("hello-core"), "deb", distro = "debian", job = debian_job)
target(base("hello-extra"), "deb", distro = "debian", job = debian_job)
```

Load only what you name. A relative `src` in a loaded file still resolves against the folder of the script you build,
not the folder of the loaded file.

## Data with json

`json.encode(value)` and `json.decode(text)` are available. Use them to build text such as a config file from a dict:

```python
settings = json.encode({"port": 8080, "debug": False})
build = step(f"echo '{settings}' > /etc/hello.json")
```

## Check the result

Run `pkgsmith plan` after every change. It evaluates the script, lists every package and its file name, and starts no
container. Use `--where` and `-F` to look at part of it:

```console
$ pkgsmith plan --where arch=arm64
$ pkgsmith plan -F deb --where distro=debian
```

An error in the script stops the command with the file, the line and the message. A target that is wrong stops it with
the number of the target, counted in the order the script declared them.
