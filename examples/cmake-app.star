# A C++ application built with CMake and Ninja. `exclude` leaves out headers and static libraries.
# Install cmake, ninja, a C++ compiler and libpng first, then run:
#
#   pkgsmith build -f examples/cmake-app.star
#
# The libpng package has a different name on each system, so the script picks it from host().

depends = {
    "deb": ["libpng16-16t64"],
    "rpm": ["libpng"],
    "archlinux": ["libpng"],
    "apk": ["libpng"],
}.get(host()["format"], [])

sh(
    """
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build
DESTDIR="$pkgdir" cmake --install build
""",
    name = "Build and install",
)

package(
    host()["format"],
    name = "acme-viewer",
    version = "2.1.0",
    description = "An image viewer",
    depends = depends,
    exclude = ["/usr/include", "/usr/lib/*.a", "/usr/lib/cmake"],
)
