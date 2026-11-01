# pkgsmith packages itself. Build the Linux packages for every distro with:
#
#   pkgsmith build --build-arg VERSION=0.1.0
#
# or one distro with `--where distro=fedora`. The packages are written to ./dist.
#
# The dmg, msi and nsis targets have no job, so they run no build, and they exist only when STAGE is set. STAGE is the
# folder that holds a program that was compiled on the target system (see docs/macos-and-windows.md). On a Mac:
#
#   pkgsmith build -F dmg --where arch=arm64 --build-arg STAGE=pkgsmith-aarch64-apple-darwin
#
# On Windows (nsis needs makensis on PATH):
#
#   pkgsmith build -F msi,nsis --build-arg STAGE=pkgsmith-x86_64-pc-windows-msvc

VERSION = arg("VERSION", "0.1.0")
STAGE = arg("STAGE")

# The distro Rust is too old for the dependencies, so a setup step installs a pinned one with rustup.
rust = step(
    name = "Install Rust",
    run = """
curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.99.1
echo "$HOME/.cargo/bin" >> "$PKGSMITH_PATH"
""",
)

# The build installs the binary and every plugin under the name pkgsmith looks for.
build = step(
    name = "Build and install",
    run = """
cargo build --release --workspace --locked
install -Dm755 /tmp/pkgsmith-target/release/pkgsmith /usr/bin/pkgsmith
for f in /tmp/pkgsmith-target/release/libpkgsmith_plugin_*.so; do
  install -Dm755 "$f" "/usr/lib/pkgsmith/plugins/pkgsmith-plugin-${f##*/libpkgsmith_plugin_}"
done
install -Dm644 LICENSE /usr/share/licenses/pkgsmith/LICENSE
install -Dm644 README.md /usr/share/doc/pkgsmith/README.md
""",
)

# -crt-static makes the Alpine binary a dynamic one, which it must be to load plugins.
env = {
    "RUSTFLAGS": "-C target-feature=-crt-static",
    "CARGO_TARGET_DIR": "/tmp/pkgsmith-target",
}

# Fedora, Debian and Arch all use glibc, and a program built against the older glibc of Debian 13 runs on their newer
# one, so one job serves the rpm, deb and Arch packages. Alpine uses musl, so it has a job of its own. A job runs once for
# each architecture, however many targets use it, so `pkgsmith plan` shows one BUILD letter for the rpm, deb and Arch
# packages of an architecture.
glibc = job(
    runs_on = "debian:13",
    env = env,
    setup = [
        step(
            name = "Install the build tools",
            run = """
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y curl ca-certificates gcc make
""",
        ),
        rust,
    ],
    steps = [build],
)

musl = job(
    runs_on = "alpine:3.22",
    env = env,
    setup = [
        step(name = "Install the build tools", run = "apk add --no-cache curl gcc make musl-dev"),
        rust,
    ],
    steps = [build],
)

pkg = package(
    name = "pkgsmith",
    version = VERSION,
    description = """Build distribution packages from one script
pkgsmith builds your program in a container for each distro and writes the rpm, deb, Arch, Alpine, OpenWrt, tar
or zip package. It needs Podman.
""",
    maintainer = "Kieran Drewett <kieran@drewett.dev>",
    homepage = "https://github.com/kierandrewett/pkgsmith",
    license = "MIT",
)

# pkgsmith starts containers through Podman, and it does not look inside the binary for libraries.
for arch in ["amd64", "arm64"]:
    target(pkg, "rpm", distro = "fedora", job = glibc, arch = arch, depends = ["podman", "glibc"])
    target(pkg, "deb", distro = "debian", job = glibc, arch = arch, depends = ["podman", "libc6"])
    # The name of an apk does not hold the architecture, so the two would overwrite each other.
    target(
        pkg, "apk", distro = "alpine", job = musl, arch = arch, depends = ["podman"],
        file = "{name}-{version}-r{release}.{arch}.apk",
    )

    # The archlinux image has no arm64 build.
    if arch == "amd64":
        target(pkg, "archlinux", distro = "arch", job = glibc, arch = arch, depends = ["podman", "glibc"])

# The next targets have no job: they pack the files in STAGE, so they exist only when STAGE is set.
if STAGE:
    stage = [tree(STAGE, "/pkgsmith")]
    for arch in ["amd64", "arm64"]:
        target(pkg, "dmg", distro = "macos", arch = arch, contents = stage, options = {"volume_name": "pkgsmith"})
    target(
        pkg, "msi", distro = "windows", contents = stage,
        options = {"prefix": "/pkgsmith", "manufacturer": "Kieran Drewett"},
    )
    target(pkg, "nsis", distro = "windows", contents = stage, options = {"prefix": "/pkgsmith"})
