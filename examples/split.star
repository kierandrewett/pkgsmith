# One prefix, two packages: the program and its documentation. The packages take different files from the same prefix and
# have different names. The documentation is the same on every architecture, so its arch is "all".
#
#   make install PREFIX=/usr DESTDIR=prefix
#   pkgsmith plan  -f examples/split.star
#   pkgsmith build -f examples/split.star

common = {
    "version": "3.0.0",
    "maintainer": "Example Maintainer <maintainer@example.com>",
    "license": "MIT",
}

# A package holds the whole prefix unless the script narrows it. exclude() and include() apply to every package declared
# after them, and reset() forgets them. acme is the prefix without the documentation.
exclude("/usr/share/doc", "/usr/share/man")

for format in ["rpm", "deb", "archlinux", "apk", "tar"]:
    package(
        format,
        name = "acme",
        description = "The acme program",
        **common
    )

# acme-doc starts from the whole prefix again and keeps only the documentation.
reset()
include("/usr/share/doc/**", "/usr/share/man/**")

for format in ["rpm", "deb", "archlinux", "apk", "tar"]:
    package(
        format,
        name = "acme-doc",
        description = "The manual of acme",
        arch = "all",
        depends = ["acme"],
        **common
    )
