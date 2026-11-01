# The details every package shares. Other scripts load this file, so the name, the version, the licence and the files
# are written once. A loaded file may define values and functions. If it called package(), that would write packages too.
common = {
    "name": "hello",
    "version": "1.0.0",
    "description": "A small program",
    "maintainer": "Example Maintainer <maintainer@example.com>",
    "homepage": "https://example.com/hello",
    "license": "MIT",
    "contents": [file("../demo/hello.c", "/usr/share/hello/hello.c")],
}

def depends_for(format):
    """The libc package of a format."""
    return {"rpm": ["glibc"], "deb": ["libc6"]}.get(format, [])
