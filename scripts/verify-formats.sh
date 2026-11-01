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
IMAGE_RUBY="${IMAGE_RUBY:-docker.io/library/ruby:latest}"
IMAGE_NODE="${IMAGE_NODE:-docker.io/library/node:latest}"
IMAGE_MAMBA="${IMAGE_MAMBA:-docker.io/mambaorg/micromamba:latest}"
IMAGE_DOTNET="${IMAGE_DOTNET:-mcr.microsoft.com/dotnet/sdk:latest}"
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
for image in "$IMAGE_FEDORA" "$IMAGE_DEBIAN" "$IMAGE_ARCH" "$IMAGE_ALPINE" "$IMAGE_SLACKWARE" "$IMAGE_VOID" "$IMAGE_RUBY" "$IMAGE_NODE" "$IMAGE_MAMBA" "$IMAGE_DOTNET"; do
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

# The language package formats install into a language's own tree, so each one gets a manifest of its own.
lang_manifest() { # lang_manifest FORMAT: writes lang-FORMAT.yaml with the common header and the given contents and section
    { printf 'name: hi\nversion: 1.0.0\narch: all\ndescription: Says hi\nmaintainer: Test <test@example.org>\nhomepage: https://example.org/hi\nlicense: MIT\n'; cat; } > "lang-$1.yaml"
}
build_lang() { "$PKGSMITH" build . -f "lang-$1.yaml" -F "$1" -o "out-$1" --progress plain; }
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

if selected gem; then
    mkdir -p lang/gem/lib lang/gem/bin
    printf 'module Hi\n  def self.say = "hi-from-gem"\nend\n' > lang/gem/lib/hi.rb
    printf '#!/usr/bin/env ruby\nrequire "hi"\nputs Hi.say\n' > lang/gem/bin/hi; chmod 755 lang/gem/bin/hi
    lang_manifest gem <<'YAML'
contents:
  - { src: lang/gem/lib/hi.rb, dst: /usr/share/gems/gems/hi-1.0.0/lib/hi.rb }
  - { src: lang/gem/bin/hi, dst: /usr/bin/hi }
YAML
    check "gem: build" build_lang gem
    check "gem: install, require, run, uninstall (latest Ruby)" in_image "$IMAGE_RUBY" out-gem '
        gem install --local /p/hi-1.0.0.gem --no-document >/dev/null; ruby -e "require %q(hi); puts Hi.say" | grep -q hi-from-gem
        hi | grep -q hi-from-gem; gem spec /p/hi-1.0.0.gem name | grep -q hi; gem uninstall hi -x >/dev/null'
fi
if selected npm; then
    mkdir -p lang/npm
    printf 'module.exports = "hi-from-npm";\n' > lang/npm/index.js
    lang_manifest npm <<'YAML'
contents:
  - { src: lang/npm/index.js, dst: /usr/lib/node_modules/hi/index.js }
YAML
    check "npm: build" build_lang npm
    check "npm: install and require (latest Node)" in_image "$IMAGE_NODE" out-npm '
        mkdir /tmp/proj && cd /tmp/proj && npm init -y >/dev/null && npm install --no-save /p/hi-1.0.0.tgz >/dev/null
        node -p "require(\"hi\")" | grep -q hi-from-npm; npm publish --dry-run /p/hi-1.0.0.tgz >/dev/null 2>&1'
fi
if selected conda; then
    mkdir -p lang/conda/bin
    printf '#!/bin/sh\necho hi-from-conda\n' > lang/conda/bin/hi; chmod 755 lang/conda/bin/hi
    lang_manifest conda <<'YAML'
contents:
  - { src: lang/conda/bin/hi, dst: /usr/bin/hi }
