# pkgsmith: every function and field, with the values each one accepts.
#
# This file is valid. Run it through the planner with:
#   pkgsmith plan -f examples/full.star --build-arg STAGE=/tmp/stage
# The docs in docs/ describe each part.
#
# pkgsmith runs this script on this system, top to bottom, like makepkg. Only the code that the run reaches counts: every
# package() call that runs writes one package, and a branch of an `if` that is false here writes none.

# Reuse values from another script. A path is relative to this file. The loaded file may define values and functions.
load("include/common.star", "depends_for")

# ---------------------------------------------------------------------------------------------------------------------
# Inputs
# ---------------------------------------------------------------------------------------------------------------------

# arg(NAME, default) reads `--build-arg NAME=VALUE`, then the environment variable NAME, then the default.
#
# The version is MAJOR.MINOR.PATCH, or MAJOR.MINOR.PATCH-prerelease. pkgsmith spells it per format (docs/versions.md).
#   release:  0.4.2
#   nightly:  0.4.3-nightly.20261031.142.gabc1234   -> deb/rpm 0.4.3~nightly...  arch 0.4.3nightly20261031.142
VERSION = arg("VERSION", "0.4.2")
PREFIX = arg("PREFIX", "/usr")
STAGE = arg("STAGE")

# host() describes the system that runs the script. It returns a dict:
#   os      linux, macos or windows
#   arch    amd64 or arm64
#   distro  the ID of /etc/os-release (fedora, debian, ...), macos or windows
#   like    the ID_LIKE of /etc/os-release, as a list
#   version the VERSION_ID of /etc/os-release
#   libc    glibc, musl, or ""
#   format  the native package format (deb, rpm, archlinux, apk, dmg, msi), or ""
h = host()

# What every package shares. A dict with ** puts it in each package() call, and a call can add or replace a field.
common = {
    # Lowercase letters, digits and + - . _ ; starts with a letter or digit.
    "name": "acme-tool",
    "version": VERSION,
    # The first line is the summary. The rest is the long text. A blank line becomes a paragraph break.
    "description": """An example tool that shows every field
The long description starts here. It may have several paragraphs.

A blank line becomes a paragraph break in the package.
""",
    "maintainer": "Example Maintainer <maintainer@example.com>",
    "homepage": "https://example.com/acme-tool",
    "license": "Apache-2.0",  # an SPDX expression, e.g. MIT, GPL-3.0-or-later, "MIT OR Apache-2.0"
}

# ---------------------------------------------------------------------------------------------------------------------
# Packages with no build
# ---------------------------------------------------------------------------------------------------------------------

# A package() call with no sh() before it has no build. It packs `contents` only. Here the files of a program that was
# compiled on the target system are in STAGE (docs/macos-and-windows.md).
if STAGE:
    staged = [tree(STAGE, "/acme-tool")]
    package("dmg", distro = "macos", contents = staged, options = {"volume_name": "acme-tool"}, **common)
    package("msi", distro = "windows", contents = staged, options = {"prefix": "/acme-tool", "manufacturer": "Example"}, **common)
    package("nsis", distro = "windows", contents = staged, options = {"prefix": "/acme-tool"}, **common)

# ---------------------------------------------------------------------------------------------------------------------
# The build
# ---------------------------------------------------------------------------------------------------------------------

# sh(run, name, env) adds a command to the build. Each command runs in a fresh `sh -e`, in the folder of this script
# ($srcdir), in order. The files that a command writes there stay for the next command.
#   run:   the command, as text
#   name:  a label for the output
#   env:   variables for this command. Their values are literal text.
#
# $pkgdir is an empty folder. Anything that a command installs into it is also packed. A command can pass state to the
# commands after it by appending to two files, which pkgsmith reads as data:
#   echo "NAME=value" >> "$PKGSMITH_ENV"
#   echo "/opt/tool/bin" >> "$PKGSMITH_PATH"
#
# A command sees PKGSMITH_NAME, PKGSMITH_VERSION, PKGSMITH_RELEASE and PKGSMITH_ARCH.
sh("make PREFIX=" + PREFIX, name = "Build", env = {"CFLAGS": "-O2"})
sh("make install PREFIX={} DESTDIR=\"$pkgdir\"".format(PREFIX), name = "Install")

# ---------------------------------------------------------------------------------------------------------------------
# Files
# ---------------------------------------------------------------------------------------------------------------------

