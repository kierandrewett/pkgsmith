# A C++ application built with CMake and Ninja. `cmake --install --prefix` puts the installed files in the prefix, and
# `exclude` leaves out the headers and static libraries that only a developer needs.
#
#   cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
#   cmake --build build
#   cmake --install build --prefix prefix/usr
#   pkgsmith build -f examples/cmake-app.star
#
# The libpng package has a different name on each system, so each format names its own.

expose(tree("/", "/"))

libpng = {
    "rpm": ["libpng"],
    "deb": ["libpng16-16t64"],
    "archlinux": ["libpng"],
    "apk": ["libpng"],
}

for format in ["rpm", "deb", "archlinux", "apk"]:
    package(
        format,
        name = "acme-viewer",
        version = "2.1.0",
        description = "An image viewer",
        depends = libpng[format],
        exclude = ["/usr/include", "/usr/lib/*.a", "/usr/lib/cmake"],
    )
