# Install scripts

A package can run scripts when it is installed or removed.

```yaml
scripts:
  pre_install: |
    getent passwd app >/dev/null || useradd --system app
  post_install: |
    systemctl daemon-reload || true
  pre_remove: |
    systemctl stop app || true
  post_remove: packaging/post-remove.sh
```

## Writing a script

- Inline text, or the name of a file next to the manifest. A value with a line break is the script. A one-line value
  is read as a file name.
- A script without a `#!` line gets `#!/bin/sh` and `set -e` in front of it. Add your own `#!` line to use another
  interpreter, for example `#!/bin/bash`.
- Expect an install to be an **upgrade** as well. The same script runs, and it must work when the files are already
  there. Use `|| true` and `getent ... ||` guards.
- Do not read the script arguments. Each format passes different ones. See below.

## What runs when

| Manifest key   | deb, ipk              | rpm      | archlinux                      | apk                              | tar, zip |
| -------------- | --------------------- | -------- | ------------------------------ | -------------------------------- | -------- |
| `pre_install`  | `preinst`             | `%pre`   | `pre_install`, `pre_upgrade`   | `.pre-install`, `.pre-upgrade`   | ignored  |
| `post_install` | `postinst`            | `%post`  | `post_install`, `post_upgrade` | `.post-install`, `.post-upgrade` | ignored  |
| `pre_remove`   | `prerm`               | `%preun` | `pre_remove`                   | `.pre-deinstall`                 | ignored  |
| `post_remove`  | `postrm`              | `%postun`| `post_remove`                  | `.post-deinstall`                | ignored  |

Differences to know about:

- **deb and ipk** run `prerm` and `postrm` on upgrade too. dpkg passes `upgrade` as `$1` then.
- **rpm** runs `%preun` and `%postun` on upgrade too. rpm passes the number of installed copies in `$1`: `0` for
  the last removal and `1` for an upgrade.
- **archlinux** runs the remove scripts only on removal. The install scripts run on install and on upgrade.
- **archlinux** does not run the script as a program. pkgsmith writes it into `.INSTALL` as a shell function, and
  pacman runs it in its own shell. The `#!` line and `set -e` are removed for that.
- **rpm** takes the interpreter from the `#!` line.
- **apk** runs scripts as written. Alpine has no `bash`, so a `#!/bin/bash` script needs the `bash` package.

## Per format or per distro

`overrides` can replace the whole `scripts` block:

```yaml
scripts:
  post_install: systemctl daemon-reload || true
overrides:
  apk:
    scripts:
      post_install: rc-update add app default || true
```

## Example: a service

See [examples/service.yaml](../examples/service.yaml).
