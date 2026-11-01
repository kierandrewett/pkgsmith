# An OpenWrt package. The `arch` option of ipk names the OpenWrt target. The init script is only for ipk, so only the
# ipk target lists it.
#   pkgsmith build -f examples/openwrt.star -F ipk
build = job(
    runs_on = "debian:13",
    setup = [step(name = "Install the build tools", run = "apt-get update -qq && apt-get install -y gcc make")],
    steps = [
        step(
            name = "Build",
            run = """
make
make install PREFIX=/usr
""",
        ),
    ],
)

pkg = package(
    name = "acme-router",
    version = "0.2.0",
    description = "A router helper",
    maintainer = "Example Maintainer <maintainer@example.com>",
    depends = ["libc"],
    contents = [config(None, "/etc/config/acme-router")],
)

target(
    pkg, "ipk", distro = "openwrt", job = build,
    contents = pkg["contents"] + [file("packaging/acme-router.init", "/etc/init.d/acme-router", mode = "0755")],
    options = {"arch": "x86_64"},
)
