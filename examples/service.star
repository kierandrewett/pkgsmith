# A system service: unit file, configuration file, runtime folder, a service user and package scripts.
#
#   pkgsmith build -f examples/service.star

sh('go build -o "$pkgdir/usr/bin/acme-agent" ./cmd/agent')

expose(
    # The unit file comes from the source tree. The build does not install it.
    file("packaging/acme-agent.service", "/usr/lib/systemd/system/acme-agent.service", mode = "0644"),
    # A configuration file: the package manager keeps the user's changes.
    config("packaging/agent.yaml", "/etc/acme-agent/agent.yaml", mode = "0640", group = "acme"),
    # A state folder owned by the service user.
    directory("/var/lib/acme-agent", mode = "0750", owner = "acme", group = "acme"),
    # A log file the package owns but does not carry. Only rpm records it.
    ghost("/var/log/acme-agent.log"),
)

# These functions record the scripts of the package. The commands do not run now and do not join the build. They run
# on the machine that installs or removes the package.
def pre_install():
    sh("""
getent group acme >/dev/null || groupadd --system acme
getent passwd acme >/dev/null || useradd --system --gid acme --no-create-home --shell /sbin/nologin acme
""")

def post_install():
    sh("systemctl daemon-reload")
    sh("systemctl enable acme-agent.service || true")

def pre_remove():
    sh("systemctl stop acme-agent.service || true")
    sh("systemctl disable acme-agent.service || true")

def post_remove():
    sh("systemctl daemon-reload")

package(
    host()["format"],
    name = "acme-agent",
    version = "1.4.0",
    description = "A monitoring agent",
    license = "Apache-2.0",
    scripts = {
        "pre_install": pre_install,
        "post_install": post_install,
        "pre_remove": pre_remove,
        "post_remove": post_remove,
    },
)
