# Release archives for people who unpack them themselves. `top_folder` puts everything below acme-tool-1.0.0/.
#
#   make install PREFIX=/opt/acme-tool DESTDIR=prefix
#   pkgsmith build -f examples/release-tarball.star

common = {
    "name": "acme-tool",
    "version": "1.0.0",
    "description": "A tool",
    "distro": "any",
}

package("tar", options = {"compression": "zstd", "top_folder": True}, **common)
package("zip", options = {"top_folder": True}, **common)
