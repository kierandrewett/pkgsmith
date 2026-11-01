# Command line

```sh
pkgsmith [--plugin-path DIR] COMMAND [OPTIONS] [PATH]
```

`PATH` is the build context: a folder that holds `pkgsmith.yaml`, or the manifest file itself. It defaults to `.`. The
folder is mounted at `/src` in a tracked build.

## Commands

| Command           | What it does                                                                    |
| ----------------- | ------------------------------------------------------------------------------- |
| `build`           | Builds every package the manifest describes and writes them to `dist`           |
| `plan` (or `ls`)  | Shows what `build` would do, without starting a container                       |
| `files`           | Builds, then lists the files each package would carry. Writes no package        |
| `formats`         | Lists the formats that the installed plugins write                              |
| `plugins`         | Lists the loaded plugins and the folders pkgsmith searches                      |
| `plugins install` | Downloads the plugins of this version from its GitHub release                   |
| `prune`           | Removes the cached setup images                                                 |

## Choosing what to build

`build`, `plan` and `files` take these options.

| Option                  | Values                                         | Default                  | Effect |
| ----------------------- | ---------------------------------------------- | ------------------------ | ------ |
| `-f`, `--file FILE`     | A path                                         | `PATH/pkgsmith.yaml`     | Reads this manifest |
| `-F`, `--format LIST`   | Names from `pkgsmith formats`, comma separated | Every format the matrix allows | Builds only these formats |
| `--job KEY=VALUE`       | Any matrix key and value. Repeatable           | All jobs                 | Builds only the jobs with this value |
| `--platform LIST`       | `linux/amd64`, `linux/arm64`, comma separated  | All architectures        | Builds only these architectures |
| `--distro NAME`         | `fedora`, `opensuse`, `debian`, `ubuntu`, `archlinux` | None              | Picks the `overrides` entry for jobs that name no distro |
| `--build-arg NAME=VALUE` | Text. Repeatable. Also `--set`                | None                     | Sets `${NAME}` in the manifest. It wins over the environment |

```sh
pkgsmith build --job distro=fedora --platform linux/arm64
pkgsmith build -F deb,rpm --build-arg VERSION=0.4.2
```

## Running the build

`build` and `files` also take these options.

| Option               | Values                          | Default     | Effect |
| -------------------- | ------------------------------- | ----------- | ------ |
| `-j`, `--jobs N`     | A number                        | CPUs, at most 8 | Runs this many builds at once. Jobs that share a build run it once |
| `--progress MODE`    | `auto`, `tty`, `plain`, `quiet` | `auto`      | `tty` redraws lanes in place. `plain` prints lines. `auto` picks `tty` on a terminal |
| `--work DIR`         | A path                          | `.pkgsmith` | Holds the files a tracked build installs |

`build` alone takes these.

| Option               | Values         | Default | Effect |
| -------------------- | -------------- | ------- | ------ |
| `-o`, `--output DIR` | A path         | `dist`  | Receives the packages |
| `-q`, `--quiet`      | A flag         | Off     | Prints only the path of each package |
| `--no-cache`         | A flag         | Off     | Runs the setup scripts again instead of reusing the cached images |

`build` signs packages when it gets a key. See [signing](signing.md) for the key types and how to check a signature.

| Option                             | Values                  | Default | Effect |
| ---------------------------------- | ----------------------- | ------- | ------ |
| `--sign-key [FORMAT=][SIGNER:]KEY` | A key. Repeatable       | None    | Signs with this key, for one format or for all it fits |
| `--sign-passphrase-file FILE`      | A path                  | None    | Holds the passphrase of the keys |
| `--sign-armor`                     | A flag                  | Off     | Writes `FILE.asc` instead of `FILE.sig` |

`plan` also takes `--json`, which prints the jobs as JSON for scripts.

## Container engine

A job with an `image` runs in a container, so pkgsmith needs the Podman API. A manifest with no `image` needs no engine.
pkgsmith looks for the API in this order:

1. `CONTAINER_HOST`, if it is set. It takes a `unix://` or `tcp://` address, on every system.
2. Linux: `$XDG_RUNTIME_DIR/podman/podman.sock`. If nothing answers there, pkgsmith starts `podman system service` for the
   length of the build.
3. macOS: the socket that `podman machine inspect` names. Run `podman machine init` once and `podman machine start`.
4. Windows: there is no default. Start Podman and set `CONTAINER_HOST` to a `tcp://` address that it serves.

Docker is not supported.

## Plugins

`--plugin-path DIR` adds a folder to the front of the plugin search path. Repeat it for more folders. It works with
every command. See [plugins](plugins.md) for the whole search path.

`pkgsmith plugins install` copies the plugins of a release into one folder.

| Option             | Values                 | Default                               | Effect |
| ------------------ | ---------------------- | ------------------------------------- | ------ |
| `--version VERSION` | A release number      | The version of this pkgsmith          | Picks the release |
| `--dir DIR`        | A path                 | The data folder of the user, see [plugins](plugins.md#where-pkgsmith-looks) | Receives the plugins |
| `--from FILE`      | A `pkgsmith-TARGET.tar.gz` | Download                          | Reads this archive instead |
| `--release-url URL` | A URL with `{version}` | The GitHub release of pkgsmith       | Where the release files are, for a mirror or a fork |

The command checks the archive against the `SHA256SUMS` file of the release and refuses it if they differ. It does not
need `sudo`: the default folder is on the plugin search path.
