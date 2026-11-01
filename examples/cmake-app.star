# A C++ application built with CMake and Ninja. `exclude` leaves out headers and static libraries.
#   pkgsmith build -f examples/cmake-app.star -F deb
build = job(
    runs_on = "debian:13",
    exclude = ["/usr/include", "/usr/lib/*.a", "/usr/lib/cmake"],
    setup = [
        step(
            name = "Install the build tools",
            run = "apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y cmake ninja-build g++ libpng-dev",
        ),
    ],
    steps = [
        step(
            name = "Build",
            run = """
cmake -S . -B /tmp/build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build /tmp/build
cmake --install /tmp/build
""",
        ),
    ],
)

pkg = package(
    name = "acme-viewer",
    version = "2.1.0",
    description = "An image viewer",
    depends = ["libpng16-16t64"],
)

target(pkg, "deb", distro = "debian", job = build)
