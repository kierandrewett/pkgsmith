#!/usr/bin/env bash
# Checks package signing end to end with the native tools. It builds signed packages with the three signer plugins,
# then verifies them with rpm (Fedora) and apk (Alpine) in Podman and with gpg, and it checks that a changed package and
# a wrong passphrase are refused.
#
#   scripts/verify-signing.sh [PATH-TO-pkgsmith]
#
# It needs gpg, openssl and podman. pkgsmith builds nothing and starts no container: this script makes a small prefix
# and pkgsmith packs it. Podman only installs the packages into a clean distro. Like verify-formats.sh it installs into
# the newest Fedora and Alpine on every run, so a new release that breaks a signature is found. Set IMAGE_FEDORA or
# IMAGE_ALPINE to test one fixed release. pkgsmith and its plugins must sit together, as `cargo build --workspace`
# leaves them. Every key is made fresh in a temporary folder, with a private GNUPGHOME.
set -euo pipefail

IMAGE_FEDORA="${IMAGE_FEDORA:-registry.fedoraproject.org/fedora:latest}"
IMAGE_ALPINE="${IMAGE_ALPINE:-docker.io/library/alpine:latest}"
PKGSMITH="${1:-target/debug/pkgsmith}"
PKGSMITH="$(realpath "$PKGSMITH")"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export GNUPGHOME="$WORK/gnupg"
mkdir -m 700 "$GNUPGHOME"
cd "$WORK"

failures=0
check() { # check NAME COMMAND...
    local name="$1"; shift
    if "$@" >"$WORK/check.log" 2>&1; then
        echo "ok    $name"
    else
        echo "FAIL  $name"; sed 's/^/        /' "$WORK/check.log"; failures=$((failures + 1))
    fi
}
refused() { # refused NAME COMMAND...: the command must fail
    local name="$1"; shift
    if "$@" >"$WORK/check.log" 2>&1; then
        echo "FAIL  $name (it succeeded)"; failures=$((failures + 1))
    else
        echo "ok    $name"
    fi
}

# A tiny package: a prefix with one program that prints a line, and a script that writes it in three formats.
umask 022
mkdir -p prefix/usr/bin
printf '#!/bin/sh\necho signed-hi\n' > prefix/usr/bin/hi
chmod 755 prefix/usr/bin/hi
cat > pkgsmith.star <<'STAR'

common = {"name": "hi", "version": "1.0.0", "description": "A tiny signing test"}
for fmt in ["tar", "rpm", "apk"]:
    package(fmt, **common)
STAR

# Keys: an OpenPGP key, an OpenPGP key with a passphrase, an RSA key, an RSA key with a passphrase.
gpg --batch --passphrase '' --quick-gen-key 'Plain <plain@example.org>' rsa3072 sign never 2>/dev/null
gpg --armor --export-secret-keys plain@example.org > plain.asc
gpg --armor --export plain@example.org > plain.pub.asc
gpg --batch --pinentry-mode loopback --passphrase pw1 --quick-gen-key 'Locked <locked@example.org>' rsa3072 sign never 2>/dev/null
gpg --batch --pinentry-mode loopback --passphrase pw1 --armor --export-secret-keys locked@example.org > locked.asc
printf 'pw1\n' > pw1; printf 'wrong\n' > pw-wrong; printf 's3cret\n' > pw-s3cret
openssl genrsa -out release.rsa 3072 2>/dev/null
openssl rsa -in release.rsa -pubout -out release.rsa.pub 2>/dev/null
openssl pkcs8 -topk8 -in release.rsa -out locked.rsa -passout pass:s3cret 2>/dev/null

build() { "$PKGSMITH" build . --prefix prefix -F "$1" -o "$2" --progress plain "${@:3}"; }

echo "== plugins"
check "signers are listed" bash -c "'$PKGSMITH' plugins | grep -q 'signer-gpg' && '$PKGSMITH' plugins | grep -q 'signer-openpgp' && '$PKGSMITH' plugins | grep -q 'signer-rsa'"

