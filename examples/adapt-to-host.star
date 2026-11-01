# A script that adapts to the system that built the prefix. host() describes the system that runs pkgsmith. Use it for
# the facts that depend on it: the architecture, the C library and the names of the libraries that the program links to.
# Never use it to choose which formats to write. Every format is written from the same prefix.
#
#   make install PREFIX=/usr DESTDIR=prefix
#   pkgsmith plan  -f examples/adapt-to-host.star
#   pkgsmith build -f examples/adapt-to-host.star
#
# host() returns a dict:
#   os      linux, macos or windows
#   arch    amd64 or arm64
#   distro  the ID of /etc/os-release (fedora, debian, ubuntu, arch, alpine), macos or windows
#   like    the ID_LIKE of /etc/os-release, as a list
#   version the VERSION_ID of /etc/os-release
#   libc    glibc, musl, or "" when it is not Linux
#   format  the native package format: deb, rpm, archlinux, apk, dmg on macOS, msi on Windows, or ""

h = host()

# The C library of the prefix. A program built on Alpine links to musl and cannot run on a glibc system.
libc = "musl" if h["libc"] == "musl" else "glibc"

# The same library has a different package name on each family of systems, and the C library changes some of them.
depends = {
    "glibc": {
        "rpm": ["glibc", "openssl-libs"],
        "deb": ["libc6", "libssl3"],
        "archlinux": ["glibc", "openssl"],
        "apk": ["gcompat", "libssl3"],
    },
    "musl": {
        "rpm": ["musl-libc", "openssl-libs"],
        "deb": ["musl", "libssl3"],
        "archlinux": ["musl", "openssl"],
        "apk": ["musl", "libssl3"],
    },
}[libc]

# Ubuntu 24.04 renamed the library for a change of its time_t. The prefix links to the one of the system that built it.
if h["distro"] == "ubuntu" and h["version"] >= "24.04":
    depends["deb"] = ["libc6", "libssl3t64"]

expose(tree("/", "/"))

for format in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
    package(
        format,
        name = "acme-tool",
        version = "1.0.0",
        description = "A tool built for " + h["distro"],
        # package() takes the architecture of this system when `arch` is not set. It is written here for the reader.
        arch = h["arch"],
        depends = depends.get(format, []),
    )
