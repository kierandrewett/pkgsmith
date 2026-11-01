# pkgsmith: every field and helper, with the values each one accepts.
#
# This file is valid. Run it through the planner with:
#   pkgsmith plan -f examples/full.star
# Comments that show a field show the other values you can choose. The docs in docs/ describe each part.

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

# ---------------------------------------------------------------------------------------------------------------------
# Jobs
# ---------------------------------------------------------------------------------------------------------------------

# A job builds the files, in a container image. Targets whose jobs are equal share one run of it.
#
# A step runs in a fresh `sh -e` with the build context at /src. A throw-away overlay covers /src, so the build may write
# there. Everything the `steps` install outside /src becomes the package. The `setup` steps run once and are cached as an
# image, and what they install is not packaged.
#   runs_on:   the container image
#   setup:     steps that prepare the image
#   steps:     the build
#   env:       variables for every step. Their values are literal text.
#   exclude:   paths the package leaves out: path prefixes or globs. Defaults are listed in docs/contents.md.
#   arch:      the architecture the job runs for. The default is the architecture of the target.
#
# A job cannot read the format or the distro of a target. A step can change what later steps see by appending to two files:
#   echo "NAME=value" >> "$PKGSMITH_ENV"
#   echo "/opt/tool/bin" >> "$PKGSMITH_PATH"
#
# A step sees PKGSMITH_NAME, PKGSMITH_VERSION, PKGSMITH_RELEASE and PKGSMITH_ARCH.

# A function builds the jobs, because the distros differ only in their image and their install command.
def make_job(image, install, exclude = []):
    return job(
        runs_on = image,
        exclude = exclude,
        setup = [step(name = "Install the build tools", run = install)],
        steps = [
            step(
                name = "Build",
                run = f"""
make
make install PREFIX={PREFIX}
""",
            ),
        ],
    )

fedora = make_job(
    "fedora:43",
    "dnf -y install gcc make",
    exclude = [
        "/usr/share/doc/**",  # a glob
        "/usr/lib/debug",  # a prefix
        "/usr/share/man",
    ],
)
opensuse = make_job("registry.opensuse.org/opensuse/tumbleweed", "zypper -n install gcc make")
debian = make_job("debian:13", "apt-get update -qq && apt-get install -y gcc make")
ubuntu = make_job("ubuntu:24.04", "apt-get update -qq && apt-get install -y gcc make")
arch = make_job("archlinux:base", "pacman -Sy --noconfirm gcc make")
alpine = make_job("alpine:3.22", "apk add --no-cache gcc make musl-dev")

# ---------------------------------------------------------------------------------------------------------------------
# The package: what every target shares (docs/manifest.md)
# ---------------------------------------------------------------------------------------------------------------------

pkg = package(
    # Lowercase letters, digits and + - . _ ; starts with a letter or digit.
    name = "acme-tool",
    version = VERSION,
    # The first line is the summary. The rest is the long text. A blank line becomes a paragraph break in the package.
    description = """An example tool that shows every field
The long description starts here. It may have several paragraphs.

A blank line becomes a paragraph break in the package.
""",
    # Package revision of one version. Reset to "1" when the version changes.
    release = "1",
    epoch = 0,
    # Architecture of the package.
    #   amd64  64-bit x86       (rpm x86_64, arch x86_64, apk x86_64, deb amd64)
    #   arm64  64-bit Arm       (rpm aarch64, arch aarch64, apk aarch64, deb arm64)
    #   all    no compiled code (rpm noarch, arch any, apk noarch, deb all)
    arch = "amd64",
    maintainer = "Example Maintainer <maintainer@example.com>",
    homepage = "https://example.com/acme-tool",
    license = "Apache-2.0",  # an SPDX expression, e.g. MIT, GPL-3.0-or-later, "MIT OR Apache-2.0"
    # Relations: names in the syntax of the target format (docs/dependencies.md). A target may replace them.
    depends = ["libc.so.6"],
    recommends = ["acme-tool-docs"],
    suggests = ["acme-tool-extras"],
    conflicts = ["acme-tool-legacy"],
    replaces = ["acme-tool-legacy"],
    provides = ["acme"],
    # Contents: add files, or change tracked ones (docs/contents.md). Each helper returns one entry.
    contents = [
        # file: one file. mode is octal text. owner and group default to root.
        file(
            "packaging/acme-tool.service",
            "/usr/lib/systemd/system/acme-tool.service",
            mode = "0644",
            owner = "root",
            group = "root",
        ),
        # file with a glob: dst is a folder.
        file("packaging/man/*.1", "/usr/share/man/man1"),
        # config: the package manager keeps the file when the user changed it.
        config("packaging/acme-tool.conf", "/etc/acme-tool.conf", mode = "0640"),
        # config without a source: change a file the build installed and keep its content.
        config(None, "/etc/acme-tool.d/defaults.conf"),
        # tree: a folder, copied recursively, including the folder and every folder below it.
        tree("packaging/share", "/usr/share/acme-tool"),
        # directory: an empty folder the package owns.
        directory("/var/lib/acme-tool", mode = "0750", owner = "acme", group = "acme"),
        # symlink: the first argument is the link target as it reads after install.
        symlink("/usr/lib/acme-tool/bin/acme-tool", "/usr/bin/acme-tool"),
        # ghost: owned but not carried. Only rpm records it.
        ghost("/var/log/acme-tool.log"),
    ],
    # Scripts: text, or the name of a file next to this script (docs/scripts.md). One line is a file name.
    scripts = {
        "pre_install": """
getent passwd acme >/dev/null || useradd --system --no-create-home acme
""",
        "post_install": """
systemctl daemon-reload || true
""",
        "pre_remove": """
systemctl stop acme-tool || true
""",
        "post_remove": "packaging/post-remove.sh",  # a one-line value is a file name
    },
)

