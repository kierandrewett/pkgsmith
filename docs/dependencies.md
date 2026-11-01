# Dependencies

```python
pkg = package(
    name = "acme-tool",
    version = "1.0.0",
    description = "A tool",
    depends = ["glibc", "libssl3 >= 3.0"],
    recommends = ["acme-docs"],
    suggests = ["acme-extras"],
    conflicts = ["acme-legacy"],
    replaces = ["acme-legacy"],
    provides = ["acme"],
)

target(pkg, "rpm", distro = "fedora")
```

pkgsmith writes the names as you give them. It does not translate a name between distros. That is your job, in the script.

## Name and version syntax

Each format has its own way to write a version constraint. Write the constraint in the syntax of the format that reads
it.

| Format      | Example                  |
| ----------- | ------------------------ |
| `rpm`       | `openssl-libs >= 3.0`    |
| `deb`, `ipk`| `libssl3 (>= 3.0)`       |
| `archlinux` | `openssl>=3.0`           |
| `apk`       | `openssl>=3.0`           |

For `rpm` pkgsmith reads `name`, `name >= 1.2`, `name <= 1.2`, `name < 1.2`, `name > 1.2` and `name = 1.2` and writes the
rpm dependency flags itself. The other formats receive the text as written.

## Which key becomes what

| Key          | deb, ipk   | rpm         | archlinux   | apk                | tar, zip |
| ------------ | ---------- | ----------- | ----------- | ------------------ | -------- |
| `depends`    | Depends    | Requires    | depend      | depend             | none     |
| `recommends` | Recommends | Recommends  | optdepend   | not supported      | none     |
| `suggests`   | Suggests   | Suggests    | optdepend   | not supported      | none     |
| `conflicts`  | Conflicts  | Conflicts   | conflict    | `depend = !name`   | none     |
| `replaces`   | Replaces   | Obsoletes   | replaces    | replaces           | none     |
| `provides`   | Provides   | Provides    | provides    | provides           | none     |

Alpine has no weak dependencies, so `recommends` and `suggests` are left out of an `apk`.

## One name per distro

The library names differ between distros. Give each target its own list:

```python
pkg = package(name = "acme-tool", version = "1.0.0", description = "A tool", depends = ["glibc"])

target(pkg, "rpm", distro = "fedora")                              # uses the list of pkg
target(pkg, "archlinux", distro = "arch")
target(pkg, "deb", distro = "debian", depends = ["libc6"])
target(pkg, "apk", distro = "alpine", depends = ["musl"])
target(pkg, "rpm", distro = "opensuse", depends = ["glibc", "libopenssl3"], file = "opensuse-{file}")
```

A list on a target replaces the list of `pkg`. It does not merge with it. See the
[target reference](manifest.md#target). To keep the shared names and add one, write
`depends = pkg["depends"] + ["libopenssl3"]`.

## Libraries the program needs

pkgsmith does not look inside your binaries. It does not add the libraries a program links against. rpm and dpkg
normally do that for you, so a package built here can miss a dependency that a spec file would have found.

List the libraries yourself. To see what a program needs, run `readelf -d /usr/bin/prog | grep NEEDED` in the build
container and map each library to a package of that distro.
