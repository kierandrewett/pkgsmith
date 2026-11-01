# One script, every format, every architecture, from programs staged for each architecture. Plain loops make the
# packages. Nothing is built here, so one machine can write the packages of both architectures.
#
#   pkgsmith plan  -f examples/bingux-config.star --build-arg STAGE=/path/to/stage
#   pkgsmith build -f examples/bingux-config.star --build-arg STAGE=/path/to/stage --build-arg VERSION=0.1.0
#   pkgsmith build -f examples/bingux-config.star --build-arg STAGE=/path/to/stage -F deb --where arch=arm64

STAGE = arg("STAGE")
if not STAGE:
    fail("pass the staged folders: --build-arg STAGE=/path/to/stage")

common = {
    "name": "bingux-config",
    "version": arg("VERSION", "0.1.0"),
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "homepage": "https://github.com/kierandrewett/bingux",
    "license": "GPL-3.0-or-later",
    "description": """Configuration command for the Bingux desktop
bingux-config reads, changes and checks the Bingux configuration file in the user's configuration folder. The shell,
the search daemon and Settings use it, and it can be run by hand.""",
    "scripts": {"post_install": 'echo "bingux-config installed"'},
}

for arch in ["amd64", "arm64"]:
    # One staged folder per architecture: STAGE-amd64 and STAGE-arm64.
    staged = [file("{}-{}/usr/bin/bingux-config".format(STAGE, arch), "/usr/bin/bingux-config", mode = "0755")]
    for format in ["tar", "deb", "archlinux", "ipk", "rpm", "apk"]:
        package(format, arch = arch, contents = staged, **common)
