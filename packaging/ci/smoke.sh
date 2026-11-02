#!/bin/sh
# Installs a built package on a fresh container of the distribution it's
# for, and starts the window offscreen: a screenshot means the QML loaded
# with that distribution's Qt and Kirigami. owly isn't in any of their
# repositories, so it's stood in for; the radio isn't tried.
#
#   packaging/ci/smoke.sh PACKAGE [SCREENSHOT]
set -eu
pkg=$(realpath "$1")
shot=${2:-/tmp/kowly-smoke.png}

case "$pkg" in
*.deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    standin=$(mktemp -d)
    mkdir "$standin/DEBIAN"
    printf 'Package: owly\nVersion: 0\nArchitecture: all\nMaintainer: none <none@example.com>\nDescription: stand-in for the smoke test\n' \
        > "$standin/DEBIAN/control"
    dpkg-deb --build "$standin" /tmp/owly-standin.deb >/dev/null
    apt-get install -y -qq /tmp/owly-standin.deb "$pkg" >/dev/null
    ;;
*.rpm)
    # Everything it requires but owly, then the package itself.
    rpm -qp --requires "$pkg" | grep -v -e '^owly' -e '^rpmlib(' -e '^/' \
        | tr '\n' '\0' | xargs -0 dnf install -y -q >/dev/null
    rpm -i --nodeps "$pkg"
    ;;
*.pkg.tar.zst)
    pacman -Syu --noconfirm -q >/dev/null
    pacman -U --noconfirm --assume-installed owly "$pkg" >/dev/null
    ;;
*)
    echo "smoke.sh: not a package: $pkg" >&2
    exit 2
    ;;
esac

home=$(mktemp -d)
HOME=$home XDG_CONFIG_HOME=$home/config QT_QPA_PLATFORM=offscreen \
    timeout 60 kowly --dev-name "Smoke Test" --dev-screenshot "$shot" --dev-wait 2000
[ -s "$shot" ] || { echo "smoke.sh: kowly took no screenshot" >&2; exit 1; }
echo "kowly started from $(basename "$pkg"): $shot"
