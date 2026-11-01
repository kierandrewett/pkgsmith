# A release archive for people who unpack it themselves. `top_folder` puts everything below acme-tool-1.0.0/.
#
#   pkgsmith build -f examples/release-tarball.star

sh("make")
sh('make install PREFIX=/opt/acme-tool DESTDIR="$pkgdir"')

package(
    "tar",
    distro = "any",
    name = "acme-tool",
    version = "1.0.0",
    description = "A tool",
    options = {"compression": "zstd", "top_folder": True},
)
