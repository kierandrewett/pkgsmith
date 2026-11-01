# A Python program with no compiled code: `arch = "all"` gives a package that installs on every architecture.
#   pkgsmith build -f examples/python-app.star
build = job(
    runs_on = "debian:13",
    exclude = ["**/__pycache__", "**/*.pyc"],
    setup = [step(name = "Install the build tools", run = "apt-get update -qq && apt-get install -y python3-pip")],
    steps = [
        step(
            name = "Build",
            run = """
pip install --break-system-packages --no-deps --root=/ --prefix=/usr .
# pip writes caches and bytecode that the package does not need
""",
        ),
    ],
)

pkg = package(
    name = "notes-cli",
    version = "0.9.0",
    arch = "all",
    description = "A note taking tool",
    depends = ["python3"],
)

target(pkg, "deb", distro = "debian", job = build)
