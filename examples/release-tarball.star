# A release archive for people who unpack it themselves. `top_folder` puts everything below acme-tool-1.0.0/.
#   pkgsmith build -f examples/release-tarball.star -F tar
build = job(
    runs_on = "debian:13",
    setup = [step(name = "Install the build tools", run = "apt-get update -qq && apt-get install -y gcc make")],
    steps = [
        step(
            name = "Build",
            run = """
make
make install PREFIX=/opt/acme-tool
""",
        ),
    ],
)

pkg = package(name = "acme-tool", version = "1.0.0", description = "A tool")

target(pkg, "tar", distro = "any", job = build, options = {"compression": "zstd", "top_folder": True})
