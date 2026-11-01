# Matrix and expressions

A manifest works like a GitHub Actions workflow. `targets` and a `matrix` make the packages. `${{ }}` expressions and
`if:` see the values of each package.

## Targets

A target is a distro, the formats to write for it, and the job that builds its files. Every target is crossed with the
cells of the `matrix`, so a target written once is built for every architecture.

```yaml
targets:
  - format: rpm
    distro: fedora
    needs: glibc
  - format:
      - deb
      - tar
    distro: debian
    needs: glibc
  - format: apk
    distro: alpine
    needs: musl
matrix:
  arch:
    - amd64
    - arm64
```

That is 4 packages for each architecture, so 8 in all.

| Key      | Meaning                                                                                                    |
| -------- | ---------------------------------------------------------------------------------------------------------- |
| `format` | One format, or a list. Each format is a package of its own. Required                                       |
| `distro` | Selects the `overrides` entry of that name, and names the lane in the progress output                      |
| `needs`  | The job whose files the target packs. See [build environment](build-environment.md#which-job-a-package-uses) |
| `file`   | The name of the output file. A template. See [naming the output file](#naming-the-output-file)             |
| `if`     | An expression over `env.NAME`. A false target makes no packages. See [dropping a target](#dropping-a-target-with-if) |

Any other key is a variable. `${{ matrix.size }}` reads it in `contents` and `if:`.

A key of a target cannot also be an axis of the `matrix`: the target sets it for itself.

Without `targets`, the `matrix` alone makes the packages, and `format` and `distro` can be matrix keys.

## The matrix

The matrix holds what varies for every target, such as the architecture.

```yaml
matrix:
  arch:
    - amd64
    - arm64
  exclude:
    - distro: alpine
      arch: arm64
```

Every key except `include` and `exclude` is an axis: a name with a list of values. pkgsmith makes one cell for every
combination of the axes, then applies `include`. Each cell is crossed with every target. Then `exclude` drops the
packages that match, so it can name `distro` and `format` of the targets.

### Reserved keys

| Key      | Effect                                                                                           |
| -------- | ------------------------------------------------------------------------------------------------ |
| `format` | The format of the package. Without this key, a package is written in every format the command line asks for |
| `arch`   | The architecture. Overrides `arch` in the manifest                                               |
| `distro` | Selects the matching `overrides` entry                                                           |
| `file`   | The name of the output file. A template                                                          |
| `needs`  | The job that builds the files                                                                    |

`file` and `needs` are not shown in package names. A build job sees only the keys of its own matrix, never these. See
[build environment](build-environment.md#what-a-job-can-see).

### Naming the output file

Every format picks a default file name, such as `hello-1.0.0-amd64.tar.xz`. `pkgsmith plan` shows it. A `file`
template on a target replaces it.

| Placeholder  | Value                                                            |
| ------------ | ---------------------------------------------------------------- |
| `{file}`     | The name the format picked                                       |
| `{name}`, `{version}`, `{release}` | The package fields                         |
| `{arch}`     | `amd64`, `arm64` or `all`                                        |
| `{format}`, `{distro}` | The format and the distro of the package               |

```yaml
targets:
  - format: tar
    distro: fedora
    file: "{name}_{version}.tar.bz2"
  - format: rpm
    distro: opensuse
    file: "opensuse-{file}"
```

The result must be a plain file name: not empty, not starting with a dot, and without `/`. The template names the whole
file, so write the extension yourself, or keep it with `{file}`.

Two packages that would write the same file stop the build before it starts, because they run side by side. This
happens by default when two distros make the same format, for example an rpm for Fedora and one for openSUSE. Give one
of them a `file` template.

Each package builds in a private folder and moves its file to the final name, so the folder `-o` receives only finished
packages.

### Dropping a target with `if`

A target may carry `if:`. It is an expression, like the `if:` of a `contents` entry. When it is false, the target makes no
packages. `if` is not a matrix value, so it does not appear in the package name.

The expression sees `env`, which holds the environment and every `--build-arg`. This target exists only when you pass
`--build-arg STAGE=...`:

```yaml
targets:
  - format: dmg
    distro: macos
    needs: []
    if: env.STAGE
```

Without `STAGE`, `pkgsmith plan` does not list the target. With `--build-arg STAGE=stage` it does. An expression that
cannot be evaluated stops the run with an error.

### `exclude`

An entry removes every package that has all of its keys. This removes the Debian arm64 package:

```yaml
matrix:
  exclude:
    - distro: debian
      arch: arm64
```

An entry may use a single key: `arch: arm64` removes every arm64 package.

### `include`

`include` adds values to cells, or adds cells. It follows the rule of GitHub Actions.

1. For each entry, find the cells that match it. A cell matches when every key of the entry that is also an **axis**
   has the value of that cell. Keys of the entry that are not axes do not take part in matching.
2. If there are matching cells, add the extra keys to them. A value that is already set on a cell stays.
3. If no cell matches, the entry becomes a new cell.

Only the cells that the axes made can match. A cell that an earlier entry added never matches a later entry.

The `matrix` of a job works the same way. This is how a job gets a variable for each architecture:

```yaml
jobs:
  build:
    runs-on: debian:13
    matrix:
      arch:
        - amd64
        - arm64
      include:
        - arch: amd64
          triple: x86_64-linux-gnu
        - arch: arm64
          triple: aarch64-linux-gnu
    steps:
      - run: echo "building for $MATRIX_TRIPLE"
```

### `pkgsmith plan`

`pkgsmith plan` prints the packages without running anything. Use it to check targets and a matrix:

```console
$ pkgsmith plan examples/full.yaml
  #  FORMAT  DISTRO  ARCH   IMAGE      BUILD  FILE
  1  rpm     fedora  amd64  fedora:43  a      acme-tool-0.4.2-1.x86_64.rpm
...
```

### Selecting from the command line

| Option                       | Effect                                                          |
| ---------------------------- | --------------------------------------------------------------- |
| `-F deb,rpm`                 | Only these formats                                              |
| `--job distro=fedora`        | Only packages with this value. Repeat for more keys             |
| `--platform linux/arm64`     | Only packages with `arch: arm64`                                |

## Expressions

An expression sits between `${{` and `}}` in text, or by itself in `if:`.

```yaml
src: ${STAGE}-${{ matrix.arch }}/usr/bin/tool
dst: /usr/bin/tool
if: matrix.arch == 'arm64' && format != 'tar'
```

### Names

| Name              | Value                                                       |
| ----------------- | ----------------------------------------------------------- |
| `matrix.NAME`     | The value of the axis `NAME` for this job                   |
| `env.NAME`        | The process environment, `--build-arg`, then the manifest `env:` |
| `format`          | The format of this build: `deb`, `rpm`, ...                 |
| `distro`          | The matrix `distro`, or empty                               |
| `arch`            | `amd64`, `arm64` or `all`                                   |

A name that is not set is empty and false.

### Values and operators

- Strings in single quotes: `'amd64'`. Write a quote inside a string twice: `'it''s'`.
- Numbers: `1`, `2.5`. `true`, `false`, `null`.
- Comparison: `==` and `!=`. String comparison ignores case, so `distro == 'Fedora'` matches `fedora`.
- Logic: `!`, `&&`, `||`, and parentheses.
- `&&` and `||` return a value, as in GitHub Actions. That gives you a conditional value:
  `${{ matrix.arch == 'arm64' && 'aarch64' || 'x86_64' }}` is `aarch64` or `x86_64`.
- Falsy values: `false`, `null`, `0`, the empty string. Everything else is true.

### Functions

| Function                | Result                                                                 |
| ----------------------- | ---------------------------------------------------------------------- |
| `contains(text, part)`  | `true` when `text` has `part`, ignoring case                           |
| `startsWith(text, part)`| `true` when `text` starts with `part`                                  |
| `endsWith(text, part)`  | `true` when `text` ends with `part`                                    |
| `format('{0}-{1}', a, b)`| Replaces `{0}`, `{1}`, ... with the arguments                         |
| `lower(text)`, `upper(text)` | Changes the case                                                  |

An expression with a syntax error is an error. It is never silently false.

### Variables with `${NAME}`

`${NAME}` reads the environment and `--build-arg`. `${NAME:-default}` uses `default` when `NAME` is unset or empty.
An unset name without a default is an error that names the variable.

### Where each form works

| Field                                                      | `${NAME}` | `${{ }}` |
| ---------------------------------------------------------- | --------- | -------- |
| `name`, `version`, `release`, `maintainer`, `homepage`, `license` | yes | yes      |
| `depends` and the other relation lists                     | yes       | yes      |
| `contents` `src` and `dst`                                 | yes       | yes      |
| `contents` `if`, and `if` of a matrix `include` entry      | no        | yes (bare expression) |
| `runs-on` of a job                                         | yes       | yes      |
| `run` and `env` of a step, `scripts`                       | no        | `${{ }}` only in `run` and `env` |

## Examples

Give a build a value per architecture. The job has a matrix of its own, so each value is a variable of the steps:

```yaml
jobs:
  build:
    runs-on: debian:13
    matrix:
      arch:
        - amd64
        - arm64
      include:
        - arch: amd64
          triple: x86_64-linux-gnu
        - arch: arm64
          triple: aarch64-linux-gnu
    steps:
      - run: echo "building for $MATRIX_TRIPLE"
```

Build one file only for some formats:

```yaml
contents:
  - { src: packaging/init, dst: /etc/init.d/tool, mode: "0755", if: "format == 'ipk' || format == 'deb'" }
```

Different dependency for one distro:

```yaml
depends: [glibc]
overrides:
  debian:
    depends: [libc6]
```
