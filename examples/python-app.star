# A Python program with no compiled code: `arch = "all"` gives a package that installs on every architecture, and any
# system can build it.
#
#   pkgsmith build -f examples/python-app.star

sh('pip install --no-deps --root="$pkgdir" --prefix=/usr .', name = "Install")

package(
    host()["format"],
    name = "notes-cli",
    version = "0.9.0",
    description = "A note taking tool",
    arch = "all",
    depends = ["python3"],
    # pip writes caches and bytecode that the package does not need.
    exclude = ["**/__pycache__", "**/*.pyc"],
)
