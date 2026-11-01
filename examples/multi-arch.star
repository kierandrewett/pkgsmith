# One script, two architectures. `--platform linux/arm64` builds only the arm64 package. The arm64 container needs
# qemu-user binfmt on an x86_64 host (for example the qemu-user-static package).
#   pkgsmith plan  -f examples/multi-arch.star
#   pkgsmith build -f examples/multi-arch.star --platform linux/arm64
triples = {"amd64": "x86_64-linux-gnu", "arm64": "aarch64-linux-gnu"}

for arch, triple in triples.items():
    # The job differs by architecture, so each architecture has its own run. $TRIPLE is a variable of the steps.
    build = job(
        runs_on = "debian:13",
        env = {"TRIPLE": triple},
        setup = [step(name = "Install the build tools", run = "apt-get update -qq && apt-get install -y gcc make")],
        steps = [
            step(
                name = "Build",
                run = """
echo "building for $PKGSMITH_ARCH ($TRIPLE)"
make
make install PREFIX=/usr
""",
            ),
        ],
    )
    pkg = package(name = "acme-tool", version = "1.0.0", arch = arch, description = "A tool for two architectures")
    target(pkg, "deb", distro = "debian", job = build)
