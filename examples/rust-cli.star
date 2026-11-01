# A Rust command line tool, built with cargo on this system. Run it on Fedora for an rpm and on Debian for a deb.
#
#   pkgsmith build -f examples/rust-cli.star
#
# The build installs nothing. expose() names the files that the build made, and where each one goes in the package.

sh("cargo build --release --locked", name = "Compile")

expose(
    file("target/release/ripfind", "/usr/bin/ripfind", mode = "0755"),
    file("docs/ripfind.1", "/usr/share/man/man1/ripfind.1"),
)

# glibc is on every Fedora and Debian system, but the dependency tells the package manager.
package(
    host()["format"],
    name = "ripfind",
    version = arg("VERSION", "0.3.0"),
    description = """Find files fast
ripfind searches a folder tree for names and contents.""",
    maintainer = "Example Maintainer <maintainer@example.com>",
    license = "MIT",
    depends = {"rpm": ["glibc"], "deb": ["libc6"]}.get(host()["format"], []),
)
