# The prefix

pkgsmith builds nothing. You build your program with your own tools and install it into a **prefix**: a folder laid out
like an installed system. The script of pkgsmith describes the packages made from that folder.

```sh
make install PREFIX=/usr DESTDIR="$PWD/prefix"   # your build, your tools
pkgsmith build                                   # reads ./prefix, writes ./dist
```

A prefix looks like this:

```text
prefix/
  etc/hello.conf
  usr/bin/hello
  usr/share/doc/hello/README
```

`--prefix DIR` names the folder. The default is `prefix`, relative to the folder where you run pkgsmith. The prefix must
exist, or pkgsmith stops and says so.

## Make a prefix

Each build tool has a way to install into a staging folder. Use an absolute path for it: some tools change folder while
they install.

| Build tool | Command                                                                                              |
| ---------- | ---------------------------------------------------------------------------------------------------- |
| make       | `make install PREFIX=/usr DESTDIR="$PWD/prefix"`                                                     |
| CMake      | `cmake -B build -DCMAKE_INSTALL_PREFIX=/usr && cmake --build build && DESTDIR="$PWD/prefix" cmake --install build` |
| Meson      | `meson setup build --prefix=/usr && meson compile -C build && meson install -C build --destdir "$PWD/prefix"` |
| Cargo      | `cargo build --release --locked && install -Dm755 target/release/hello prefix/usr/bin/hello`        |
| Python     | `python -m pip install . --no-compile --prefix /usr --root "$PWD/prefix"`                            |
| Go         | `go build -o hello . && install -Dm755 hello prefix/usr/bin/hello`                                   |

Notes on the table:

- `cargo install --root prefix/usr` also works. It writes `.crates.toml` and `.crates2.json` into `prefix/usr`, so leave
  them out with `exclude("/usr/.crates*")` before the packages.
- A Python prefix holds the interpreter version in its path, such as `usr/lib/python3.13/site-packages`. A package of it
  is for that version only, so name the Python package it needs in `depends`.
- Start from an empty folder each time: `rm -rf prefix` before the build. pkgsmith packs what is there, including files
  that an earlier build left behind.

Then write the script. This one packs the whole prefix into six formats. A package holds the whole prefix unless the
script narrows it with `include(...)` and `exclude(...)`:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A small program",
}

for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(fmt, **common)
```

## Layout rules

- **Paths are installed paths.** `prefix/usr/bin/hello` becomes `/usr/bin/hello` in the package. Install with
  `PREFIX=/usr` so that programs find their own files there, not under the path of your staging folder.
- **The prefix is read-only.** pkgsmith and the script never change it, and never write into it.
- **Modes come from the disk.** A file keeps its mode, so install programs with mode `0755`. A `mode` in the script
  overrides it for one entry.
- **The owner is root.** Every file is owned by `root:root` in the package, whoever owns it in the prefix. Set `owner`
  and `group` on an entry to change that.
- **Symbolic links stay links.** A link in the prefix is packed as it reads. Make links relative, such as
  `hello -> hello-1.0`. An absolute link into the staging folder would point nowhere on the installed system.
  A `file` or `config` entry that reads through a link must stay inside the prefix or the script folder.
- **Folders come with their files.** The package holds the folders below the root with their files. The root `/` and
  system folders such as `/usr`, `/usr/bin` and `/etc` are not owned by the package, unless `directory(...)` asks.
- **Name files by their final place.** Put configuration under `/etc`, units under `/usr/lib/systemd/system`, and so on.
  Mark configuration files with `config_files`. See [contents](contents.md).

Use `pkgsmith files` to see the paths, modes and owners of every package before you write it.

## What a script may read

A script reads the prefix and the folder that holds the script. It cannot read, write or run anything else.

| Place                  | How to name it                              | Used for                                  |
| ---------------------- | ------------------------------------------- | ----------------------------------------- |
| The prefix             | `/usr/bin/hello` or `usr/bin/hello`         | The files of the packages, and queries    |
| The folder of the script | `here() + "/LICENSE"`                     | Files that are not part of the installed system, such as install scripts and licences |

- A path that leaves both places, by `..` or by a symbolic link, is an error. `exists` returns `False` for it.
- The script runs without a network and without processes. A build step in a script is not possible.
- The functions are `read_file`, `exists`, `is_dir`, `listdir` and `glob`. See the [script reference](manifest.md#reading-files).

A file that is not part of the installed system, such as an install script or a licence in the source tree, lives next
to the script. Name it with `here()`:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package("tar", contents = [file(here() + "/LICENSE", "/usr/share/licenses/hello/LICENSE")], **common)
```

The `contents` of a call add to the whole prefix.

## Other architectures and C libraries

The files in the prefix decide which system the package is for. pkgsmith does not look inside them, so you tell it.

- **Architecture.** `arch` defaults to the architecture of the system that runs pkgsmith. If the prefix holds arm64
  files, write `arch = "arm64"`. For a prefix with no compiled code, such as scripts and data, write `arch = "all"`.
  `amd64`, `arm64` and `all` are the values.
- **How to get an arm64 prefix.** Build on an arm64 machine, or cross-compile with your own toolchain and install into
  the prefix. pkgsmith can then run on any system.
- **C library.** A program links to the C library of the system that built it. A glibc build runs on distros with the
  same or a newer glibc, so build on the oldest one you support. It does not run on Alpine, which uses musl. For an `apk`
  of a compiled program, make a prefix on Alpine or with a musl toolchain, and write the `apk` from that prefix only.
- **Several prefixes.** Run pkgsmith once for each prefix, with `--prefix` and `--where` or `-F` to pick the formats:

```sh
pkgsmith build --prefix prefix-glibc -F rpm,deb,archlinux,tar
pkgsmith build --prefix prefix-musl -F apk
```

`host()["libc"]` and `host()["arch"]` tell the script about the system that runs it. They are for adapting dependency
names or the `arch` field, not for choosing formats. See [scripting](scripting.md#the-system-you-run-on).

macOS and Windows programs are built on those systems and packed from a prefix there. See
[macOS and Windows](macos-and-windows.md).

## Dependencies

pkgsmith does not look inside your binaries, so it does not add the libraries they link to. Name them in `depends`. To
see what a program needs, run `readelf -d prefix/usr/bin/hello | grep NEEDED` and map each library to a package of the
distro. See [dependencies](dependencies.md).

## Reproducibility

The same prefix, the same script and the same `SOURCE_DATE_EPOCH` give the same package. pkgsmith writes every time
stamp in a package from `SOURCE_DATE_EPOCH`, or from the current time when it is not set.

```sh
SOURCE_DATE_EPOCH=1767225600 pkgsmith build
```

What the prefix holds is up to your build. Many builds write the build time or the build path into a file. To make
the prefix the same each time, build in the same path, and use the settings of your tool for it, such as
`--no-compile` for pip.

## Troubleshooting

| Message                                       | Cause and fix                                                              |
| --------------------------------------------- | -------------------------------------------------------------------------- |
| `cannot read the prefix prefix`               | The folder does not exist. Make it with your build, or name it with `--prefix` |
| `declares no package`                         | No `package` call ran. Check your `if` conditions with `pkgsmith plan`     |
| `outside the prefix`                          | A path or a link leads out of the prefix and the script folder. Name a path inside one |
| `matches no files`                            | A glob in `src` found nothing. Check the prefix with `ls -R prefix`        |
| A package holds too much                      | Call `exclude(...)` or `include(...)` before the package. `pkgsmith files` shows the list |
| A file is missing from the package            | It is not in the prefix, or an `include`, an `exclude` or an `expose` left it out. Check each |
