# The smallest script. pkgsmith builds nothing: you install your program into a prefix, and this script describes the
# packages made from it. The prefix is a folder laid out like an installed system.
#
#   make install DESTDIR=prefix PREFIX=/usr        (see examples/demo for a C program with a Makefile)
#   pkgsmith build -f examples/hello.star
#
# Every package() call writes one package of the files in the prefix. The loop writes the same files in four formats.

for format in ["rpm", "deb", "archlinux", "tar"]:
    package(
        format,
        name = "hello",
        version = "1.0.0",
        description = "A small program",
    )
