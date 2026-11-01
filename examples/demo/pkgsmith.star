# The script of the demo. The build runs on this system, in this folder, and installs below $pkgdir. The native package
# of the system is written: rpm on Fedora, deb on Debian, and so on. The tar is the same files for any system.
#
#   pkgsmith build

sh("make && make install PREFIX=/usr DESTDIR=\"$pkgdir\"")

common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A demo that packages a small C program",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "license": "MIT",
}

package("tar", **common)

format = host()["format"]
if format in ["deb", "rpm", "archlinux", "apk"]:
    package(format, **common)
