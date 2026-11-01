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

# expose() adds files to what every package() call after it packs. Only the program is exposed, so acme holds the
# program and nothing else.
expose(
    file("/usr/bin/acme", "/usr/bin/acme", mode = "0755"),
    tree("/usr/lib/acme", "/usr/lib/acme"),
)

# `contents` adds files to one package. The documentation package gets the program too, because the program is exposed,
# so `include` keeps only the documentation paths.
docs = [
    tree("/usr/share/doc/acme", "/usr/share/doc/acme"),
    file("/usr/share/man/man1/*.1", "/usr/share/man/man1/"),
]

for format in ["rpm", "deb", "archlinux", "apk", "tar"]:
    package(
        format,
        name = "acme",
        description = "The acme program",
        **common
    )
    package(
        format,
        name = "acme-doc",
        description = "The manual of acme",
        arch = "all",
        depends = ["acme"],
        contents = docs,
        include = ["/usr/share/doc/**", "/usr/share/man/**"],
        **common
    )
