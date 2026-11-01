# One build, several packages. The build runs once. The program package and the documentation package take different
# files from it and have different names. The documentation is the same on every architecture, so its arch is "all".
#
#   pkgsmith plan  -f examples/split.star
#   pkgsmith build -f examples/split.star
#
# `plan` shows the same BUILD letter for every package, so the build is shared.

sh("make all docs", name = "Build the program and the manual")

common = {
    "version": "3.0.0",
    "maintainer": "Example Maintainer <maintainer@example.com>",
    "license": "MIT",
}

# Two native formats when the system has one, and a tar everywhere. Each package lists its own files in `contents`.
formats = ["tar"]
if host()["format"] in ["deb", "rpm", "archlinux", "apk"]:
    formats.append(host()["format"])

for format in formats:
    package(
        format,
        name = "acme",
        description = "The acme program",
        contents = [file("build/acme", "/usr/bin/acme", mode = "0755")],
        **common
    )
    package(
        format,
        name = "acme-doc",
        description = "The manual of acme",
        arch = "all",
        depends = ["acme"],
        contents = [tree("build/html", "/usr/share/doc/acme")],
        **common
    )
