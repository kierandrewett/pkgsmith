# pkgsmith: every function and field, with the values each one accepts.
#
# This file is valid. Make a prefix with the files it names, then plan it:
#
#   install -Dm755 acme-tool prefix/usr/bin/acme-tool
#   install -Dm644 acme-tool.1 prefix/usr/share/man/man1/acme-tool.1
#   install -Dm640 acme-tool.conf prefix/etc/acme-tool.conf
#   install -Dm644 acme-tool.service prefix/usr/lib/systemd/system/acme-tool.service
#   pkgsmith plan -f examples/full.star
#
# pkgsmith builds nothing. It runs this script on this system, top to bottom. The script cannot run a command, use the
# network or change a file. Every package() call that the run reaches writes one package. A branch of an `if` that is
# false writes none.

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
#
# prefix() is the folder that holds the files, and here() is the folder of this script, both as text. read_file(path),
# exists(path), is_dir(path), listdir(path) and glob(pattern) read them and nothing else. A relative path and a path that
# starts with / both name a file of the prefix. A path that starts with here() names a file next to the script.
if exists("/usr/share/acme-tool/VERSION"):
    VERSION = arg("VERSION", read_file("/usr/share/acme-tool/VERSION").strip())
else:
    VERSION = arg("VERSION", "0.4.2")
CHANNEL = arg("CHANNEL", "stable")
NIGHTLY = arg("NIGHTLY")

# host() describes the system that runs the script. It returns a dict:
#   os      linux, macos or windows
#   arch    amd64 or arm64
#   distro  the ID of /etc/os-release (fedora, debian, ...), macos or windows
#   like    the ID_LIKE of /etc/os-release, as a list
#   version the VERSION_ID of /etc/os-release
#   libc    glibc, musl, or ""
#   format  the native package format (deb, rpm, archlinux, apk, dmg, msi), or ""
# Use it for the architecture and for names that depend on the system. Do not use it to choose which formats to write.
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
# Files
# ---------------------------------------------------------------------------------------------------------------------

# Without expose(), a package holds the whole prefix, and include() and exclude() narrow it (see Packages below).
# expose(*entries) is for a prefix of an odd layout, or when only the files that you list are offered. Every package()
# call after it packs them. A source path is a path in the prefix. A source that starts with here() is a file next to
# this script.
expose(
    # file: one file. mode is octal text. owner and group default to root.
    file(
        "/usr/bin/acme-tool",
        "/usr/bin/acme-tool",
        mode = "0755",
        owner = "root",
        group = "root",
    ),
    # file with a glob: a dst that ends in / is a folder.
    file("/usr/share/man/man1/*.1", "/usr/share/man/man1/"),
    # config: the package manager keeps the file when the user changed it.
    config("/etc/acme-tool.conf", "/etc/acme-tool.conf", mode = "0640"),
    # tree: a folder, copied recursively, including the folder and every folder below it. tree("/", "/") is the whole prefix.
    tree("/usr/share/acme-tool", "/usr/share/acme-tool"),
    # directory: an empty folder the package owns.
    directory("/var/lib/acme-tool", mode = "0750", owner = "acme", group = "acme"),
    # symlink: the first argument is the link target as it reads after install.
    symlink("/usr/lib/acme-tool/bin/acme-tool", "/usr/lib/acme-tool/current"),
    # ghost: owned but not carried. Only rpm records it.
    ghost("/var/log/acme-tool.log"),
)

# A list of entries is accepted too, so code can make them. glob() gives the paths of the prefix that match, sorted.
expose([file(path, path) for path in glob("/usr/lib/systemd/system/*.service")])

# A file next to the script, such as a licence, is read from the folder that here() names.
if exists(here() + "/LICENSE"):
    expose(file(here() + "/LICENSE", "/usr/share/licenses/acme-tool/LICENSE"))

# The plugins that the build installed. listdir(path) gives sorted names, and is_dir(path) tells folders from files.
plugins = [name for name in listdir("/usr/lib/acme-tool") if is_dir("/usr/lib/acme-tool/" + name)] if is_dir("/usr/lib/acme-tool") else []

# ---------------------------------------------------------------------------------------------------------------------
# Scripts of the package
# ---------------------------------------------------------------------------------------------------------------------

# A script is text, or the name of a file next to this script. A value of one line names a file, so write a script of
# one command with a line break after it. A script without a #! line gets `#!/bin/sh` and `set -e`. The scripts run on
# the machine that installs or removes the package.
scripts = {
    "pre_install": """getent passwd acme >/dev/null || useradd --system --no-create-home acme
""",
    "post_install": """systemctl daemon-reload || true
""",
    "pre_remove": """systemctl stop acme-tool || true
""",
    "post_remove": """systemctl daemon-reload || true
""",
}

