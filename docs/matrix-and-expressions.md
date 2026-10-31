# Matrix and expressions

A manifest works like a GitHub Actions workflow. A `matrix` expands into jobs. `${{ }}` expressions and `if:` see the
values of the job.

## The matrix

```yaml
matrix:
  arch: [amd64, arm64]
  distro: [fedora, debian]
  include:
    - { distro: fedora, image: "fedora:43", format: rpm }
    - { distro: debian, image: "debian:13", format: deb }
  exclude:
    - { distro: debian, arch: arm64 }
```

Every key except `include` and `exclude` is an axis: a name with a list of values. pkgsmith builds one job for every
combination of the axes (here 2 x 2 = 4), then applies `exclude`, then `include`.

### Reserved axes

| Axis     | Effect                                                                                       |
| -------- | -------------------------------------------------------------------------------------------- |
| `format` | The format of the job. Without this axis, a job builds every format the command line asks for |
| `arch`   | The architecture. Overrides `arch` in the manifest                                           |
| `distro` | Selects the matching `overrides` entry                                                       |

Any other axis is a variable. `${{ matrix.image }}` reads it, and a build script sees it as `MATRIX_IMAGE`.

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

That makes two common shapes work.

**Add values to jobs the axes already made.** Give each distro its image and format:

```yaml
matrix:
  distro: [fedora, debian]
  include:
    - { distro: fedora, image: "fedora:43", format: rpm }
    - { distro: debian, image: "debian:13", format: deb }
```

Jobs: `distro=fedora, image=fedora:43, format=rpm` and `distro=debian, image=debian:13, format=deb`.

**List the jobs by hand.** With no axes, every entry is a job:

```yaml
matrix:
  include:
    - { distro: fedora, image: "fedora:43", format: rpm }
    - { distro: debian, image: "debian:13", format: deb }
    - { distro: debian, image: "debian:13", format: tar }
```

To cross the hand-written jobs with architectures, make `distro` an axis and `include` the image and format as in
the first shape. One distro that needs two formats needs two jobs, so list the jobs by hand and add `arch` as a
second axis only when each job has all its keys in `include`.

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
| `contents` `if`                                            | no        | yes (bare expression) |
| `image`                                                    | yes       | yes      |
| `setup`, `build`, `scripts`                                | no        | `${{ }}` only in `setup` and `build` |

## Examples

Pick the image, the architecture and the filename per job:

```yaml
matrix:
  arch: [amd64, arm64]
  include:
    - { arch: amd64, triple: x86_64-linux-gnu }
    - { arch: arm64, triple: aarch64-linux-gnu }
build: |
  echo "building for $MATRIX_TRIPLE"
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
