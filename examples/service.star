# A system service: a unit file, a configuration file, a state folder, a service user and package scripts.
#
#   go build -o prefix/usr/bin/acme-agent ./cmd/agent
#   install -Dm644 packaging/acme-agent.service prefix/usr/lib/systemd/system/acme-agent.service
#   install -Dm640 packaging/agent.yaml prefix/etc/acme-agent/agent.yaml
#   pkgsmith build -f examples/service.star

# A script of more than one line is the script itself. A one-line value names a file next to this script.
scripts = {
    "pre_install": """getent group acme >/dev/null || groupadd --system acme
getent passwd acme >/dev/null || useradd --system --gid acme --no-create-home --shell /sbin/nologin acme
""",
    "post_install": """systemctl daemon-reload
systemctl enable acme-agent.service || true
""",
    "pre_remove": """systemctl stop acme-agent.service || true
systemctl disable acme-agent.service || true
""",
    "post_remove": """systemctl daemon-reload
""",
}

for format in ["rpm", "deb", "archlinux", "apk"]:
    package(
        format,
        name = "acme-agent",
        version = "1.4.0",
        description = "A monitoring agent",
        license = "Apache-2.0",
        # The package manager keeps the changes of the user in these files.
        config_files = ["/etc/acme-agent/*"],
        # `contents` adds to the whole prefix. The service user owns the state folder. The prefix has no such folder, so
        # the package makes it.
        contents = [directory("/var/lib/acme-agent", mode = "0750", owner = "acme", group = "acme")] + (
            # A log file the package owns but does not carry. Only rpm records it.
            [ghost("/var/log/acme-agent.log")] if format == "rpm" else []
        ),
        scripts = scripts,
    )
