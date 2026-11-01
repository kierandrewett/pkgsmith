# One script, four distros. Each distro has a job with its own image and setup step. The deb and tar packages of Debian
# use one job, so they share one build.
# The build step is the same everywhere. pkgsmith records the files it installs and packages them.
build = step(run = "make && make install PREFIX=/usr")

fedora = job(runs_on = "fedora:43", setup = [step(run = "dnf -y install gcc make")], steps = [build])
debian = job(
    runs_on = "debian:13",
    setup = [
        step(
            run = """
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y gcc make
""",
        ),
    ],
    steps = [build],
)
arch = job(runs_on = "archlinux:base", setup = [step(run = "pacman -Sy --noconfirm gcc make")], steps = [build])
alpine = job(runs_on = "alpine:3.22", setup = [step(run = "apk add --no-cache gcc make musl-dev")], steps = [build])

pkg = package(
    name = "hello",
    version = "1.0.0",
    description = "A demo that packages a small C program",
    maintainer = "Kieran Drewett <kieran@drewett.dev>",
    license = "MIT",
)

target(pkg, "rpm", distro = "fedora", job = fedora)
target(pkg, "deb", distro = "debian", job = debian)
target(pkg, "tar", distro = "debian", job = debian)
target(pkg, "archlinux", distro = "arch", job = arch)
target(pkg, "apk", distro = "alpine", job = alpine)
