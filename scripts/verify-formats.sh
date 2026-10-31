#!/usr/bin/env bash
# Builds one small package in every format and checks each with the tool of that format, in Podman. This is the test
# suite of the format plugins. It saves a log, so a run can be inspected and repeated.
#
#   scripts/verify-formats.sh [PATH-TO-pkgsmith] [FORMAT...]
#
# Without FORMAT every format is checked. pkgsmith and its plugins must sit together, as `cargo build --workspace` leaves
# them. It needs podman. Every run pulls the NEWEST version of each image it installs into, on purpose: the suite exists
# to find the bug that a new distro release brings, so it must not test a frozen environment. The image digests of a
# run are written to the log. To test one fixed release, set a variable such as IMAGE_FEDORA=fedora:43, and set
# PKGSMITH_NO_PULL=1 to use the images that are already on the machine. The appimage check needs a runtime file in
# PKGSMITH_APPIMAGE_RUNTIME (from github.com/AppImage/type2-runtime/releases) and is skipped without it. The wheel check
# needs python3 with venv. The result is written to verify-formats.log in the current folder.
set -uo pipefail

PKGSMITH="$(realpath "${1:-target/debug/pkgsmith}")"
shift || true
WANT=("$@")
WORK="$(mktemp -d "${TMPDIR:-/var/tmp}/pkgsmith-verify.XXXXXX")"
LOG="$PWD/verify-formats.log"
trap 'rm -rf "$WORK"' EXIT
: > "$LOG"
cd "$WORK"

# The environments the packages are installed into. The rolling tags always point to the current release.
IMAGE_FEDORA="${IMAGE_FEDORA:-registry.fedoraproject.org/fedora:latest}"
IMAGE_DEBIAN="${IMAGE_DEBIAN:-docker.io/library/debian:latest}"
IMAGE_ARCH="${IMAGE_ARCH:-docker.io/library/archlinux:base}"
IMAGE_ALPINE="${IMAGE_ALPINE:-docker.io/library/alpine:latest}"
IMAGE_SLACKWARE="${IMAGE_SLACKWARE:-docker.io/vbatts/slackware:current}"
IMAGE_VOID="${IMAGE_VOID:-ghcr.io/void-linux/void-glibc:latest}"
PULL="--pull=always"
[ -n "${PKGSMITH_NO_PULL:-}" ] && PULL="--pull=missing"