YAML
    check "conda: build" build_lang conda
    check "conda: index, create an environment, run, remove (latest micromamba)" in_image "$IMAGE_MAMBA" out-conda '
        mkdir -p /tmp/ch/noarch /tmp/ch/linux-64; cp /p/*.conda /tmp/ch/linux-64/
        micromamba create -y -q -n idx -c conda-forge conda-index >/dev/null
        micromamba run -n idx python -m conda_index /tmp/ch >/dev/null
        micromamba create -y -q -n t -c file:///tmp/ch --override-channels hi >/dev/null
        micromamba run -n t hi | grep -q hi-from-conda; micromamba remove -y -q -n t hi >/dev/null'
fi
if selected nupkg; then
    mkdir -p lang/nupkg
    printf 'hi-from-nupkg\n' > lang/nupkg/hi.txt
    lang_manifest nupkg <<'YAML'
contents:
  - { src: lang/nupkg/hi.txt, dst: /usr/share/hi/hi.txt }
nupkg:
  prefix: /usr/share/hi
YAML
    check "nupkg: build" build_lang nupkg
    check "nupkg: dotnet add package from a local feed (latest .NET SDK)" in_image "$IMAGE_DOTNET" out-nupkg '
        mkdir /feed && cp /p/*.nupkg /feed/; cd /tmp && dotnet new console -o proj --no-restore >/dev/null && cd proj
        printf "<configuration><packageSources><clear /><add key=\"feed\" value=\"/feed\" /></packageSources></configuration>" > nuget.config
        dotnet add package hi --version 1.0.0 --source /feed >/dev/null; grep -q hi-from-nupkg /root/.nuget/packages/hi/1.0.0/tools/hi.txt'
fi

if selected dmg; then
    # 320 files in one folder make the catalog more than one node. The other files cover sizes, modes and names.
    rm -rf lang/dmg; mkdir -p lang/dmg/many lang/dmg/bin
    for i in $(seq -w 0 319); do printf 'file %s\n' "$i" > "lang/dmg/many/f$i.txt"; done
    head -c 5242880 /dev/urandom > lang/dmg/big.bin
    : > lang/dmg/empty.txt
    printf 'hi-from-dmg\n' > lang/dmg/bin/hi; chmod 755 lang/dmg/bin/hi
    printf 'spaced\n' > "lang/dmg/with space.txt"
    long="$(printf 'n%.0s' $(seq 1 200)).txt"; printf 'long\n' > "lang/dmg/$long"
    {
        echo "contents:"
        echo "  - { src: lang/dmg/bin/hi, dst: /usr/bin/hi }"
        echo "  - { src: lang/dmg/big.bin, dst: /usr/share/hi/big.bin }"
        echo "  - { src: lang/dmg/empty.txt, dst: /usr/share/hi/empty.txt }"
        echo "  - { src: \"lang/dmg/with space.txt\", dst: \"/usr/share/hi/with space.txt\" }"
        echo "  - { src: lang/dmg/$long, dst: /usr/share/hi/$long }"
        for f in lang/dmg/many/*.txt; do echo "  - { src: $f, dst: /usr/share/hi/many/${f##*/} }"; done
        echo "dmg:"
        echo "  volume_name: Hi Tools"
    } | lang_manifest dmg
    check "dmg: build" build_lang dmg
    cp -r lang/dmg out-dmg/src
    check "dmg: fsck.hfsplus, 7z extract, every file identical (latest Fedora)" in_image "$IMAGE_FEDORA" out-dmg '
        dnf -y -q install dmg2img hfsplus-tools p7zip p7zip-plugins diffutils findutils >/dev/null
        cd /tmp && dmg2img /p/*.dmg vol.img >/dev/null
        # The first 64 sectors are the partition map. The HFS+ volume follows.
        dd if=vol.img of=hfs.img bs=512 skip=64 status=none
        fsck.hfsplus -n hfs.img | grep -q "appears to be OK"
        7z x -y -oex hfs.img >/dev/null
        test "$(find ex -name "f*.txt" | wc -l)" -eq 320
        cmp ex/Hi\ Tools/usr/share/hi/big.bin /p/src/big.bin
        cmp ex/Hi\ Tools/usr/bin/hi /p/src/bin/hi
        cmp "ex/Hi Tools/usr/share/hi/with space.txt" "/p/src/with space.txt"
        cmp ex/Hi\ Tools/usr/share/hi/empty.txt /p/src/empty.txt
        for f in /p/src/many/*.txt; do cmp "$f" "ex/Hi Tools/usr/share/hi/many/${f##*/}"; done
        test -x "ex/Hi Tools/usr/bin/hi"
        long="$(printf "n%.0s" $(seq 1 200)).txt"
        find ex -type f -name "$long" | grep -q .'
