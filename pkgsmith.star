# pkgsmith packages itself. `pkgsmith build` runs this script on this system, top to bottom. sh() adds a command to the
# build, expose() offers the files that the build made, and each package() call that the run reaches writes one package.
# A Fedora machine writes an rpm and a Debian machine writes a deb, so run it on each system you want a package for. The
# packages are written to ./dist.
#
# The dmg, msi and nsis packages need no build, and they exist only when STAGE is set. STAGE is the folder that holds a
# program that was compiled on the target system (see docs/macos-and-windows.md). On a Mac:
#
#   pkgsmith build -F dmg --build-arg STAGE=pkgsmith-aarch64-apple-darwin
#
# On Windows (nsis needs makensis on PATH):
#
#   pkgsmith build -F msi,nsis --build-arg STAGE=pkgsmith-x86_64-pc-windows-msvc

common = {
    "name": "pkgsmith",
    "version": arg("VERSION", "0.1.0"),
    "description": """Build distribution packages from one script
pkgsmith builds your program on this system and writes the rpm, deb, Arch, Alpine, OpenWrt, tar or zip package.""",
    "maintainer": "Kieran Drewett <kieran@drewett.dev>",
    "homepage": "https://github.com/kierandrewett/pkgsmith",
    "license": "MIT",
}

STAGE = arg("STAGE")
format = host()["format"]

if STAGE:
    # These packages pack the files in STAGE and run no build.
    stage = [tree(STAGE, "/pkgsmith")]
    package("dmg", distro = "macos", contents = stage, options = {"volume_name": "pkgsmith"}, **common)
    package("msi", distro = "windows", contents = stage, options = {"prefix": "/pkgsmith", "manufacturer": "Kieran Drewett"}, **common)
    package("nsis", distro = "windows", contents = stage, options = {"prefix": "/pkgsmith"}, **common)
else:
    # -crt-static makes the Alpine binary a dynamic one, which it must be to load plugins.
    sh("cargo build --release --workspace --locked", name = "Build", env = {"RUSTFLAGS": "-C target-feature=-crt-static"})
    expose(
        file("target/release/pkgsmith", "/usr/bin/pkgsmith", mode = "0755"),
        file("target/release/libpkgsmith_plugin_*.so", "/usr/lib/pkgsmith/plugins/", mode = "0755"),
        file("LICENSE", "/usr/share/licenses/pkgsmith/LICENSE"),
        file("README.md", "/usr/share/doc/pkgsmith/README.md"),
    )

    # pkgsmith finds plugins by the name that cargo gives them, so the library files need no renaming.
    if format == "rpm":
        package("rpm", depends = ["glibc"], **common)
    elif format == "deb":
        package("deb", depends = ["libc6"], **common)
    elif format == "archlinux":
        package("archlinux", depends = ["glibc"], **common)
    elif format == "apk":
        package("apk", **common)
    else:
        package("tar", **common)
