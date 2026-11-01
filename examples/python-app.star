# A Python program with no compiled code. `arch = "all"` gives a package that installs on every architecture, so one
# prefix makes packages for all of them.
#
#   pip install --no-deps --prefix=prefix/usr .
#   pkgsmith build -f examples/python-app.star

expose(tree("/", "/"))

# The Python package has a different name on each system.
python = {
    "rpm": "python3",
    "deb": "python3",
    "archlinux": "python",
    "apk": "python3",
}

for format in ["rpm", "deb", "archlinux", "apk"]:
    package(
        format,
        name = "notes-cli",
        version = "0.9.0",
        description = "A note taking tool",
        arch = "all",
        depends = [python[format]],
        # pip writes caches and bytecode that the package does not need.
        exclude = ["**/__pycache__", "**/*.pyc"],
    )
