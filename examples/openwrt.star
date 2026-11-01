# An OpenWrt package. The `arch` option of ipk names the OpenWrt target.
#
#   pkgsmith build -f examples/openwrt.star
#
# A build runs on this system, so this makes an x86_64 package on an x86_64 machine. For another OpenWrt target, build
# on a machine of that architecture, or build the program elsewhere and pack it with a script that has no sh() (see
# staged.star).

sh("make")
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

expose(
    # The build installs this file. Marking it as config keeps the user's changes on upgrade.
    config(None, "/etc/config/acme-router"),
    # The init script is in the source tree. The build does not install it.
    file("packaging/acme-router.init", "/etc/init.d/acme-router", mode = "0755"),
)

package(
    "ipk",
    distro = "openwrt",
    name = "acme-router",
    version = "0.2.0",
    description = "A router helper",
    maintainer = "Example Maintainer <maintainer@example.com>",
    depends = ["libc"],
    options = {"arch": "x86_64"},
)
