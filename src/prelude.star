# Content helpers for expose(...) and package(contents = [...]). A relative src is a path in the prefix. Each one returns a plain dict. pkgsmith checks every field when it reads the
# dict, so a misspelled field is an error that names it.

def file(src, dst, mode = None, owner = None, group = None):
    """A file, or the files a glob matches."""
    return {"type": "file", "src": src, "dst": dst, "mode": mode, "owner": owner, "group": group}

def config(src, dst, mode = None, owner = None, group = None):
    """A configuration file: the package manager keeps the user's changes."""
    return {"type": "config", "src": src, "dst": dst, "mode": mode, "owner": owner, "group": group}

def tree(src, dst, mode = None, owner = None, group = None):
    """A folder copied recursively, including the folder itself."""
    return {"type": "tree", "src": src, "dst": dst, "mode": mode, "owner": owner, "group": group}

def symlink(link_target, dst):
    """A symbolic link at dst that points to link_target."""
    return {"type": "symlink", "src": link_target, "dst": dst}

def directory(dst, mode = None, owner = None, group = None):
    """An empty folder the package owns."""
    return {"type": "dir", "dst": dst, "mode": mode, "owner": owner, "group": group}

def ghost(dst):
    """A path the package owns but does not carry, for example a log. Only rpm records it."""
    return {"type": "ghost", "dst": dst}
