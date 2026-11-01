# Script reference

A `pkgsmith.star` declares the packages to build. This page lists every function and field. To learn how to write
loops, conditions and shared code, read [scripting](scripting.md).

The default file name is `pkgsmith.star`. `-f FILE` names another. The script is Starlark, with f-strings. It cannot
read files, use the network or start a process. Relative `src` paths in `contents` and script file names resolve against
the folder of the script.

A field that a function does not know is an error that names it, so a typo is not a silent no-op.

## A complete example

```python
pkg = package(
    name = "bingux-config",
    version = arg("VERSION", "0.1.0"),
    description = """Configuration command for the Bingux desktop
Reads, changes and checks the Bingux configuration file.
""",
    maintainer = "Kieran Drewett <kieran@drewett.dev>",
    homepage = "https://github.com/kierandrewett/bingux",
    license = "GPL-3.0-or-later",
    scripts = {"post_install": 'echo "bingux-config installed"'},
)

build = step(
    name = "Build",
    run = """
cargo build --release --manifest-path crates/config/Cargo.toml
install -Dm755 target/release/bingux-config /usr/bin/bingux-config
""",
)

fedora = job(
    runs_on = "fedora:43",
    setup = [step("dnf -y install cargo")],
    steps = [build],
    exclude = ["/usr/share/doc/**"],
)
debian = job(
    runs_on = "debian:13",
    setup = [step("apt-get update -qq && apt-get install -y cargo")],
    steps = [build],
    exclude = ["/usr/share/doc/**"],
)

for arch in ["amd64", "arm64"]:
    target(pkg, "rpm", distro = "fedora", job = fedora, arch = arch, depends = ["glibc"])
    target(pkg, "deb", distro = "debian", job = debian, arch = arch, depends = ["libc6"])
```

## arg

`arg(name, default=None)` reads a build argument.

| Argument  | Type | Default | Meaning                                                                         |
| --------- | ---- | ------- | ------------------------------------------------------------------------------- |
| `name`    | text | required | The name of the argument                                                       |
| `default` | any  | `None`  | The result when there is neither a `--build-arg` nor an environment variable    |

The result is, in order: the value of `--build-arg NAME=VALUE`, the environment variable `NAME`, then `default`.

```python
VERSION = arg("VERSION", "0.4.2")
```

## package

`package(...)` returns a dict with the fields that most targets share. It declares nothing by itself. Pass it to
`target`.

