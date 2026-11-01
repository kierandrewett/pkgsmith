# No container. You build and stage the program yourself, and the script says where each file goes.
#   make install DESTDIR=/tmp/stage PREFIX=/usr
#   pkgsmith build -f examples/staged.star --build-arg STAGE=/tmp/stage --build-arg VERSION=0.1.0
VERSION = arg("VERSION")
STAGE = arg("STAGE")

pkg = package(
    name = "bingux-config",
    version = VERSION,
    description = """Configuration command for the Bingux desktop
Reads, changes and checks the Bingux configuration file.
""",
    maintainer = "Kieran Drewett <kieran@drewett.dev>",
    license = "GPL-3.0-or-later",
    contents = [file(STAGE + "/usr/bin/bingux-config", "/usr/bin/bingux-config", mode = "0755")],
    scripts = {"post_install": "echo \"bingux-config installed\""},
)

for format in ["tar", "deb", "rpm", "archlinux", "apk"]:
    target(pkg, format)