echo "== detached signatures (tar) checked with gpg"
check "openpgp-file, binary" build tar out-bin --sign-key plain.asc
check "  gpg --verify" gpg --verify out-bin/hi-1.0.0-amd64.tar.xz.sig out-bin/hi-1.0.0-amd64.tar.xz
check "openpgp-file, armor" build tar out-arm --sign-key plain.asc --sign-armor
check "  gpg --verify" gpg --verify out-arm/hi-1.0.0-amd64.tar.xz.asc out-arm/hi-1.0.0-amd64.tar.xz
check "openpgp-file, key with a passphrase" build tar out-lock --sign-key locked.asc --sign-passphrase-file pw1
check "  gpg --verify" gpg --verify out-lock/hi-1.0.0-amd64.tar.xz.sig out-lock/hi-1.0.0-amd64.tar.xz
check "gpg signer, key id" build tar out-gpg --sign-key gpg:plain@example.org
check "  gpg --verify" gpg --verify out-gpg/hi-1.0.0-amd64.tar.xz.sig out-gpg/hi-1.0.0-amd64.tar.xz
check "gpg signer, key id with a passphrase file" build tar out-gpg2 --sign-key gpg:locked@example.org --sign-passphrase-file pw1
check "  gpg --verify" gpg --verify out-gpg2/hi-1.0.0-amd64.tar.xz.sig out-gpg2/hi-1.0.0-amd64.tar.xz
cp out-bin/hi-1.0.0-amd64.tar.xz tampered.tar.xz; printf 'x' >> tampered.tar.xz
refused "  a changed file fails gpg --verify" gpg --verify out-bin/hi-1.0.0-amd64.tar.xz.sig tampered.tar.xz

echo "== refusals before any build"
refused "key of the wrong type for tar" build tar out-x --sign-key release.rsa
refused "unknown gpg key" build tar out-x --sign-key gpg:nobody@example.org
refused "locked OpenPGP key, no passphrase" build tar out-x --sign-key locked.asc
refused "locked OpenPGP key, wrong passphrase" build tar out-x --sign-key locked.asc --sign-passphrase-file pw-wrong
refused "locked RSA key, no passphrase" build apk out-x --sign-key locked.rsa
refused "locked RSA key, wrong passphrase" build apk out-x --sign-key locked.rsa --sign-passphrase-file pw-wrong
refused "a key file that does not exist" build tar out-x --sign-key /nonexistent.asc

echo "== rpm, embedded OpenPGP signature, checked with rpm in Fedora"
check "build the rpm" build rpm out-rpm --sign-key plain.asc
check "build the rpm through gpg" build rpm out-rpm-gpg --sign-key gpg:plain@example.org
cp out-rpm/hi-1.0.0-1.x86_64.rpm tampered.rpm
python3 - <<'PY'
d = bytearray(open('tampered.rpm', 'rb').read()); d[-5] ^= 0xFF; open('tampered.rpm', 'wb').write(d)
PY
cp plain.pub.asc out-rpm/
cp tampered.rpm out-rpm/
check "rpm -Kv: NOKEY before the import, OK after, BAD when changed" podman run --rm --pull=always -v "$PWD/out-rpm:/p:ro,z" "$IMAGE_FEDORA" sh -ec '
    rpm -Kv /p/hi-1.0.0-1.x86_64.rpm | grep -q "NOKEY"
    rpm --import /p/plain.pub.asc
    out=$(rpm -Kv /p/hi-1.0.0-1.x86_64.rpm)
    echo "$out"
    test "$(echo "$out" | grep -c "signature.*: OK")" -ge 2
    ! echo "$out" | grep -q "BAD"
    rpm -Kv /p/tampered.rpm | grep -q "BAD"
    rpm -i /p/hi-1.0.0-1.x86_64.rpm && hi | grep -q signed-hi'
cp plain.pub.asc out-rpm-gpg/
check "rpm made through gpg verifies" podman run --rm --pull=always -v "$PWD/out-rpm-gpg:/p:ro,z" "$IMAGE_FEDORA" sh -ec '
    rpm --import /p/plain.pub.asc
    rpm -Kv /p/hi-1.0.0-1.x86_64.rpm | grep -q "signature.*: OK"'

echo "== apk, embedded RSA signature, checked with apk in Alpine (no --allow-untrusted)"
check "build the apk" build apk out-apk --sign-key release.rsa
check "build the apk from an encrypted key" build apk out-apk-enc --sign-key locked.rsa --sign-passphrase-file pw-s3cret
cp release.rsa.pub out-apk/
check "apk: UNTRUSTED without the key, installs with it" podman run --rm --pull=always -v "$PWD/out-apk:/p:ro,z" "$IMAGE_ALPINE" sh -ec '
    if apk add --no-cache --no-network /p/hi-1.0.0-r1.apk 2>&1 | grep -q "UNTRUSTED"; then :; else echo "expected UNTRUSTED"; exit 1; fi
    cp /p/release.rsa.pub /etc/apk/keys/release.rsa.pub
    apk add --no-cache --no-network /p/hi-1.0.0-r1.apk
    hi | grep -q signed-hi'

echo
if [ "$failures" -ne 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "all checks passed"
