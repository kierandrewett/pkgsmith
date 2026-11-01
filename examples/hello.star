# The smallest script that builds something: one job with one setup step and one build step.
#   pkgsmith build -f examples/hello.star     (the folder of the script is the build context)
build = job(
    runs_on = "fedora:43",
    setup = [step(name = "Install the build tools", run = "dnf -y install gcc make")],
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

pkg = package(name = "hello", version = "1.0.0", description = "A small program")

target(pkg, "rpm", distro = "fedora", job = build)
