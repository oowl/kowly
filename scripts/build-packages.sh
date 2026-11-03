#!/bin/sh
# Builds kowly's packages in containers, into dist/: the .deb for Debian 13,
# the .rpm for Fedora 44 and the Arch package. Needs docker or podman.
#
#   scripts/build-packages.sh [deb] [rpm] [arch]     (all three by default)
#
# Each builds the working tree, and keeps its cargo cache in a volume,
# kowly-<format>-cargo.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
out=$here/dist
mkdir -p "$out"
engine=${ENGINE:-$(command -v podman || command -v docker || true)}
[ -n "$engine" ] || { echo "build-packages.sh: needs docker or podman" >&2; exit 1; }
formats=${*:-deb rpm arch}

for format in $formats; do
    case $format in
    deb) image=debian:trixie cache=/root/.cargo ;;
    rpm) image=fedora:44 cache=/root/.cargo ;;
    arch) image=archlinux:latest cache=/home/builder/.cargo ;;
    *) echo "build-packages.sh: no such format $format" >&2; exit 2 ;;
    esac
    echo "== $format ($image)"
    # The checkout is mounted read-only and copied in, without target/ and
    # dist/, so the container builds from scratch and writes nothing back
    # but the package.
    "$engine" run --rm \
        -v "$here:/src:ro" -v "$out:/out" -v "kowly-$format-cargo:$cache" \
        -e OUT=/out \
        "$image" sh -c "
            set -e
            mkdir /build
            tar -C /src --exclude=./target --exclude=./dist -cf - . | tar -C /build -xf -
            /build/packaging/ci/$format.sh
            chown $(id -u):$(id -g) /out/*
        "
done
ls -l "$out"
