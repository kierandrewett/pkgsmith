# The script that you build. It loads the common details from common.star (a path relative to this file). A keyword of
# package() sets a field, so this script changes the description and sets its own file name and options.
#
#   make install DESTDIR=prefix PREFIX=/usr
#   pkgsmith build examples/include
load("common.star", "common", "depends_for")

fields = dict(common, description = "A small program, as a release archive")

package(
    "tar",
    distro = "any",
    file = "{name}-{version}.tar.xz",
    options = {"compression": "xz"},
    depends = depends_for("tar"),
    **fields
)
package("deb", depends = depends_for("deb"), **common)
