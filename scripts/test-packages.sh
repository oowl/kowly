#!/bin/sh
# Installs each package in dist/ on a fresh container of its distribution
# and starts it (packaging/ci/smoke.sh). Screenshots land beside them, as
# dist/smoke-<format>.png. Needs docker or podman.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
engine=${ENGINE:-$(command -v podman || command -v docker || true)}
[ -n "$engine" ] || { echo "test-packages.sh: needs docker or podman" >&2; exit 1; }
failed=0
for pkg in "$here"/dist/*.deb "$here"/dist/*.rpm "$here"/dist/*.pkg.tar.zst; do
    [ -e "$pkg" ] || continue
    case "$pkg" in
    *.deb) image=debian:trixie format=deb ;;
    *.rpm) image=fedora:44 format=rpm ;;
    *) image=archlinux:latest format=arch ;;
    esac
    echo "== $format ($image)"
    if "$engine" run --rm -v "$here:/src:ro" -v "$here/dist:/dist" "$image" sh -c "
        /src/packaging/ci/smoke.sh '/dist/$(basename "$pkg")' /dist/smoke-$format.png
        chown $(id -u):$(id -g) /dist/smoke-$format.png"; then
        :
    else
        echo "FAIL: $format"
        failed=1
    fi
done
exit $failed
