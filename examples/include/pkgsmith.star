# The script that you build. It loads the common details from common.star (a path relative to this file). A keyword of
# `target` replaces the same field of the package, so this target changes the description and sets its own options.
load("common.star", "common")

target(
    common, "tar", distro = "any",
    description = "A small program, as a release archive",
    file = "{name}-{version}.tar.xz",
    options = {"compression": "xz"},
)
