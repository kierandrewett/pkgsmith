# The smallest script. pkgsmith runs it on this system and writes the native package of the system: an rpm on Fedora, a
# deb on Debian, a pacman package on Arch, an apk on Alpine.
#
#   pkgsmith build -f examples/hello.star
#
# The folder of the script is the build folder, so it needs a Makefile next to it (see examples/demo). The commands run
# in that folder. The Makefile installs below $pkgdir, an empty folder, and everything that ends up in $pkgdir is packed.

sh("make")
sh('make install PREFIX=/usr DESTDIR="$pkgdir"')

package(
    host()["format"],
    name = "hello",
    version = "1.0.0",
    description = "A small program",
)
