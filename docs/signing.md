# Signing packages

Give `pkgsmith build` a key and it signs the packages. Without a key it writes unsigned packages, as before.

```sh
pkgsmith build --sign-key signing-key.asc
```

How a package is signed depends on its format. Each format needs a key of one type.

| Format                              | Signature                                   | Key you give                    |
| ----------------------------------- | ------------------------------------------- | ------------------------------- |
| `rpm`                               | Inside the package, OpenPGP                 | Armored OpenPGP secret key      |
| `apk`                               | Inside the package, RSA with SHA-256        | RSA private key, PEM            |
| `deb`, `ipk`, `archlinux`, `tar`, `zip` | A file next to the package, OpenPGP     | Armored OpenPGP secret key      |
| `snap`, `oci`                       | Not signed by pkgsmith                      | None                            |

A format that cannot be signed produces an unsigned package and the summary says so. pkgsmith does not sign snap
packages or container images.

## Keys

pkgsmith reads two kinds of key file.

- An **OpenPGP secret key** in ASCII armor. Export one with `gpg --armor --export-secret-keys KEYID > key.asc`. The key
  must be able to sign. pkgsmith signs with the primary key and SHA-256.
- An **RSA private key** in PEM form: PKCS#1 (`BEGIN RSA PRIVATE KEY`), PKCS#8 (`BEGIN PRIVATE KEY`) or encrypted PKCS#8
  (`BEGIN ENCRYPTED PRIVATE KEY`).

Make a test key of each kind:

```sh
gpg --batch --passphrase '' --quick-gen-key 'Release Signer <you@example.org>' rsa3072 sign never
gpg --armor --export-secret-keys you@example.org > key.asc
gpg --armor --export you@example.org > key.pub.asc

openssl genrsa -out release.rsa 3072
openssl rsa -in release.rsa -pubout -out release.rsa.pub
```

## Options

| Option                          | Meaning                                                                          |
| ------------------------------- | -------------------------------------------------------------------------------- |
| `--sign-key FILE`               | A key for every format it fits. Repeatable                                       |
| `--sign-key FORMAT=FILE`        | A key for one format, for example `--sign-key apk=release.rsa`. It wins over `FILE` |
| `--sign-passphrase-file FILE`   | A file with the passphrase of the keys. One passphrase unlocks all of them       |
| `--sign-armor`                  | Write detached signatures as ASCII armor in `FILE.asc` instead of binary in `FILE.sig` |

The environment can replace the options. This is the way to pass a key in CI.

| Variable                   | Meaning                                                              |
| -------------------------- | -------------------------------------------------------------------- |
| `PKGSMITH_SIGN_KEY`        | Key files separated by `:`. Each may start with `FORMAT=`            |
| `PKGSMITH_SIGN_PASSPHRASE` | The passphrase of the keys                                           |

pkgsmith checks the keys before it starts a build. If a format needs a key that you did not give, or the key has the
wrong type, or the passphrase is missing or wrong, the command stops and says which.

```sh
# One OpenPGP key for rpm and every detached signature, one RSA key for apk
pkgsmith build --sign-key key.asc --sign-key apk=release.rsa

# Keys in a CI job
PKGSMITH_SIGN_KEY="rpm=$RUNNER_TEMP/key.asc:apk=$RUNNER_TEMP/release.rsa" pkgsmith build
```

## Checking a signature

Whoever installs the package needs your **public** key.

**rpm.** Import the key, then check the file. `rpm -i` checks the signature too once the key is imported.

```sh
rpm --import key.pub.asc
rpm -Kv hello-1.0.0-1.x86_64.rpm
```

pkgsmith writes both the header signature and the whole-package signature, and `rpm -Kv` shows both as `OK`.

**apk.** pkgsmith names the signature after the private key file: `release.rsa` becomes `.SIGN.RSA256.release.rsa.pub`.
Copy the public key into `/etc/apk/keys` under that name. Then `apk add` accepts the package without
`--allow-untrusted`.

```sh
cp release.rsa.pub /etc/apk/keys/release.rsa.pub
apk add ./hello-1.0.0-r1.apk
```

Without the key, `apk` stops with `UNTRUSTED signature`.

**deb, ipk, archlinux, tar, zip.** Check the file next to the package with `gpg`:

```sh
gpg --import key.pub.asc
gpg --verify hello-1.0.0.tar.xz.sig hello-1.0.0.tar.xz
```

Package managers do not check a `.sig` file next to a loose `.deb`. Arch's `pacman` does read a `FILE.sig` next to a
package when it installs from a repository or a file, and uses the keys in its own keyring.

## What was tested

| Format | Checked with                                                                                  |
| ------ | --------------------------------------------------------------------------------------------- |
| `rpm`  | `rpm -Kv` in Fedora 43 with an RSA key: `NOKEY` before the import, `OK` after it, `BAD` after a byte was changed |
| `apk`  | `apk add` in Alpine 3.22: `UNTRUSTED signature` without the key, installed with it            |
| `tar`  | `gpg --verify` for binary and armored signatures, with and without a passphrase               |

Not tested: signed `deb`, `ipk` and `archlinux` packages with their package managers, and OpenPGP keys that are not RSA.
For those, rpm uses the `DSA` and `GPG` tags and the signature file is made the same way as for RSA.

## Keys and plugins

A plugin that signs asks pkgsmith for a signature. The key is not part of the package description that the plugin
receives. A plugin is native code in the process of pkgsmith, so it can still read the key file. Install plugins only
from a source that you trust.
