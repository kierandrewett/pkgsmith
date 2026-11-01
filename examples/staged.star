# No build. You compile and stage the program yourself, and the script says where each file goes. A script with no sh()
# before a package() call has no build, so pkgsmith only packs the files you name. Nothing runs, so one machine writes
# packages of every format.
#
#   make install DESTDIR=/tmp/stage PREFIX=/usr
#   pkgsmith build -f examples/staged.star --build-arg STAGE=/tmp/stage --build-arg VERSION=0.1.0

STAGE = arg("STAGE")
if not STAGE:
    fail("pass the staged folder: --build-arg STAGE=/tmp/stage")

common = {
    "name": "bingux-config",
    "version": arg("VERSION", "0.1.0"),
    "description": """Configuration command for the Bingux desktop
Reads, changes and checks the Bingux configuration file.""",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "license": "GPL-3.0-or-later",
    "contents": [file(STAGE + "/usr/bin/bingux-config", "/usr/bin/bingux-config", mode = "0755")],
    "scripts": {"post_install": 'echo "bingux-config installed"'},
}

for format in ["tar", "deb", "rpm", "archlinux", "apk"]:
    package(format, **common)
