# Matrix and expressions

A manifest works like a GitHub Actions workflow. A `matrix` expands into jobs. `${{ }}` expressions and `if:` see the
values of the job.

## The matrix

```yaml
matrix:
  arch: [amd64, arm64]
  distro: [fedora, debian]
  include:
    - { distro: fedora, format: rpm }
    - { distro: debian, format: deb }
  exclude:
    - { distro: debian, arch: arm64 }
```

Every key except `include` and `exclude` is an axis: a name with a list of values. pkgsmith builds one job for every
combination of the axes (here 2 x 2 = 4), then applies `exclude`, then `include`.

### Reserved keys

| Key      | Effect                                                                                       |
| -------- | -------------------------------------------------------------------------------------------- |
| `format` | The format of the job. Without this key, a job builds every format the command line asks for |
| `arch`   | The architecture. Overrides `arch` in the manifest                                           |
| `distro` | Selects the matching `overrides` entry                                                       |
| `file`   | The name of the output file. A template. See below                                           |

`file` is not exported as a `MATRIX_` variable and is not shown in job names. A build sees only the matrix keys that
the `by` of its builder lists. See [build environment](build-environment.md#builders).

### Naming the output file

Every format picks a default file name, such as `hello-1.0.0-amd64.tar.xz`. `pkgsmith plan` shows it. A `file`
template in a matrix entry replaces it.

| Placeholder  | Value                                                            |
| ------------ | ---------------------------------------------------------------- |
| `{file}`     | The name the format picked                                       |
| `{name}`, `{version}`, `{release}` | The package fields                         |
| `{arch}`     | `amd64`, `arm64` or `all`                                        |
| `{format}`, `{distro}` | The format and the distro of the job                   |

```yaml
matrix:
  include:
    - { format: tar, distro: fedora, file: "{name}_{version}.tar.bz2" }
    - { format: rpm, distro: opensuse, file: "opensuse-{file}" }
```

The result must be a plain file name: not empty, not starting with a dot, and without `/`. The template names the whole
file, so write the extension yourself, or keep it with `{file}`.

Two jobs that would write the same file stop the build before it starts, because they run side by side. This happens by
default when two distros make the same format, for example an rpm for Fedora and one for openSUSE. Give one of them a
`file` template.

Each job builds in a private folder and moves its file to the final name, so the folder `-o` receives only finished
packages.

Any other key is a variable. `${{ matrix.size }}` reads it, and a build script sees it as `MATRIX_SIZE`.

Values may be text, numbers or `true` / `false`. All are kept as text.

### `exclude`

An entry removes every job that has all of its keys. `{ distro: debian, arch: arm64 }` removes the Debian arm64 job.
An entry may use a single key: `{ arch: arm64 }` removes every arm64 job.

### `include`

`include` follows the rule of GitHub Actions.

1. For each entry, find the jobs that match it. A job matches when every key of the entry that is also an **axis**
   has the value of that job. Keys of the entry that are not axes do not take part in matching.
2. If there are matching jobs, add the extra keys to them. A value that is already set on a job stays.
3. If no job matches, the entry becomes a new job.

Only the jobs that the axes made can match. A job that an earlier entry added never matches a later entry, so two entries
with the same axis values, such as `{ format: msi, distro: windows }` and `{ format: nsis, distro: windows }`, stay two jobs.

That makes two common shapes work.

**Add values to jobs the axes already made.** Give each distro its format:

```yaml
matrix:
  distro: [fedora, debian]
  include:
    - { distro: fedora, format: rpm }
    - { distro: debian, format: deb }
```

Jobs: `distro=fedora, format=rpm` and `distro=debian, format=deb`.

**List the jobs by hand.** With no axes, every entry is a job:

```yaml
matrix:
  include:
    - { distro: fedora, format: rpm }
    - { distro: debian, format: deb }
    - { distro: debian, format: tar }
```

To cross the hand-written jobs with architectures, make `distro` an axis and `include` the format as in
the first shape. One distro that needs two formats needs two jobs, so list the jobs by hand and add `arch` as a
second axis only when each job has all its keys in `include`.

### Dropping a job with `if`

An `include` entry may carry `if:`. It is an expression, like the `if:` of a `contents` entry. When it is false, pkgsmith
drops the job. `if` is not a matrix value, so it does not appear in the job label.

The expression sees `env`, which holds the environment and every `--build-arg`, the matrix values of the job, `format`,
`distro` and `arch`. This job exists only when you pass `--build-arg STAGE=...`:

```yaml
matrix:
  include:
    - { format: dmg, arch: arm64, distro: macos, if: env.STAGE }
```

Without `STAGE`, `pkgsmith plan` does not list the job. With `--build-arg STAGE=stage` it does. An expression that
cannot be evaluated stops the run with an error.

### `pkgsmith plan`

`pkgsmith plan` prints the jobs without running anything. Use it to check a matrix:

```console
$ pkgsmith plan examples/full.yaml
rpm [arch=amd64, distro=fedora, format=rpm, image=fedora:43]
...
```

### Selecting from the command line

| Option                       | Effect                                                          |
| ---------------------------- | --------------------------------------------------------------- |
| `-F deb,rpm`                 | Only these formats. A job with a `format` axis value outside the list is skipped |
| `--job distro=fedora`        | Only jobs with this value. Repeat for more keys                 |
| `--platform linux/arm64`     | Only jobs with `arch: arm64`                                    |

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
| `image` of a builder                                       | yes       | yes      |
| `run` and `env` of a step, `scripts`                       | no        | `${{ }}` only in `run` and `env` |

## Examples

Give a build a value per architecture. The builder lists the key in `by`, so each value makes its own run:

```yaml
matrix:
  arch: [amd64, arm64]
  include:
    - { arch: amd64, triple: x86_64-linux-gnu }
    - { arch: arm64, triple: aarch64-linux-gnu }
builders:
  main:
    image: debian:13
    by: [triple]
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
