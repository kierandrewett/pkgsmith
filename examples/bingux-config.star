# One script, every format, every architecture. Plain loops declare the targets.
#
#   pkgsmith plan  -f examples/bingux-config.star --build-arg STAGE=/path/to/stage
#   pkgsmith build -f examples/bingux-config.star --build-arg STAGE=/path/to/stage --build-arg VERSION=0.1.0
#   pkgsmith build -f examples/bingux-config.star --build-arg STAGE=/path/to/stage -F deb --where arch=arm64
VERSION = arg("VERSION")
STAGE = arg("STAGE")

for arch in ["amd64", "arm64"]:
    pkg = package(
        name = "bingux-config",
        version = VERSION,
        arch = arch,
        maintainer = "Kieran Drewett <kieran@drewett.dev>",
        homepage = "https://github.com/kierandrewett/bingux",
        license = "GPL-3.0-or-later",
        description = """Configuration command for the Bingux desktop
bingux-config reads, changes and checks the Bingux configuration file in the
user's configuration folder. The shell, the search daemon and Settings use it,
and it can be run by hand.
""",
        # One staged prefix per architecture: STAGE-amd64, STAGE-arm64.
        contents = [file(f"{STAGE}-{arch}/usr/bin/bingux-config", "/usr/bin/bingux-config", mode = "0755")],
        scripts = {"post_install": "echo \"bingux-config installed\""},
    )
    for format in ["tar", "deb", "archlinux", "ipk", "rpm", "apk"]:
        target(pkg, format)