| Field         | Type   | Default  | Meaning                                                                                  |
| ------------- | ------ | -------- | ---------------------------------------------------------------------------------------- |
| `name`        | text   | required | Package name. Lowercase letters, digits and `+ - . _`. Starts with a letter or digit     |
| `version`     | text   | required | `MAJOR.MINOR.PATCH` or `MAJOR.MINOR.PATCH-prerelease`. See [versions](versions.md)       |
| `description` | text   | required | The first line is the summary. The rest is the long text                                 |
| `release`     | text   | `"1"`    | Package revision for one version. Reset it to `"1"` when the version changes. A number is accepted |
| `epoch`       | number | `0`      | Makes the version newer than every version with a lower epoch. `0` leaves it out. See [versions](versions.md#epoch) |
| `arch`        | text   | `"amd64"` | `amd64`, `arm64` or `all`. See below                                                    |
| `maintainer`  | text   | `None`   | `Name <email>`                                                                           |
| `homepage`    | text   | `None`   | URL                                                                                      |
| `license`     | text   | `None`   | License expression, for example `GPL-3.0-or-later`                                       |
| `depends`     | list   | `[]`     | See [dependencies](dependencies.md)                                                      |
| `recommends`  | list   | `[]`     | See [dependencies](dependencies.md)                                                      |
| `suggests`    | list   | `[]`     | See [dependencies](dependencies.md)                                                      |
| `conflicts`   | list   | `[]`     | See [dependencies](dependencies.md)                                                      |
| `replaces`    | list   | `[]`     | See [dependencies](dependencies.md)                                                      |
| `provides`    | list   | `[]`     | See [dependencies](dependencies.md)                                                      |
| `contents`    | list   | `[]`     | Files to add, or to change in the tracked list. See [contents](contents.md)              |
| `scripts`     | dict   | `{}`     | Install scripts. See [scripts](#scripts)                                                 |
| `options`     | dict   | `{}`     | Options of the format. See [options](#options)                                           |

`arch` values:

| Value   | Meaning                                                             |
| ------- | ------------------------------------------------------------------- |
| `amd64` | x86-64                                                              |
| `arm64` | 64-bit Arm                                                          |
| `all`   | A package without compiled code. `--platform` never removes it      |

### Dependencies and relations

Each field is a list of package names, written in the syntax of the target format.

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

A value with a line break is the script. A one-line value is the name of a file next to the script. See
[scripts](scripts.md).

```python
scripts = {
    "post_install": "systemctl daemon-reload || true",
    "post_remove": "packaging/post-remove.sh",
}
```

### Options

`options` is a dict of options for one format. The plugin that writes the format defines the keys, and a key it does not
know is an error. Options belong to a target, so give each format its own:

```python
target(pkg, "deb", distro = "debian", job = build, options = {"section": "utils"})
target(pkg, "tar", options = {"compression": "zstd", "top_folder": True})
```

See [formats](formats.md) for the options of every format.

## target

`target(pkg, format, ...)` declares one package: `pkg` written as `format`.

| Argument     | Type   | Default  | Meaning                                                                                      |
| ------------ | ------ | -------- | -------------------------------------------------------------------------------------------- |
| `pkg`        | dict   | required | The result of `package`                                                                      |
| `format`     | text   | required | A name from `pkgsmith formats`, for example `rpm`, `deb`, `archlinux`, `apk`, `tar`          |
| `distro`     | text   | `None`   | Names the lane in the progress output and the distro a plugin may adapt to. `--where distro=NAME` selects it |
| `job`        | dict   | `None`   | The result of `job`. Without it the package holds its `contents` only and no build runs      |
| `file`       | text   | `None`   | The name of the output file. A template. See [naming the output file](#naming-the-output-file) |
| `tags`       | dict   | `{}`     | Text labels for `--where`. Keys and values are text                                          |
| any field of `package` | | the value of `pkg` | Replaces that field for this target, for example `depends = ["libc6"]`, `arch = "arm64"` |

`target` fails when a keyword is neither an argument nor a field of `pkg`. The error lists the fields.

A list or dict that you pass replaces the whole value in `pkg`. It does not merge with it. To add to a list, write
`depends = pkg["depends"] + ["extra"]`.

```python
target(pkg, "rpm", distro = "fedora", job = build, depends = ["glibc"], tags = {"stage": "release"})
```

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
target(pkg, "tar", file = "{name}_{version}.tar.bz2", options = {"compression": "bzip2"})
target(pkg, "rpm", distro = "opensuse", file = "opensuse-{file}")
```

The result must be a plain file name: not empty, not starting with a dot, and without `/`. The template names the whole
file, so write the extension yourself, or keep it with `{file}`. To write a literal brace in an f-string, double it.

Two packages that would write the same file stop the build before it starts, because they run side by side. This
happens when two distros make the same format, for example an rpm for Fedora and one for openSUSE. Give one of them a
`file` template.

Each package builds in a private folder and moves its file to the final name, so the folder `-o` receives only finished
packages.

### Selecting with tags

`--where KEY=VALUE` selects targets by `format`, `distro`, `arch`, `name` and by the keys of `tags`. Use `tags` to name a
group of targets:

```python
target(pkg, "deb", distro = "debian", job = build, tags = {"channel": "nightly"})
```

```console
$ pkgsmith build --where channel=nightly
```

## job

`job(runs_on, steps, ...)` describes a build in a container. The files that its steps install are packaged. See
[build environment](build-environment.md).

| Argument  | Type | Default            | Meaning                                                                           |
| --------- | ---- | ------------------ | --------------------------------------------------------------------------------- |
| `runs_on` | text | required           | The container image. pkgsmith pulls it when it is not on the machine              |
| `steps`   | list | required           | The `step` results that make the files, in order. At least one                    |
| `setup`   | list | `[]`               | Steps that prepare the image. They run once and are cached. What they install is not packaged |
| `env`     | dict | `{}`               | Variables for every step. Names use letters, digits and `_`. The values are literal text |
| `exclude` | list | `[]`               | Paths (globs) the package leaves out, such as `/usr/share/doc/**`                 |
| `arch`    | text | the arch of the target | `amd64`, `arm64` or `all`. Runs the job for this architecture, whatever the target |

A job sees only its own fields and the architecture. It never sees the format or the distro of a target. Targets whose
jobs are equal share one run.

## step

`step(run, name=None, env={})` is one script of a job.

| Argument | Type | Default | Meaning                                                                       |
| -------- | ---- | ------- | ----------------------------------------------------------------------------- |
| `run`    | text | required | The script. It runs with `sh -e` in `/src`                                    |
| `name`   | text | `None`  | Shown in the progress output                                                  |
| `env`    | dict | `{}`    | Variables for this step, after those of the job. The values are literal text  |

## Content helpers

Each helper returns one entry for the `contents` list. See [contents](contents.md) for what each type does.

| Function                                    | Entry                                                                  |
| ------------------------------------------- | ---------------------------------------------------------------------- |
| `file(src, dst, mode, owner, group, config=False)` | A file, or the files that a glob matches. `config=True` makes it a configuration file |
| `config(src, dst, mode, owner, group)`      | A configuration file                                                   |
| `tree(src, dst, mode, owner, group)`        | A folder copied recursively, including the folder itself               |
| `symlink(link_target, dst)`                 | A symbolic link at `dst` that points to `link_target`                  |
| `directory(dst, mode, owner, group)`        | An empty folder that the package owns                                  |
| `ghost(dst)`                                | A path the package owns but does not carry. Only `rpm` records it      |

| Argument | Type | Default | Meaning                                                                          |
| -------- | ---- | ------- | -------------------------------------------------------------------------------- |
| `src`    | text | required | Where the content comes from, relative to the folder of the script. Pass `None` to change the tracked file at `dst` |
| `dst`    | text | required | An absolute path on the installed system                                         |
| `mode`   | text | the mode of the source | Permissions as octal text, for example `"0755"`                  |
| `owner`  | text | `root`  | User name                                                                         |
| `group`  | text | `root`  | Group name                                                                        |

```python
contents = [
    file("packaging/app.desktop", "/usr/share/applications/app.desktop", mode = "0644"),
    config("packaging/app.conf", "/etc/app.conf"),
    symlink("/usr/lib/app/bin/app", "/usr/bin/app"),
]
```

## Command line

`pkgsmith build`, `plan` and `files` read the script, and `--build-arg`, `--where`, `-F` and `--platform` choose
among its targets. See [command line](cli.md).
