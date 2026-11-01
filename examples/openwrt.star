# An OpenWrt package. Build the program with the OpenWrt SDK or toolchain for your router, install it into a prefix, and
# name the target in the `arch` option of ipk.
#
#   make CROSS_COMPILE=aarch64-openwrt-linux-musl- install DESTDIR=prefix PREFIX=/usr
#   install -Dm755 packaging/acme-router.init prefix/etc/init.d/acme-router
#   install -Dm644 packaging/acme-router.config prefix/etc/config/acme-router
#   pkgsmith build -f examples/openwrt.star
#
# The prefix holds arm64 files, so `arch` says so. The default is the architecture of this machine.

package(
    "ipk",
    distro = "openwrt",
    name = "acme-router",
    version = "0.2.0",
    description = "A router helper",
    maintainer = "Example Maintainer <maintainer@example.com>",
    arch = "arm64",
    depends = ["libc"],
    options = {"arch": "aarch64_cortex-a53"},
    # The files below /etc/config are settings: opkg keeps the changes of the user.
    config_files = ["/etc/config/*"],
    # The init script must run, whatever mode the build gave it. `contents` adds to the whole prefix and replaces the file
    # of the prefix with the same path.
    contents = [file("/etc/init.d/acme-router", "/etc/init.d/acme-router", mode = "0755")],
    scripts = {
        "post_install": """[ -n "$IPKG_INSTROOT" ] || /etc/init.d/acme-router enable
""",
        "pre_remove": """[ -n "$IPKG_INSTROOT" ] || /etc/init.d/acme-router disable
""",
    },
)
