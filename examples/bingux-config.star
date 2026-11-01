# One script, every format, for either architecture. A prefix holds files of one architecture, so run the script once per
# architecture, each time on the prefix that holds those files. Nothing is built here, so one machine writes the packages
# of both.
#
#   install -Dm755 target/x86_64-unknown-linux-gnu/release/bingux-config prefix-amd64/usr/bin/bingux-config
#   install -Dm755 target/aarch64-unknown-linux-gnu/release/bingux-config prefix-arm64/usr/bin/bingux-config
#   pkgsmith build -f examples/bingux-config.star --prefix prefix-amd64 --build-arg ARCH=amd64
#   pkgsmith build -f examples/bingux-config.star --prefix prefix-arm64 --build-arg ARCH=arm64 -F deb
#
# The file name of each package holds its architecture, so both runs write to ./dist.

expose(file("/usr/bin/bingux-config", "/usr/bin/bingux-config", mode = "0755"))

common = {
    "name": "bingux-config",
    "version": arg("VERSION", "0.1.0"),
    "arch": arg("ARCH", host()["arch"]),
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "homepage": "https://github.com/kierandrewett/bingux",
    "license": "GPL-3.0-or-later",
    "description": """Configuration command for the Bingux desktop
bingux-config reads, changes and checks the Bingux configuration file in the user's configuration folder. The shell,
the search daemon and Settings use it, and it can be run by hand.""",
    "scripts": {"post_install": """echo "bingux-config installed"
"""},
}

for format in ["tar", "deb", "archlinux", "ipk", "rpm", "apk"]:
    package(format, **common)
