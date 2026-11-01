# pkgsmith packages itself. pkgsmith builds nothing: you build your program, install it into a prefix, and this script
# describes the packages made from that prefix. On Linux, for example:
#
#   cargo build --release --workspace --locked
#   install -Dm755 target/release/pkgsmith prefix/usr/bin/pkgsmith
#   for f in target/release/libpkgsmith_plugin_*.so; do install -Dm755 "$f" prefix/usr/lib/pkgsmith/plugins/; done
#   install -Dm644 LICENSE prefix/usr/share/licenses/pkgsmith/LICENSE
#   install -Dm644 README.md prefix/usr/share/doc/pkgsmith/README.md
#   pkgsmith build --prefix prefix
#
# Every Linux format is written from the same prefix. The packages are written to ./dist. The binary links to glibc, so an
# apk (for Alpine, which uses musl) needs a prefix that was built on musl: pass -F to leave it out of a glibc build.
#
# macOS and Windows programs are built on those systems, so on them the script packs a prefix that holds the program in
# the folder /pkgsmith (see docs/macos-and-windows.md). On a Mac, the disk image:
#
#   mkdir -p prefix/pkgsmith/plugins
#   cp target/release/pkgsmith LICENSE README.md prefix/pkgsmith/
#   for f in target/release/libpkgsmith_plugin_*.dylib; do
#     cp "$f" "prefix/pkgsmith/plugins/pkgsmith-plugin-${f##*/libpkgsmith_plugin_}"
#   done
#   pkgsmith build --prefix prefix
#
# On Windows the same prefix gives the msi and the nsis installer (nsis needs makensis on PATH).

common = {
    "name": "pkgsmith",
    "version": arg("VERSION", "0.1.0"),
    "description": """Write distribution packages from one script
pkgsmith writes the rpm, deb, Arch, Alpine, OpenWrt, tar, zip and other packages of a program that you already built.""",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "homepage": "https://github.com/kierandrewett/pkgsmith",
    "license": "MIT",
}

os = host()["os"]

if os == "linux":
    expose(tree("/", "/"))

    # The binary links to the C library of the system that built it, so each package names the library of its own distro.
    libc = {"rpm": "glibc", "deb": "libc6", "archlinux": "glibc"}
    for fmt in ["rpm", "deb", "archlinux", "apk", "tar", "zip"]:
        package(fmt, depends = [libc[fmt]] if fmt in libc else [], **common)
else:
    expose(tree("/pkgsmith", "/pkgsmith"))

    if os == "macos":
        package("dmg", distro = "macos", options = {"volume_name": "pkgsmith"}, **common)
    elif os == "windows":
        package("msi", distro = "windows", options = {"prefix": "/pkgsmith", "manufacturer": "Kieran Drewett"}, **common)
        package("nsis", distro = "windows", options = {"prefix": "/pkgsmith"}, **common)
