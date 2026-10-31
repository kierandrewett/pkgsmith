# Dependencies

```yaml
depends: [glibc, "libssl3 >= 3.0"]
recommends: [acme-docs]
suggests: [acme-extras]
conflicts: [acme-legacy]
replaces: [acme-legacy]
provides: [acme]
```

pkgsmith writes the names as you give them. It does not translate a name between distros. That is your job, with
`overrides`.

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

The library names differ between distros. Use `overrides`:

```yaml
depends: [glibc]          # fedora, opensuse, arch
overrides:
  deb:
    depends: [libc6]
  apk:
    depends: [musl]
  opensuse:
    depends: [glibc, libopenssl3]
```

A list in `overrides` replaces the base list. See [manifest reference](manifest.md#overrides).

## Libraries the program needs

pkgsmith does not look inside your binaries. It does not add the libraries a program links against. rpm and dpkg
normally do that for you, so a package built here can miss a dependency that a spec file would have found.

List the libraries yourself. To see what a program needs, run `readelf -d /usr/bin/prog | grep NEEDED` in the build
container and map each library to a package of that distro.
