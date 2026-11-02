#!/bin/sh
# In an archlinux container, as root: builds packaging/arch/PKGBUILD with
# makepkg, as a user of its own, from this checkout rather than the release
# tarball on GitHub, into OUT. owly is in the AUR, not the repositories, and
# only needed at runtime, so it's left out (makepkg -d).
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${OUT:-$PWD}
pacman -Syu --noconfirm --needed -q base-devel rust \
    qt6-base qt6-declarative kirigami hicolor-icon-theme >/dev/null
id builder >/dev/null 2>&1 || useradd -m -u 1000 builder
pkgver=$(sed -n 's/^pkgver=//p' "$ROOT/packaging/arch/PKGBUILD")
work=$(mktemp -d)
cp "$ROOT/packaging/arch/PKGBUILD" "$work/"
# The tarball the PKGBUILD names, as GitHub would make it, from the checkout.
tar -C "$ROOT" --exclude=./.git --exclude=./target --exclude=./dist \
    --transform "s|^\./|kowly-$pkgver/|;s|^\.$|kowly-$pkgver|" \
    -czf "$work/kowly-$pkgver.tar.gz" .
chown -R builder "$work"
[ ! -d /home/builder/.cargo ] || chown -R builder /home/builder/.cargo
packager=${PACKAGER:-Jun Ouyang <ouyangjun1999@gmail.com>}
su builder -c "cd '$work' && PACKAGER='$packager' makepkg -d --noconfirm --skipchecksums" >&2
for pkg in "$work"/*.pkg.tar.zst; do
    case "$pkg" in *-debug-*) continue ;; esac
    cp "$pkg" "$OUT/"
    echo "built $OUT/$(basename "$pkg")"
done