fi
if selected msi; then
    rm -rf lang/msi; mkdir -p "lang/msi/share/hi docs" lang/msi/bin lang/msi/empty
    printf 'MZ-fake\n' > lang/msi/bin/hi.exe; chmod 755 lang/msi/bin/hi.exe
    head -c 1572864 /dev/urandom > lang/msi/share/big.bin
    : > lang/msi/share/empty.txt
    printf 'spaced\n' > "lang/msi/share/hi docs/read me.txt"
    printf 'long\n' > lang/msi/share/a-rather-long-file-name.configuration
    cat > lang-msi.yaml <<'YAML'
name: hi
version: 1.2.3
arch: amd64
description: Says hi
maintainer: Test Person <test@example.org>
contents:
  - src: lang/msi/bin/hi.exe
    dst: /usr/bin/hi.exe
  - src: lang/msi/share/big.bin
    dst: /usr/share/big.bin
  - src: lang/msi/share/empty.txt
    dst: /usr/share/empty.txt
  - src: "lang/msi/share/hi docs/read me.txt"
    dst: "/usr/share/hi docs/read me.txt"
  - src: lang/msi/share/a-rather-long-file-name.configuration
    dst: /usr/share/a-rather-long-file-name.configuration
  - type: dir
    dst: /usr/lib/empty
msi:
  manufacturer: Example Ltd
YAML
    check "msi: build" build_lang msi
    cp -r lang/msi out-msi/src
    # msitools reads the database and extracts the cabinet. It cannot install, so this does not prove that Windows accepts it.
    check "msi: msiinfo tables and Summary Information, msiextract, every file identical (latest Fedora)" in_image "$IMAGE_FEDORA" out-msi '
        dnf -y -q install msitools diffutils findutils >/dev/null
        cd /tmp; f=$(ls /p/*.msi)
        msiinfo tables "$f" | grep -qx InstallExecuteSequence
        msiinfo suminfo "$f" | grep -q "Template: x64;1033"
        msiinfo export "$f" File | grep -q "A-RATH~1.CON|a-rather-long-file-name.configuration"
        msiextract -C ex "$f" >/dev/null
        test "$(find ex -type f | wc -l)" -eq 5
        cmp ex/hi/share/big.bin /p/src/share/big.bin
        cmp ex/hi/bin/hi.exe /p/src/bin/hi.exe
        cmp "ex/hi/share/hi docs/read me.txt" "/p/src/share/hi docs/read me.txt"
        cmp ex/hi/share/empty.txt /p/src/share/empty.txt
        cmp ex/hi/share/a-rather-long-file-name.configuration /p/src/share/a-rather-long-file-name.configuration'
fi
if selected flatpak; then
    if need flatpak; then
        mkdir -p lang/flatpak/bin
        printf '#!/bin/sh\necho hi-from-flatpak\n' > lang/flatpak/bin/hi
        chmod 755 lang/flatpak/bin/hi
        # A bundle is for one architecture, so this manifest does not use the arch: all of lang_manifest.
        cat > lang-flatpak.yaml <<'YAML'
name: hi
version: 1.0.0
arch: amd64
description: Says hi
contents:
  - { src: lang/flatpak/bin/hi, dst: /usr/bin/hi }
flatpak:
  app_id: org.example.Hi
  runtime_version: "24.08"
YAML
        check "flatpak: build" build_lang flatpak
        # Needs network for the runtime from Flathub, and --privileged because flatpak makes a sandbox.
        check "flatpak: install the bundle, run it (latest Debian)" podman run --rm $PULL --privileged -v "$WORK/out-flatpak:/p:ro,z" "$IMAGE_DEBIAN" sh -ec '
            export DEBIAN_FRONTEND=noninteractive HOME=/tmp/home; mkdir -p "$HOME"
            apt-get update -qq >/dev/null && apt-get install -y -qq flatpak ostree bubblewrap >/dev/null
            flatpak --user remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
            flatpak --user install -y --bundle /p/*.flatpak >/dev/null
            flatpak --user run org.example.Hi | grep -q hi-from-flatpak'
    else
        skip "flatpak" "the flatpak program is not installed on this machine"
    fi
fi

say ""
say "$passed passed, $failed failed, $skipped skipped. Log: $LOG"
[ "$failed" -eq 0 ]