# expose(*entries) makes files the build produced part of what the script offers. Every package() call after it packs
# them. A source path is relative to the folder of this script.
expose(
    # file: one file. mode is octal text. owner and group default to root.
    file(
        "packaging/acme-tool.service",
        "/usr/lib/systemd/system/acme-tool.service",
        mode = "0644",
        owner = "root",
        group = "root",
    ),
    # file with a glob: a dst that ends in / is a folder.
    file("packaging/man/*.1", "/usr/share/man/man1/"),
    # config: the package manager keeps the file when the user changed it.
    config("packaging/acme-tool.conf", "/etc/acme-tool.conf", mode = "0640"),
    # config without a source: mark a file that the build installed.
    config(None, "/etc/acme-tool.d/defaults.conf"),
    # tree: a folder, copied recursively, including the folder and every folder below it.
    tree("packaging/share", "/usr/share/acme-tool"),
    # directory: an empty folder the package owns.
    directory("/var/lib/acme-tool", mode = "0750", owner = "acme", group = "acme"),
    # symlink: the first argument is the link target as it reads after install.
    symlink("/usr/lib/acme-tool/bin/acme-tool", "/usr/bin/acme-tool"),
    # ghost: owned but not carried. Only rpm records it.
    ghost("/var/log/acme-tool.log"),
)

# ---------------------------------------------------------------------------------------------------------------------
# Scripts of the package
# ---------------------------------------------------------------------------------------------------------------------

# A script is text, or a function that calls sh(). The commands that a function records become the package script. They
# run on the machine that installs the package, and they do not join the build.
def post_install():
    sh("systemctl daemon-reload || true")

scripts = {
    "pre_install": "getent passwd acme >/dev/null || useradd --system --no-create-home acme",
    "post_install": post_install,
    "pre_remove": "systemctl stop acme-tool || true",
    "post_remove": "systemctl daemon-reload || true",
}

# ---------------------------------------------------------------------------------------------------------------------
# Packages
# ---------------------------------------------------------------------------------------------------------------------

# package(format, **fields) packs the commands and the exposed files so far. `pkgsmith formats` lists the formats. Both
# the rpm and the deb below share one run of the build above. The fields:
#   name, version, description   required
#   release    the revision of one version. "1" by default. Reset to "1" when the version changes.
#   epoch      0 by default
#   arch       amd64 (rpm x86_64, deb amd64), arm64 (aarch64), or all (no compiled code: rpm noarch, deb all, arch any).
#              The default is the architecture of this system. A build can only make that or "all".
#   maintainer, homepage, license
#   depends, recommends, suggests, conflicts, replaces, provides   lists of names in the syntax of the format
#   contents   more files than the exposed ones. A list of file(), config(), tree(), ... entries.
#   options    a dict of the options of that format (docs/formats.md)
#   distro     names the lane, and what plugins adapt to. This system by default. `--where distro=fedora` selects it.
#   file       the output file name. {file} {name} {version} {release} {arch} {format} {distro}. Two packages with the
#              same format and distro need different names.
#   tags       extra values for `--where KEY=VALUE`
#   exclude    paths or globs inside $pkgdir that the package leaves out
#   scripts    a dict of pre_install, post_install, pre_remove and post_remove
if h["format"] == "rpm":
    package(
        "rpm",
        release = "1",
        epoch = 0,
        depends = ["glibc"],
        recommends = ["acme-tool-docs"],
        suggests = ["acme-tool-extras"],
        conflicts = ["acme-tool-legacy"],
        replaces = ["acme-tool-legacy"],
        provides = ["acme"],
        contents = [file("packaging/acme-tool.init", "/etc/init.d/acme-tool", mode = "0755")],
        exclude = ["/usr/share/doc/**", "/usr/lib/debug", "/usr/share/man/man3"],
        scripts = scripts,
        options = {"group": "Unspecified", "compression": "zstd"},  # zstd (default) | xz | gzip | bzip2
        tags = {"channel": "stable"},
        **common
    )
elif h["format"] == "deb":
    package(
        "deb",
        depends = depends_for("deb"),
        scripts = scripts,
        options = {"section": "utils", "priority": "optional", "compression": "xz"},  # xz | zstd | gzip
        **common
    )
elif h["format"] == "archlinux":
    package(
        "archlinux",
        depends = ["glibc"],
        scripts = scripts,
        options = {"packager": "Example Maintainer <maintainer@example.com>"},
        **common
    )
elif h["format"] == "apk":
    package("apk", depends = ["musl"], scripts = scripts, **common)

# These exist on every system. An ipk names the OpenWrt architecture in its options. The tar and the zip put everything
# below acme-tool-VERSION/, and `file` gives the zip its own name.
package(
    "ipk",
    distro = "openwrt",
    options = {"arch": "x86_64"},
    depends = ["libc"],
    **common
)
package("tar", distro = "any", options = {"compression": "xz", "top_folder": True}, **common)
package("zip", distro = "any", file = "{name}-{version}-{arch}.zip", options = {"top_folder": True}, **common)

# Code decides what exists. This package exists only when you pass `--build-arg NIGHTLY=1`.
if arg("NIGHTLY"):
    package(
        "tar",
        distro = "nightly",
        tags = {"channel": "nightly"},
        file = "{name}-{version}-nightly.tar.xz",
        **dict(common, version = VERSION + "-nightly.1")
    )