passed=0; failed=0; skipped=0
selected() { [ ${#WANT[@]} -eq 0 ] && return 0; local f; for f in "${WANT[@]}"; do [ "$f" = "$1" ] && return 0; done; return 1; }
say() { echo "$@" | tee -a "$LOG"; }
check() { # check NAME COMMAND...
    local name="$1"; shift
    { echo "--- $name"; "$@"; } >"$WORK/out.log" 2>&1
    local status=$?
    cat "$WORK/out.log" >> "$LOG"
    if [ $status -eq 0 ]; then say "ok    $name"; passed=$((passed + 1))
    else say "FAIL  $name"; sed 's/^/        /' "$WORK/out.log" | tail -15; failed=$((failed + 1)); fi
}
skip() { say "skip  $1 ($2)"; skipped=$((skipped + 1)); }
need() { command -v "$1" >/dev/null 2>&1; }

need podman || { echo "podman is required"; exit 2; }

# Pull every image now, so a pull error is one clear failure and the log shows what was tested.
for image in "$IMAGE_FEDORA" "$IMAGE_DEBIAN" "$IMAGE_ARCH" "$IMAGE_ALPINE" "$IMAGE_SLACKWARE" "$IMAGE_VOID"; do
    if [ -z "${PKGSMITH_NO_PULL:-}" ]; then podman pull -q "$image" >>"$LOG" 2>&1 || echo "could not pull $image" >&2; fi
    echo "image $image $(podman image inspect --format '{{.Digest}} {{.Created}}' "$image" 2>/dev/null)" | tee -a "$LOG"
done

# The package: a script, a config file, a symlink, a Python module, and a static program for the container image.
mkdir -p stage/usr/bin stage/etc stage/usr/lib/python3/site-packages/hipkg
printf '#!/bin/sh\necho hi-from-package\n' > stage/usr/bin/hi; chmod 755 stage/usr/bin/hi
printf 'greeting=hi\n' > stage/etc/hi.conf
printf 'VALUE = 1\n' > stage/usr/lib/python3/site-packages/hipkg/__init__.py
printf '#include <stdio.h>\nint main(void){puts("hi-from-image");return 0;}\n' > hello.c
podman run --rm $PULL -v "$PWD:/w:z" -w /w "$IMAGE_ALPINE" sh -c 'apk add --no-cache gcc musl-dev >/dev/null && cc -static -O2 -o stage/usr/bin/hi-static hello.c' >>"$LOG" 2>&1 \
    || echo "could not build the static test program; the oci check will fail" >&2
cat > pkgsmith.yaml <<'YAML'
name: hi
version: 1.0.0
description: |
  Says hi
  A tiny package that every format test installs.
maintainer: Test <test@example.org>
homepage: https://example.org/hi
license: MIT
contents:
  - { src: stage/usr/bin/hi, dst: /usr/bin/hi }
  - { src: stage/usr/bin/hi-static, dst: /usr/bin/hi-static }
  - { src: stage/etc/hi.conf, dst: /etc/hi.conf, type: config }
  - { src: /usr/bin/hi, dst: /usr/bin/hello, type: symlink, only: [rpm, deb, ipk, archlinux, apk, slackware, tar, zip, snap, xbps, freebsd] }
  - { src: stage/usr/lib/python3/site-packages/hipkg/__init__.py, dst: /usr/lib/python3/site-packages/hipkg/__init__.py }
scripts:
  post_install: |
    echo post-install-ran > /tmp/hi-post
oci:
  entrypoint: [/usr/bin/hi-static]
snap:
  apps:
    hi: { command: /usr/bin/hi }
overrides:
  freebsd:
    depends: [libfoo]
freebsd:
  abi_version: 14
  dep_origins: { libfoo: devel/libfoo }
wheel:
  site_packages: /usr/lib/python3/site-packages
  platform_tag: manylinux_2_17_x86_64
YAML

build() { "$PKGSMITH" build . -F "$1" -o "out-$1" --progress plain; }
in_image() { # in_image IMAGE DIR SCRIPT: run a script with the output folder mounted at /p
    podman run --rm $PULL -v "$WORK/$2:/p:ro,z" "$1" sh -ec "$3"
}

if selected tar; then
    check "tar: build" build tar
    check "tar: lists the files" sh -c 'tar -tf out-tar/hi-1.0.0-amd64.tar.xz | grep -q "usr/bin/hi$"'
fi
if selected zip; then
    check "zip: build" build zip
    check "zip: unzip -t and the symlink" sh -c 'unzip -tq out-zip/hi-1.0.0-amd64.zip && unzip -Z out-zip/hi-1.0.0-amd64.zip | grep -q "usr/bin/hello"'
fi
if selected rpm; then
    check "rpm: build" build rpm
    check "rpm: install, run, verify, remove (latest Fedora)" in_image "$IMAGE_FEDORA" out-rpm '
        rpm -i --nodeps /p/hi-1.0.0-1.x86_64.rpm; hi | grep -q hi-from-package; test -L /usr/bin/hello
        cat /tmp/hi-post | grep -q post-install-ran; rpm -V hi; rpm -e hi; ! test -e /usr/bin/hi'
fi
if selected deb; then
    check "deb: build" build deb
    check "deb: install, run, verify, remove (latest Debian)" in_image "$IMAGE_DEBIAN" out-deb '
        dpkg -i --force-depends /p/hi_1.0.0-1_amd64.deb >/dev/null; hi | grep -q hi-from-package
        cat /tmp/hi-post | grep -q post-install-ran; dpkg -V hi; dpkg -r --force-depends hi >/dev/null; ! test -e /usr/bin/hi'
fi
if selected ipk; then
    check "ipk: build" build ipk
    check "ipk: three members in order" sh -c 'tar -tzf out-ipk/hi_1.0.0-1_x86_64.ipk | head -3 | tr "\n" " " | grep -q "debian-binary.*data.tar.gz.*control.tar.gz"'
fi
if selected archlinux; then
    check "archlinux: build" build archlinux
    check "archlinux: install, run, check (current Arch)" in_image "$IMAGE_ARCH" out-archlinux '
        pacman -U --noconfirm --nodeps /p/hi-1.0.0-1-x86_64.pkg.tar.zst >/dev/null 2>&1; hi | grep -q hi-from-package
        pacman -Qkk hi | grep -q "0 altered files"; pacman -R --noconfirm hi >/dev/null 2>&1; ! test -e /usr/bin/hi'
fi
if selected apk; then
    check "apk: build" build apk
    check "apk: install and run (latest Alpine)" in_image "$IMAGE_ALPINE" out-apk '
        apk add --allow-untrusted --no-network --force-missing-repositories /p/hi-1.0.0-r1.apk >/dev/null 2>&1 || apk add --allow-untrusted --no-cache /p/hi-1.0.0-r1.apk >/dev/null
        hi | grep -q hi-from-package; apk del hi >/dev/null; ! test -e /usr/bin/hi'
fi
if selected slackware; then
    check "slackware: build" build slackware
    check "slackware: installpkg, upgradepkg, removepkg (Slackware current)" in_image "$IMAGE_SLACKWARE" out-slackware '
        installpkg /p/hi-1.0.0-x86_64-1.txz >/dev/null 2>&1; hi | grep -q hi-from-package; test -f /etc/hi.conf
        echo changed > /etc/hi.conf; upgradepkg --reinstall /p/hi-1.0.0-x86_64-1.txz >/dev/null 2>&1; grep -q changed /etc/hi.conf
        removepkg hi >/dev/null 2>&1; ! test -e /usr/bin/hi'
fi
if selected snap; then
    check "snap: build" build snap
    check "snap: unsquashfs lists the tree and snap.yaml parses" sh -c 'unsquashfs -s out-snap/*.snap | grep -q "Compression xz" && unsquashfs -cat out-snap/*.snap meta/snap.yaml | python3 -c "import sys,yaml; d=yaml.safe_load(sys.stdin); assert d[\"name\"]==\"hi\" and \"hi\" in d[\"apps\"]"'
    skip "snap: snapd accepts it" "snapd is not available here"
fi
if selected oci; then
    check "oci: build" build oci
    check "oci: podman load and run" sh -c 'podman load -i out-oci/hi-1.0.0-amd64.oci.tar >/dev/null && podman run --rm localhost/hi:1.0.0 | grep -q hi-from-image; status=$?; podman rmi localhost/hi:1.0.0 >/dev/null 2>&1; exit $status'
fi
if selected appimage; then
    if [ -n "${PKGSMITH_APPIMAGE_RUNTIME:-}" ] && [ -f "$PKGSMITH_APPIMAGE_RUNTIME" ]; then
        printf '\x89PNG\r\n\x1a\n' > icon.png
        cp pkgsmith.yaml appimage.yaml
        printf 'appimage:\n  runtime: %s\n  icon: icon.png\n  exec: usr/bin/hi-static\n' "$PKGSMITH_APPIMAGE_RUNTIME" >> appimage.yaml
        check "appimage: build" "$PKGSMITH" build . -f appimage.yaml -F appimage -o out-appimage --progress plain
        check "appimage: runs without FUSE" sh -c './out-appimage/hi-1.0.0-x86_64.AppImage --appimage-extract-and-run | grep -q hi-from-image'
    else
        skip "appimage" "set PKGSMITH_APPIMAGE_RUNTIME to a type 2 runtime file"
    fi
fi
if selected wheel; then
    if python3 -m venv venv >/dev/null 2>&1; then
        check "wheel: build" build wheel
        check "wheel: pip install, import, scripts, uninstall" sh -c '
            venv/bin/pip install -q --no-deps out-wheel/*.whl && venv/bin/python -c "import hipkg; assert hipkg.VALUE == 1" &&
            venv/bin/hi | grep -q hi-from-package && venv/bin/pip uninstall -q -y hi && ! venv/bin/python -c "import hipkg" 2>/dev/null'
    else
        skip "wheel" "python3 -m venv does not work"
    fi
fi
if selected xbps; then
    check "xbps: build" build xbps
    check "xbps: rindex, install, query, remove (latest Void)" podman run --rm $PULL --network host -v "$WORK/out-xbps:/p:z" "$IMAGE_VOID" sh -ec '
        xbps-rindex -a /p/*.xbps >/dev/null; mkdir /rt; xbps-install -S -R /p -r /rt -y hi >/dev/null 2>&1
        xbps-query -r /rt -f hi | grep -q /usr/bin/hi; test -f /rt/etc/hi.conf; xbps-remove -r /rt -y hi >/dev/null 2>&1; ! test -e /rt/usr/bin/hi'
fi
if selected freebsd; then
    check "freebsd: build" build freebsd
    check "freebsd: manifest order, JSON and checksums" sh -c '
        f=$(ls out-freebsd/*.pkg); tar -tf "$f" | head -2 | tr "\n" " " | grep -q "+COMPACT_MANIFEST +MANIFEST"
        tar -xOf "$f" +MANIFEST | python3 -c "import sys,json; d=json.load(sys.stdin); assert d[\"name\"]==\"hi\" and d[\"abi\"]==\"FreeBSD:14:amd64\""'
    skip "freebsd: the real pkg tool accepts it" "FreeBSD is not available here"
fi

say ""
say "$passed passed, $failed failed, $skipped skipped. Log: $LOG"
[ "$failed" -eq 0 ]