# ---------------------------------------------------------------------------------------------------------------------
# Packages
# ---------------------------------------------------------------------------------------------------------------------

# include(*globs) and exclude(*globs) narrow the packages declared after the call. A glob matches the paths of the
# package, such as /usr/bin/** or /etc/acme-tool.conf. include keeps only the matches and exclude removes them. reset()
# clears what expose, include and exclude collected, so the next package starts again from the whole prefix. This script
# leaves out the debug files of every package:
exclude("/usr/lib/debug/**")

# package(format, **fields) writes one package from the files exposed so far and `contents`. `pkgsmith formats` lists the
# formats. The fields:
#   name, version, description   required
#   release    the revision of one version. "1" by default. Reset to "1" when the version changes.
#   epoch      0 by default
#   arch       amd64 (rpm x86_64, deb amd64), arm64 (aarch64), or all (no compiled code: rpm noarch, deb all, arch any).
#              The default is the architecture of this system. Set it to the architecture of the files of the prefix.
#   maintainer, homepage, license
#   depends, recommends, suggests, conflicts, replaces, provides   lists of names in the syntax of the format
#   contents   more files than the exposed ones. A list of file(), config(), tree(), ... entries. Later entries replace
#              earlier ones with the same path.
#   options    a dict of the options of that format (docs/formats.md)
#   distro     names the lane, and what plugins adapt to. `--where distro=fedora` selects it.
#   file       the output file name. {file} {name} {version} {release} {arch} {format} {distro}. Two packages with the
#              same format and distro need different names.
#   tags       extra values for `--where KEY=VALUE`
#   exclude    paths or globs of the package that it leaves out. They add to the exclude() calls.
#   include    paths or globs of the package that it keeps. Everything else is left out. They add to the include() calls.
#   config_files   paths or globs of files that the package manager keeps when the user changed them
#   scripts    a dict of pre_install, post_install, pre_remove and post_remove
package(
    "rpm",
    release = "1",
    epoch = 0,
    arch = h["arch"],
    depends = ["glibc"],
    recommends = ["acme-tool-docs"],
    suggests = ["acme-tool-extras"],
    conflicts = ["acme-tool-legacy"],
    replaces = ["acme-tool-legacy"],
    provides = ["acme"],
    contents = [file("/usr/share/acme-tool/acme-tool.init", "/etc/init.d/acme-tool", mode = "0755")],
    exclude = ["/usr/share/doc/**", "/usr/share/man/man3"],
    config_files = ["/etc/acme-tool.d/*.conf"],
    scripts = scripts,
    options = {"group": "Unspecified", "compression": "zstd"},  # zstd (default) | xz | gzip | bzip2
    distro = "fedora",
    tags = {"channel": CHANNEL, "prefix": prefix()},
    **common
)
package(
    "deb",
    depends = depends_for("deb"),
    scripts = scripts,
    options = {"section": "utils", "priority": "optional", "compression": "xz"},  # xz | zstd | gzip
    distro = "debian",
    **common
)
package(
    "archlinux",
    depends = ["glibc"],
    scripts = scripts,
    options = {"packager": "Example Maintainer <maintainer@example.com>"},
    **common
)
package("apk", depends = ["musl"], scripts = scripts, **common)

# An ipk names the OpenWrt architecture in its options. The tar and the zip put everything below acme-tool-VERSION/ with
# `top_folder`, and `file` gives the zip its own name.
package(
    "ipk",
    distro = "openwrt",
    options = {"arch": "x86_64"},
    depends = ["libc"],
    **common
)
package("tar", distro = "any", options = {"compression": "xz", "top_folder": True}, **common)
package("zip", distro = "any", file = "{name}-{version}-{arch}.zip", options = {"top_folder": True}, **common)

# include keeps only the paths that match, so this archive holds the programs and nothing else.
package(
    "tar",
    distro = "programs",
    file = "{name}-{version}-programs.tar.gz",
    include = ["/usr/bin/**", "/usr/lib/acme-tool/**"],
    options = {"compression": "gzip"},
    **common
)

# Code decides what exists. This package exists only when you pass `--build-arg NIGHTLY=1`.
if NIGHTLY:
    package(
        "tar",
        distro = "nightly",
        tags = {"channel": "nightly", "plugins": ",".join(plugins)},
        file = "{name}-{version}-nightly.tar.xz",
        **dict(common, version = VERSION + "-nightly.1")
    )
