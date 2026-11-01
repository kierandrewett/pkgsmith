# Signing packages

Give `pkgsmith build` a key and it signs the packages. Without a key it writes unsigned packages.

```sh
pkgsmith build --sign-key gpg:release@example.org
```

Signing is done by **signer plugins**, like formats are done by format plugins. `pkgsmith plugins` lists them.

## Signers

| Signer         | Key you give                                      | Makes                          | Use it when                              |
| -------------- | ------------------------------------------------- | ------------------------------ | ---------------------------------------- |
| `gpg`          | A key id, fingerprint or e-mail address           | OpenPGP                        | The key lives in `gpg-agent`, a smartcard or a hardware key |
| `openpgp-file` | An armored OpenPGP secret key file                | OpenPGP                        | CI, where the key is a secret and gpg is not installed |
| `rsa-pem`      | An RSA private key file, PEM                      | RSA with SHA-256 or SHA-1      | Alpine packages                          |

- **`gpg`** runs your `gpg`. The agent asks for the passphrase and does the signing, so the secret key never reaches
  pkgsmith. A passphrase file is passed to gpg with `--pinentry-mode loopback`.
- **`openpgp-file`** needs no `gpg`. It signs with a signing subkey if the key has one, else with the primary key if
  that may sign. A key that cannot sign is refused.
- **`rsa-pem`** reads PKCS#1, PKCS#8 and encrypted PKCS#8 keys, and hashes the package as a stream.

## How each format is signed

| Format                                  | Signature                                    | Scheme       |
| --------------------------------------- | -------------------------------------------- | ------------ |
| `rpm`                                   | Inside the package                           | OpenPGP      |
| `apk`                                   | Inside the package                           | RSA SHA-256  |
| `deb`, `ipk`, `archlinux`, `tar`, `zip`, `slackware` | A file next to the package, `FILE.sig` | OpenPGP      |
| `snap`, `oci`                           | Not signed by pkgsmith                       | None         |

A format that cannot be signed produces an unsigned package, and the summary says so.

A detached `FILE.sig` is the signature that Arch's `pacman` reads next to a package. For `deb`, `apt` signs the
repository metadata and not single files, so the `.sig` is for people who check the file by hand.

## Options

`--sign-key` takes `[FORMAT=][SIGNER:]KEY`.

| Part     | Meaning                                                                                       |
| -------- | --------------------------------------------------------------------------------------------- |
| `KEY`    | A key file, or for `gpg` a key id                                                             |
| `SIGNER:` | Names the signer plugin. Without it, pkgsmith asks the signers one by one which one takes the key |
| `FORMAT=` | Limits the key to one format. Without it the key is tried for every format                   |

| Option                      | Meaning                                                                          |
| --------------------------- | -------------------------------------------------------------------------------- |
| `--sign-key [FORMAT=][SIGNER:]KEY` | Repeatable. A key named for the format wins over a general one            |
| `--sign-passphrase-file FILE` | A file with the passphrase of the keys. A file, never the passphrase on the command line |
| `--sign-armor`              | Write detached signatures as ASCII armor in `FILE.asc` instead of binary in `FILE.sig` |

In CI, use the environment instead of the options:

| Variable                         | Meaning                                                        |
| -------------------------------- | -------------------------------------------------------------- |
| `PKGSMITH_SIGN_KEY`              | The `--sign-key` values, separated by `;`                      |
| `PKGSMITH_SIGN_PASSPHRASE_FILE`  | The passphrase file                                            |

```sh
# rpm and every detached signature through gpg, apk with an RSA key
pkgsmith build --sign-key gpg:release@example.org --sign-key apk=release.rsa

# a CI job
PKGSMITH_SIGN_KEY="rpm=$RUNNER_TEMP/key.asc;apk=$RUNNER_TEMP/release.rsa" pkgsmith build
```

Before the first build starts, pkgsmith opens every key and asks the signers about it. These stop the run at once,
with the reason:

- a missing key file
- a key of the wrong type
- a locked key without its passphrase file, or with a wrong one

A signer that claims a key and cannot use it ends the search. pkgsmith does not fall back to another key.

## Checking a signature

Whoever installs the package needs your **public** key.

**rpm.** Import the key. `rpm -K` then shows the header signature and the whole-package signature as `OK`.

```sh
rpm --import key.pub.asc
rpm -Kv hello-1.0.0-1.x86_64.rpm
```

**apk.** The signature is named after the private key file: `release.rsa` gives `.SIGN.RSA256.release.rsa.pub`. Copy the
public key into `/etc/apk/keys` under the same name. `apk add` then accepts the package without `--allow-untrusted`.

```sh
cp release.rsa.pub /etc/apk/keys/release.rsa.pub
apk add ./hello-1.0.0-r1.apk
```

**The others.** Check the file next to the package:

```sh
gpg --import key.pub.asc
gpg --verify hello-1.0.0.tar.xz.sig hello-1.0.0.tar.xz
```

## What was tested

[scripts/verify-signing.sh](../scripts/verify-signing.sh) runs these checks. It makes fresh keys in a temporary folder
and a private `GNUPGHOME`.

| Check                                                                   | Tool                            |
| ----------------------------------------------------------------------- | ------------------------------- |
| Detached signatures from `gpg` and `openpgp-file`, binary and armored, with and without a passphrase | `gpg --verify` |
| A changed file is refused                                               | `gpg --verify`                  |
| rpm: `NOKEY` before the key import, `OK` after, `BAD` after a changed byte | `rpm -Kv` in Fedora 43       |
| rpm signed through `gpg`                                                | `rpm -Kv` in Fedora 43          |
| apk: `UNTRUSTED` without the key, installed with it, from a plain and an encrypted key | `apk add` in Alpine 3.22 |
| Wrong key type, unknown gpg key, locked key without or with the wrong passphrase, missing file | pkgsmith stops before building |

Not tested: signed `deb`, `ipk`, `archlinux` and `slackware` packages with their own package managers, and non-RSA
OpenPGP keys for rpm (rpm then uses the `DSA` and `GPG` tags).

## Plugins and keys

A signer plugin is native code in the process of pkgsmith, as every plugin is. The `gpg` signer keeps the key in gpg.
The other two read the key file in pkgsmith's process, so a malicious plugin could read it. Install plugins only from a
source that you trust. To write a signer, see [plugins](plugins.md#writing-a-signer).
