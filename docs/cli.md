# Command line

```sh
pkgsmith [--plugin-path DIR] COMMAND [OPTIONS] [PATH]
```

`PATH` is the folder that holds `pkgsmith.star`, or the script itself. It defaults to `.`. The folder of the script is
the one that `here()` names in the script.

pkgsmith reads the files to package from the **prefix**, a folder that you made with your own build. See
[the prefix](prefix.md). pkgsmith builds nothing and runs no command.

## Commands

| Command           | What it does                                                            |
| ----------------- | ----------------------------------------------------------------------- |
| `build`           | Runs the script and writes every package to `dist`                      |
| `plan` (or `ls`)  | Shows each package and its file name, and writes nothing                |
| `files`           | Lists the files each package would carry. Writes no package             |
| `formats`         | Lists the formats that the installed plugins write                      |
| `plugins`         | Lists the loaded plugins and the folders pkgsmith searches              |
| `plugins install` | Downloads the plugins of this version from its GitHub release           |

## Choosing the input and the packages

`build`, `plan` and `files` take these options.

| Option                   | Values                                         | Default                        | Effect |
| ------------------------ | ---------------------------------------------- | ------------------------------ | ------ |
| `--prefix DIR`           | A folder                                       | `prefix`                       | The files to package. It must exist. The script reads it and never changes it |
| `-f`, `--file FILE`      | A path                                         | `PATH/pkgsmith.star`           | Reads this script |
| `-F`, `--format LIST`    | Names from `pkgsmith formats`, comma separated | Every format the script writes | Writes only these formats |
| `--where KEY=VALUE`      | `format`, `distro`, `arch`, `name` or a key of a package's `tags`. Repeatable | All packages | Writes only the packages whose value equals this. Every `--where` must match |
| `--build-arg NAME=VALUE` | Text. Repeatable. Also `--set`                 | None                           | Sets `arg("NAME")` in the script. It wins over the environment |

```sh
pkgsmith build --prefix out/prefix --where arch=amd64
pkgsmith build -F deb,rpm --build-arg VERSION=0.4.2
```

A selection that matches no package is an error.

## Writing the packages

`build` and `files` also take these options.

| Option               | Values                          | Default         | Effect |
| -------------------- | ------------------------------- | --------------- | ------ |
| `-j`, `--jobs N`     | A number                        | CPUs, at most 8 | Writes this many packages at once |
| `--progress MODE`    | `auto`, `tty`, `plain`, `quiet` | `auto`          | `tty` redraws lines in place. `plain` prints lines. `auto` picks `tty` on a terminal |

`build` alone takes these.

| Option               | Values         | Default | Effect |
| -------------------- | -------------- | ------- | ------ |
| `-o`, `--output DIR` | A path         | `dist`  | Receives the packages |
| `-q`, `--quiet`      | A flag         | Off     | Prints only the path of each package |

`build` signs packages when it gets a key. See [signing](signing.md) for the key types and how to check a signature.

| Option                             | Values            | Default | Effect |
| ---------------------------------- | ----------------- | ------- | ------ |
| `--sign-key [FORMAT=][SIGNER:]KEY` | A key. Repeatable | None    | Signs with this key, for one format or for all it fits |
| `--sign-passphrase-file FILE`      | A path            | None    | Holds the passphrase of the keys |
| `--sign-armor`                     | A flag            | Off     | Writes `FILE.asc` instead of `FILE.sig` |

`plan` also takes `--json`, which prints the packages as JSON for scripts. Each row has `format`, `distro`, `arch`,
`file`, `error` and `values`.

## Other systems

pkgsmith has no option to write for another distro or architecture, because it needs none: a package is a file that a
plugin writes, on any system. What depends on the system is the content of the prefix. To package arm64 files, make an
arm64 prefix and write `arch = "arm64"` in the script. See
[the prefix](prefix.md#other-architectures-and-c-libraries).

## Plugins

`--plugin-path DIR` adds a folder to the front of the plugin search path. Repeat it for more folders. It works with
every command. See [plugins](plugins.md) for the whole search path.

`pkgsmith plugins install` copies the plugins of a release into one folder.

| Option              | Values                     | Default                      | Effect |
| ------------------- | -------------------------- | ---------------------------- | ------ |
| `--version VERSION` | A release number           | The version of this pkgsmith | Picks the release |
| `--dir DIR`         | A path                     | The data folder of the user, see [plugins](plugins.md#where-pkgsmith-looks) | Receives the plugins |
| `--from FILE`       | A `pkgsmith-TARGET.tar.gz` | Download                     | Reads this archive instead |
| `--release-url URL` | A URL with `{version}`     | The GitHub release of pkgsmith | Where the release files are, for a mirror or a fork |

The command checks the archive against the `SHA256SUMS` file of the release and refuses it if they differ. It does not
need `sudo`: the default folder is on the plugin search path.
