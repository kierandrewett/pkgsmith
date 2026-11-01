# Building

`pkgsmith build` runs your script on this system. The `sh` commands of the script run here, in the folder of the script,
the way `makepkg` runs a build. pkgsmith then packs what they made. It needs no daemon. This page
explains how the build runs and what you can rely on.

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

sh("make")                                           # runs in the folder of the script
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')     # installs into the package folder

package("tar", **common)
```

The build needs your tools on this system: the compiler, `make`, and whatever else the commands call. To build for
macOS or Windows, see [macOS and Windows](macos-and-windows.md).

## The folders

| Variable   | Value                                                                                       |
| ---------- | ------------------------------------------------------------------------------------------- |
| `$srcdir`  | The folder of the script, as an absolute path. Every command starts here                    |
| `$pkgdir`  | An empty folder that pkgsmith makes for each build. Install into it                         |

- Anything a command installs into `$pkgdir` is packaged, at the path it has below `$pkgdir`.
  `make install DESTDIR="$pkgdir"` puts `/usr/bin/hello` at `$pkgdir/usr/bin/hello`, and the package has `/usr/bin/hello`.
- Files that the build leaves in `$srcdir`, such as `target/release/hello`, are not packaged by themselves. Name them
  with `expose`. See [contents](contents.md).
- The build writes into your real folder. pkgsmith does not copy it. Keep build output in a folder that your version
  control ignores.
- `$pkgdir` lives in the work folder, `.pkgsmith` by default. `--work DIR` changes it. pkgsmith deletes and recreates a
  build's folder when the build starts.

## Commands

`sh(run, name=None, env={})` adds one command. See the [script reference](manifest.md#sh).

- Every command runs in a **fresh `sh -e`**. It does not see what an earlier command exported or where it changed
  directory. It does see the files that earlier commands wrote.
- A command that fails stops the build. So does any line of it that fails, because of `-e`.
- `sh` is `dash` on Debian and `ash` on Alpine. A `#!` line is not special: the text always runs with `sh -e`. Call
  `bash script.sh` when you need bash.
- Variables from `env` are single-quoted, so quotes, `$` and backticks in a value stay literal.

### Environment

| Variable           | Value                                                          |
| ------------------ | -------------------------------------------------------------- |
| `PKGSMITH_NAME`    | `name`                                                         |
| `PKGSMITH_VERSION` | `version`, as written in the script                            |
| `PKGSMITH_RELEASE` | `release`                                                      |
| `PKGSMITH_ARCH`    | `amd64` or `arm64`                                             |

### Passing values to later commands

Two files carry changes from one command to the next, like `GITHUB_ENV` and `GITHUB_PATH` in GitHub Actions.

```sh
echo "CARGO_HOME=$srcdir/.cargo" >> "$PKGSMITH_ENV"     # NAME=value, one per line
echo "$HOME/.cargo/bin" >> "$PKGSMITH_PATH"             # one folder per line, put in front of PATH
```

pkgsmith reads both as data, never as shell. The text after the first `=` is the value as it is. A line that is not
`NAME=value` stops the command. The `env` of a command wins over a line of `$PKGSMITH_ENV`.

The two files belong to one build. pkgsmith removes them when the build ends.

## What gets packaged

A package holds, in this order:

1. The files in `$pkgdir`, minus the paths of `exclude`.
2. The files that `expose` offered before the `package` call.
3. The `contents` entries of the call.

An entry with the same path as an earlier one replaces it. This is how you mark an installed file as configuration, or
change its mode, without listing every other file:

```python
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')
expose(config(None, "/etc/hello.conf"))
package("tar", **common)
```

### Leaving files out

`exclude` lists paths in `$pkgdir` that the package leaves out. An entry is a path prefix or a glob:

```python
package("deb", exclude = ["/usr/share/doc/**", "/usr/lib/debug", "/usr/share/man"], **common)
```

Packages with different `exclude` lists do not share a build.

## One build, many packages

Packages share one run of the build when they come after the same commands. Order the script so that the commands come
first, then the `package` calls:

```python
sh("make")
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

package("rpm", depends = ["glibc"], **common)
package("deb", depends = ["libc6"], **common)
package("tar", **common)
```

`pkgsmith plan` shows one `BUILD` letter for the three packages. A later `sh` call makes a longer build for the packages
after it, and that is a build of its own.

Two packages share a build when these are equal:

- The commands, in order, with their `env`.
- The `exclude` list.
- The folder of the script.
- The `name`, `version`, `release` and `arch`, because every command sees them as `PKGSMITH_*` variables.

The format and the distro do not matter. `-j N` runs up to `N` builds at the same time, and a package waits for its
build to end.

## Packages for other distros

A script writes packages for the system it runs on. pkgsmith builds only on the system it runs on. To get a
package for another distro or architecture, run pkgsmith on that system:

- **In CI**: run one job for each system, for example on `ubuntu-latest`, `ubuntu-24.04-arm`, `macos-latest` and
  `windows-latest`. Each job runs your script and writes its own packages.
- **In a virtual machine or on a remote host that you start yourself**: copy the project in, run `pkgsmith build` there and
  copy `dist` out.
- **With `--where`**: when one script covers several formats, `--where format=deb` writes only the deb packages on the
  system that can.

The script adapts with `host()`: it names the right dependencies and writes the native format on each system. See
[scripting](scripting.md#the-system-you-run-on).

A program built on one distro runs on distros with the same or a newer glibc. Build on the one with the oldest glibc
that you support, and test the packages on the others. A distro with another libc, such as Alpine with musl, needs a
build of its own.

A build can only make the architecture of the system that runs it, or `all`. Run pkgsmith on an arm64 system for arm64
packages. For macOS and Windows, see [macOS and Windows](macos-and-windows.md).

## Troubleshooting

| Message                                                    | Cause and fix                                                         |
| ---------------------------------------------------------- | --------------------------------------------------------------------- |
| `the build failed`                                         | A command exited with an error. Its output is printed under the progress block |
| `a build on this system (arch amd64) cannot make arch arm64` | The package asks for an architecture this system is not. Run pkgsmith on that system, or use `arch = "all"` for a package without compiled code |
| `declares no package`                                      | No `package` call ran. Check your `if` conditions with `pkgsmith plan` |
| A package contains too much                                | Add `exclude` entries. `pkgsmith files` shows the list                |
| A file is missing from the package                         | A file in `$srcdir` needs `expose`. A file the build installs must go into `$pkgdir` |
| `PKGSMITH_ENV: not NAME=value`                             | A line in `$PKGSMITH_ENV` has no `=`. Write one `NAME=value` for each line |
