# A system service: unit file, configuration file, runtime folder, a service user and scripts.
#   pkgsmith build -f examples/service.star -F rpm
build = job(
    runs_on = "fedora:43",
    setup = [step(name = "Install the build tools", run = "dnf -y install golang")],
    steps = [step(name = "Build", run = "go build -o /usr/bin/acme-agent ./cmd/agent")],
)

pkg = package(
    name = "acme-agent",
    version = "1.4.0",
    description = "A monitoring agent",
    license = "Apache-2.0",
    contents = [
        # The unit file comes from the source tree. It is not part of the build output.
        file("packaging/acme-agent.service", "/usr/lib/systemd/system/acme-agent.service", mode = "0644"),
        # A configuration file: the package manager keeps the user's changes.
        config("packaging/agent.yaml", "/etc/acme-agent/agent.yaml", mode = "0640", group = "acme"),
        # A state folder owned by the service user.
        directory("/var/lib/acme-agent", mode = "0750", owner = "acme", group = "acme"),
        # A log file the package owns but does not carry. Only rpm records it.
        ghost("/var/log/acme-agent.log"),
    ],
    scripts = {
        "pre_install": """
getent group acme >/dev/null || groupadd --system acme
getent passwd acme >/dev/null || useradd --system --gid acme --no-create-home --shell /sbin/nologin acme
""",
        "post_install": """
systemctl daemon-reload
systemctl enable acme-agent.service || true
""",
        "pre_remove": """
systemctl stop acme-agent.service || true
systemctl disable acme-agent.service || true
""",
        "post_remove": "systemctl daemon-reload",
    },
)

target(pkg, "rpm", distro = "fedora", job = build)
