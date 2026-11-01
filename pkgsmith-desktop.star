# pkgsmith packages itself for macOS and Windows. pkgsmith builds nothing: you compile the program on the target system,
# install it into a prefix, and this script describes the packages made from that prefix. The program and its files sit
# in the folder /pkgsmith of the prefix, which is what the installers install. See docs/macos-and-windows.md. The Linux
# packages come from pkgsmith.star. For example, on a Mac:
#
#   cargo build --release --workspace --locked
#   mkdir -p prefix/pkgsmith/plugins
#   cp target/release/pkgsmith LICENSE README.md prefix/pkgsmith/
#   for f in target/release/libpkgsmith_plugin_*.dylib; do
#     name="${f##*/libpkgsmith_plugin_}"; cp "$f" "prefix/pkgsmith/plugins/pkgsmith-plugin-$name"
#   done
#   pkgsmith build . -f pkgsmith-desktop.star --prefix prefix -F dmg
#
# On Windows, the same prefix gives the msi and the nsis installer (nsis needs makensis on PATH):
#
#   pkgsmith build . -f pkgsmith-desktop.star --prefix prefix -F msi,nsis

common = {
    "name": "pkgsmith",
    "version": arg("VERSION", "0.1.0"),
    "description": """Write distribution packages from one script
pkgsmith writes the rpm, deb, Arch, Alpine, OpenWrt, tar, zip and other packages of a program that you already built.""",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "homepage": "https://github.com/kierandrewett/pkgsmith",
    "license": "MIT",
}

expose(tree("/pkgsmith", "/pkgsmith"))

package("dmg", distro = "macos", options = {"volume_name": "pkgsmith"}, **common)
package("msi", distro = "windows", options = {"prefix": "/pkgsmith", "manufacturer": "Kieran Drewett"}, **common)
package("nsis", distro = "windows", options = {"prefix": "/pkgsmith"}, **common)
