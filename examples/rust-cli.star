# A Rust command line tool. `cargo install --root` puts the binary in the prefix. The manual page is installed by hand.
#
#   cargo install --path . --locked --root prefix/usr
#   install -Dm644 docs/ripfind.1 prefix/usr/share/man/man1/ripfind.1
#   pkgsmith build -f examples/rust-cli.star
#
# cargo also writes its list of installed crates in prefix/usr. A package does not need it, so `exclude` drops it.

expose(tree("/", "/"))

# glibc is on every Fedora, Debian and Arch system, but the dependency tells the package manager. The binary links to
# the C library of the system that built it, so a build on Alpine would name musl instead.
libc = {"rpm": "glibc", "deb": "libc6", "archlinux": "glibc"}

for format in ["rpm", "deb", "archlinux", "tar"]:
    package(
        format,
        name = "ripfind",
        version = arg("VERSION", "0.3.0"),
        description = """Find files fast
ripfind searches a folder tree for names and contents.""",
        maintainer = "Example Maintainer <maintainer@example.com>",
        license = "MIT",
        depends = [libc[format]] if format in libc else [],
        exclude = ["/usr/.crates.toml", "/usr/.crates2.json"],
    )
