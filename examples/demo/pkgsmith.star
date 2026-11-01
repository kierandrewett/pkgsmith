# The script of the demo. First install the program into a prefix, then build the packages from it:
#
#   make install DESTDIR=prefix
#   pkgsmith build
#
# pkgsmith does not build the program. The Makefile does, and `make install` puts hello and its files in ./prefix. This
# script describes the packages made from that folder, in every format, and pkgsmith writes them to ./dist.

expose(tree("/", "/"))

common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A demo that packages a small C program",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "license": "MIT",
    # The Makefile installs this file. The package manager keeps the changes that the user made to it.
    "config_files": ["/etc/hello.conf"],
}

for format in ["rpm", "deb", "archlinux", "apk", "tar"]:
    package(format, **common)