# ---------------------------------------------------------------------------------------------------------------------
# Targets (docs/matrix-and-expressions.md)
# ---------------------------------------------------------------------------------------------------------------------

# target(pkg, format, ...) declares one package.
#   format:  rpm, deb, archlinux, apk, ipk, tar, zip, and the formats of other plugins
#   distro:  any text. `--where distro=fedora` selects it. Two distros with the same format need different `file` names.
#   job:     the job whose files the target packs. Without a job the target packs `contents` only.
#   file:    the output file name. `{file}` is the name the format picked.
#   tags:    extra values for `--where KEY=VALUE`
#   Any other keyword replaces the field of the package with that name, so a target may change depends, contents,
#   scripts, arch or options.
#
# options is a dict of format options (docs/formats/). Each target has its own.

# Python-like code makes the combinations: this loop makes 2 architectures x 7 packages, minus the Alpine arm64 build.
for arch_name in ["amd64", "arm64"]:
    target(
        pkg, "rpm", distro = "fedora", job = fedora, arch = arch_name, depends = ["glibc"],
        options = {"group": "Unspecified", "compression": "zstd"},  # zstd (default) | xz | gzip
    )
    target(
        pkg, "rpm", distro = "opensuse", job = opensuse, arch = arch_name, file = "opensuse-{file}",
        depends = ["glibc"],
        scripts = dict(pkg["scripts"], post_install = "echo \"installed on openSUSE\""),
    )
    for format in ["deb", "tar"]:
        target(
            pkg, format, distro = "debian", job = debian, arch = arch_name,
            depends = ["libc6"] if format == "deb" else pkg["depends"],
            options = (
                {"section": "utils", "priority": "optional", "compression": "xz"}  # deb: xz | zstd | gzip
                if format == "deb"
                else {"compression": "xz", "top_folder": True}  # tar: xz | gzip | zstd | none
            ),
        )
    target(
        pkg, "deb", distro = "ubuntu", job = ubuntu, arch = arch_name, file = "ubuntu-{file}",
        depends = ["libc6"],
    )
    target(
        pkg, "archlinux", distro = "arch", job = arch, arch = arch_name, depends = ["glibc"],
        options = {"packager": "Example Maintainer <maintainer@example.com>"},
    )
    # No arm64 Alpine build yet.
    if arch_name == "amd64":
        target(pkg, "apk", distro = "alpine", job = alpine, arch = arch_name, depends = ["musl"])

# An extra file for one format only: give that target its own contents.
target(
    pkg, "ipk", distro = "openwrt", job = debian, arch = "arm64",
    contents = pkg["contents"] + [file("packaging/acme-tool.init", "/etc/init.d/acme-tool", mode = "0755")],
    options = {"arch": "aarch64_generic"},  # OpenWrt architecture; default derived from `arch`
)

# Code decides what exists. This target exists only when you pass `--build-arg NIGHTLY=1`.
if arg("NIGHTLY"):
    target(pkg, "tar", distro = "nightly", job = debian, version = VERSION + "-nightly.1", tags = {"channel": "nightly"})
