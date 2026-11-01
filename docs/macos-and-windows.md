# macOS and Windows

pkgsmith packs a prefix, so a macOS or Windows package is made the same way as a Linux one. The difference is where the
prefix comes from: a macOS program is built on macOS, and a Windows program is built on Windows, with the tools of
those systems. pkgsmith has no cross-compiler, and builds nothing.

## What to do

1. **Build on the target system.** Compile your program with its own toolchain, as the release workflow of pkgsmith
   does. Install the result into a prefix folder.
2. **Pack the prefix.** Run pkgsmith on that machine, or copy the prefix to any other machine and run it there. The
   pure Rust formats (`dmg`, `msi`, `msix`, `appx`) need no other tool. `nsis` runs `makensis`, so it needs that program on the
   machine that writes the package.

A Linux program is a Linux program. Do not put one in a `dmg` or an `msi`: these formats pack the files you give them and
do not check the target.

## Example: a macOS disk image

Build on a Mac, and install into a prefix:

```console
$ cargo build --release
$ install -Dm755 target/release/hello prefix/hello/hello
```

The script describes the image:

```python
common = {"name": "hello", "version": "1.0.0", "description": "A small program"}

package("dmg", options = {"volume_name": "hello"}, **common)
```

```console
$ pkgsmith build -F dmg
```

The package is written to `./dist`. You should see a file such as `hello-1.0.0-1-arm64.dmg`. The `arch` of the package
is the architecture of the machine that runs pkgsmith. For files built for the other one, write `arch = "arm64"` or
`arch = "amd64"` in the call.

## Example: a Windows installer

Build on Windows, and install into a prefix. Then pack the same prefix as an installer and as a zip:

```python
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A small program",
    "maintainer": "Your Name <you@example.com>",
}

package("msi", options = {"prefix": "/"}, **common)
package("nsis", options = {"prefix": "/"}, **common)
package("zip", **common)
```

- `msi` needs a manufacturer, so give a `maintainer` or the option `manufacturer`.
- `msi` and `nsis` take a `prefix` option. Entry paths are taken relative to it, and a path outside it stops the build.
  The default is `/usr`, so set it to `/` when the prefix holds `hello/hello` and not `usr/hello`.

See [msi](formats.md#msi) and [nsis](formats.md#nsis).

## Several systems, one script

A repository that ships for all three systems runs pkgsmith on each, in CI. Each job builds there, makes a prefix, and
writes the packages of that system. One script serves all of them when you pick the formats with `-F`:

```console
$ pkgsmith build -F dmg          # on macOS
$ pkgsmith build -F msi,nsis     # on Windows
$ pkgsmith build -F rpm,deb,tar  # on Linux
```

The script lists every format once. A format that you do not select with `-F` is not written.

## Formats

| System  | Format | Notes                                                                                       |
| ------- | ------ | ------------------------------------------------------------------------------------------- |
| macOS   | `dmg`  | A disk image of your files. See [dmg](formats.md#dmg)                                       |
| Windows | `msi`  | A Windows Installer package. Written unsigned. See [msi](formats.md#msi)                    |
| Windows | `nsis` | An installer `.exe`. Needs `makensis` on the machine that writes it. See [nsis](formats.md#nsis) |
| Windows | `msix` | Written unsigned. Windows installs it only after you sign it. See [msix](formats.md#msix)   |
| Windows | `appx` | The same package as `msix` with the `.appx` extension and an older default Windows version. See [appx](formats.md#appx) |

The `dmg`, `msi`, `msix` and `appx` writers are pure Rust and run on any system. `nsis` runs `makensis`.
