#!/bin/sh
# Builds kowly as a .deb for Debian, or an .rpm for Fedora, from this
# checkout. Build it on the release it's for: the binary links Qt's private
# QML API (qmlcachegen's compiled QML), so a package works only with the Qt
# it was built against, and depends on exactly that. packaging/ci/ sets up
# a container for each; scripts/build-packages.sh and CI run them.
#
#   packaging/package.sh deb|rpm
#
# The package lands in OUT ($PWD by default). RELEASE is the packaging's
# own revision (1).
set -eu
umask 022
ROOT=$(cd "$(dirname "$0")/.." && pwd)
FORMAT=${1:-}
case "$FORMAT" in
deb | rpm) ;;
*)
    echo "usage: packaging/package.sh deb|rpm" >&2
    exit 2
    ;;
esac
OUT=${OUT:-$PWD}
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -1)
RELEASE=${RELEASE:-1}
# Reproducible: dpkg-deb and rpmbuild date their files from it.
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH=$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)
fi
export SOURCE_DATE_EPOCH
[ -n "$VERSION" ] || { echo "packaging/package.sh: no version in Cargo.toml" >&2; exit 1; }

(cd "$ROOT" && cargo build --release --locked)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
STAGE=$WORK/root
if [ "$FORMAT" = deb ]; then
    export LICENSEDIR=/usr/share/doc/kowly
fi
DESTDIR=$STAGE TARGET=$TARGET "$ROOT/packaging/stage.sh"
strip --strip-unneeded "$STAGE/usr/bin/kowly"

if [ "$FORMAT" = deb ]; then
    ARCH=$(dpkg --print-architecture)
    # Debian wants the licenses in one copyright file.
    {
        cat "$ROOT/LICENSE"
        printf '\nParts are adapted from Owly (https://github.com/t4t5/owly):\n\n'
        cat "$ROOT/LICENSE-owly"
    } > "$STAGE/usr/share/doc/kowly/copyright"
    rm "$STAGE/usr/share/doc/kowly/LICENSE" "$STAGE/usr/share/doc/kowly/LICENSE-owly"
    # The libraries the binary links, as dpkg-shlibdeps finds them. It wants
    # a debian/control to read, and DEBIAN/ to tell where the root is.
    install -d "$STAGE/DEBIAN"
    mkdir "$WORK/debian"
    printf 'Source: kowly\n\nPackage: kowly\nArchitecture: any\n' > "$WORK/debian/control"
    SHLIBS=$(cd "$WORK" && dpkg-shlibdeps -O "$STAGE/usr/bin/kowly" | sed -n 's/^shlibs:Depends=//p')
    [ -n "$SHLIBS" ] || { echo "packaging/package.sh: dpkg-shlibdeps found no libraries" >&2; exit 1; }
    # The private QML API, pinned to this Qt.
    QT_ABI=$(dpkg-query -W -f '${Provides}\n' libqt6qml6 libqt6qml6t64 2>/dev/null \
        | tr ',' '\n' | sed -n 's/^ *\(qt6-declarative-private-abi.*\)$/\1/p' | head -1)
    [ -n "$QT_ABI" ] || { echo "packaging/package.sh: no qt6-declarative-private-abi to depend on" >&2; exit 1; }
    cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: kowly
Version: $VERSION-$RELEASE
Architecture: $ARCH
Maintainer: Jun Ouyang <ouyangjun1999@gmail.com>
Installed-Size: $(du -sk --exclude=DEBIAN "$STAGE" | cut -f1)
Depends: $SHLIBS, $QT_ABI, owly, qml6-module-qtquick, qml6-module-qtquick-controls, qml6-module-qtquick-layouts, qml6-module-qtquick-dialogs, qml6-module-qtquick-templates, qml6-module-qtquick-window, qml6-module-qtqml-workerscript, qml6-module-qtcore, qml6-module-org-kde-kirigami
Recommends: qt6-wayland, qml6-module-org-kde-desktop, bluez, xdg-utils
Suggests: dolphin
Section: net
Priority: optional
Homepage: https://github.com/oowl/kowly
Description: AirDrop for KDE Plasma
 Send files to and receive files from iPhones over AirDrop, with Owly's
 radio service (owlyd) driving the Wi-Fi card. Drop files on the window or
 use Dolphin's "Send with AirDrop"; accept or decline what comes in.
CONTROL
    PKG=$OUT/kowly_${VERSION}-${RELEASE}_$ARCH.deb
    dpkg-deb --root-owner-group --build "$STAGE" "$PKG" >/dev/null
else
    # The Qt the private API pins.
    QT_VERSION=$(rpm -q --qf '%{VERSION}' qt6-qtdeclarative)
    rpmbuild -bb --quiet \
        --define "_topdir $WORK/rpmbuild" \
        --define "_rpmdir $OUT" \
        --define "_build_name_fmt %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm" \
        --define "kowly_version $VERSION" \
        --define "kowly_release $RELEASE" \
        --define "kowly_stage $STAGE" \
        --define "kowly_qt_version $QT_VERSION" \
        --define "use_source_date_epoch_as_buildtime 1" \
        --define "clamp_mtime_to_source_date_epoch 1" \
        "$ROOT/packaging/rpm/kowly.spec"
    PKG=$(ls "$OUT"/kowly-"$VERSION"-"$RELEASE".*.rpm)
fi
echo "built $PKG"
