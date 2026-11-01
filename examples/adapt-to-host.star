# A script that adapts to the system it runs on. host() describes that system, so the dependencies, the build and the
# formats can differ without a second script.
#
#   pkgsmith plan  -f examples/adapt-to-host.star
#   pkgsmith build -f examples/adapt-to-host.star
#
# host() returns a dict:
#   os      linux, macos or windows
#   arch    amd64 or arm64
#   distro  the ID of /etc/os-release (fedora, debian, ubuntu, arch, alpine), macos or windows
#   like    the ID_LIKE of /etc/os-release, as a list
#   version the VERSION_ID of /etc/os-release
#   libc    glibc, musl, or "" when unknown
#   format  the native package format: deb, rpm, archlinux, apk, dmg on macOS, msi on Windows, or ""

h = host()

# The same library has a different package name on each family of systems.
depends = {
    "rpm": ["glibc", "openssl-libs"],
    "deb": ["libc6", "libssl3"],
    "archlinux": ["glibc", "openssl"],
    "apk": ["musl", "libssl3"],
}.get(h["format"], [])

# Alpine links against musl, so it links the C library differently.
env = {"LDFLAGS": "-static-pie"} if h["libc"] == "musl" else {}

sh("make", name = "Build for " + h["distro"], env = env)
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

common = {
    "name": "acme-tool",
    "version": "1.0.0",
    "description": "A tool that adapts to the system that builds it",
}

# A tar of the same files works anywhere.
package("tar", distro = "any", **common)

# The native package exists when pkgsmith knows the format of the system. Elsewhere only the tar remains.
if h["format"] in ["deb", "rpm", "archlinux", "apk"]:
    package(h["format"], depends = depends, **common)
