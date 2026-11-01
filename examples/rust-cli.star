# A Rust command line tool for Fedora and Debian, built with cargo inside each distro.
#   pkgsmith build -f examples/rust-cli.star
VERSION = arg("VERSION", "0.3.0")

build = step(
    name = "Build",
    run = """
cargo build --release --locked
install -Dm755 target/release/ripfind /usr/bin/ripfind
install -Dm644 docs/ripfind.1 /usr/share/man/man1/ripfind.1
""",
)

# One job for each distro, because each one builds against its own libraries.
fedora = job(
    runs_on = "fedora:43",
    setup = [step(name = "Install cargo", run = "dnf -y install cargo gcc")],
    steps = [build],
)

debian = job(
    runs_on = "debian:13",
    setup = [
        step(
            name = "Install cargo",
            run = """
apt-get update -qq
apt-get install -y cargo gcc
""",
        ),
    ],
    steps = [build],
)

pkg = package(
    name = "ripfind",
    version = VERSION,
    description = """Find files fast
ripfind searches a folder tree for names and contents.
""",
    maintainer = "Example Maintainer <maintainer@example.com>",
    license = "MIT",
)

# glibc is on every Fedora and Debian system, but the dependency tells the package manager.
target(pkg, "rpm", distro = "fedora", job = fedora, depends = ["glibc"])
target(pkg, "deb", distro = "debian", job = debian, depends = ["libc6"])
