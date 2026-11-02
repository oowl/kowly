#!/bin/sh
# Lays kowly's files out under DESTDIR as a system package installs them,
# from a release build. Every package uses it: packaging/package.sh (deb,
# rpm) and packaging/arch/PKGBUILD.
#
#   DESTDIR=/tmp/root packaging/stage.sh
#
# TARGET is cargo's target directory (target/ by default). LICENSEDIR is
# where the licenses go: /usr/share/licenses/kowly, but Debian keeps them
# with the docs.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
: "${DESTDIR:?set DESTDIR}"
TARGET=${TARGET:-$ROOT/target}
LICENSEDIR=${LICENSEDIR:-/usr/share/licenses/kowly}
DOCDIR=${DOCDIR:-/usr/share/doc/kowly}
APP_ID=io.github.oowl.kowly
LINUX=$ROOT/packaging/linux

install -Dm755 "$TARGET/release/kowly" "$DESTDIR/usr/bin/kowly"
install -Dm644 "$LINUX/$APP_ID.desktop" "$DESTDIR/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$LINUX/$APP_ID.metainfo.xml" "$DESTDIR/usr/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "$LINUX/$APP_ID.svg" "$DESTDIR/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
# Dolphin's "Send with AirDrop". Outside the user's own directories, KIO
# needs no executable bit.
install -Dm644 "$LINUX/kowly-send.desktop" "$DESTDIR/usr/share/kio/servicemenus/kowly-send.desktop"
install -Dm644 "$ROOT/README.md" "$DESTDIR$DOCDIR/README.md"
install -Dm644 "$ROOT/LICENSE" "$DESTDIR$LICENSEDIR/LICENSE"
install -Dm644 "$ROOT/LICENSE-owly" "$DESTDIR$LICENSEDIR/LICENSE-owly"
