# The functions that every pkgsmith.star can call. Each one returns a plain dict, and `target` hands it to pkgsmith.
# pkgsmith checks every field when it reads the dict, so a misspelled field is an error that names it.

def step(run, name = None, env = {}):
    """One script of a job. It runs in a fresh `sh -e`, in /src."""
    return {"run": run, "name": name, "env": env}

def job(runs_on, steps, setup = [], env = {}, exclude = [], arch = None):
    """Builds files. Targets whose jobs are equal share one run of it."""
    return {
        "runs_on": runs_on,
        "steps": steps,
        "setup": setup,
        "env": env,
        "exclude": exclude,
        "arch": arch,
    }

def package(
        name,
        version,
        description,
        release = "1",
        epoch = 0,
        arch = "amd64",
        maintainer = None,
        homepage = None,
        license = None,
        depends = [],
        recommends = [],
        suggests = [],
        conflicts = [],
        replaces = [],
        provides = [],
        contents = [],
        scripts = {},
        options = {}):
    """The fields that most targets share. `target` copies them and may replace any of them."""
    return {
        "name": name,
        "version": version,
        "description": description,
        "release": release,
        "epoch": epoch,
        "arch": arch,
        "maintainer": maintainer,
        "homepage": homepage,
        "license": license,
        "depends": depends,
        "recommends": recommends,
        "suggests": suggests,
        "conflicts": conflicts,
        "replaces": replaces,
        "provides": provides,
        "contents": contents,
        "scripts": scripts,
        "options": options,
    }

def target(pkg, format, distro = None, job = None, file = None, tags = {}, **overrides):
    """One package: `pkg` written as `format`, with the files that `job` builds. Without a job the package holds its
    contents only. Any other keyword replaces the field of `pkg` with that name."""
    for key in overrides:
        if key not in pkg:
            fail("target: unknown field '{}'; the fields of a package are: {}".format(key, ", ".join(sorted(pkg.keys()))))
    spec = dict(pkg)
    spec.update(overrides)
    spec.update({"format": format, "distro": distro, "job": job, "file": file, "tags": tags})
    _register(spec)

def file(src, dst, mode = None, owner = None, group = None, config = False):
    """A file, or the files a glob matches. With config=True the package manager keeps the user's changes."""
    return {"type": "config" if config else "file", "src": src, "dst": dst, "mode": mode, "owner": owner, "group": group}

def config(src, dst, mode = None, owner = None, group = None):
    """A configuration file: the package manager keeps the user's changes."""
    return file(src, dst, mode, owner, group, True)

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
