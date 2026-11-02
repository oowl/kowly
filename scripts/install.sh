#!/bin/sh
# Builds owly-drop and installs it for this user: no root. Owly itself
# (owlyd) is a separate, system-wide install: `yay -S owly`.
#
#   scripts/install.sh            build and install
#   scripts/install.sh uninstall  remove what install put in place
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
bin="$HOME/.local/bin/owly-drop"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
menus="${XDG_DATA_HOME:-$HOME/.local/share}/kio/servicemenus"

refresh() {
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$apps" || true
}

if [ "${1:-}" = uninstall ]; then
    rm -f "$bin" "$apps/owly-drop.desktop" "$menus/owly-drop-send.desktop"
    refresh
    echo "removed owly-drop (received files and settings are kept)"
    exit 0
fi

cd "$here"
cargo build --release
install -Dm755 target/release/owly-drop "$bin"
# The full path, so neither needs ~/.local/bin on the session's PATH.
mkdir -p "$apps" "$menus"
sed "s|^Exec=owly-drop|Exec=$bin|" packaging/owly-drop.desktop > "$apps/owly-drop.desktop"
sed "s|^Exec=owly-drop|Exec=$bin|" packaging/owly-drop-send.desktop > "$menus/owly-drop-send.desktop"
# KIO only runs service menus in the user's own directory when executable.
chmod +x "$menus/owly-drop-send.desktop"
refresh

echo "installed:"
echo "  $bin"
echo "  $apps/owly-drop.desktop             (AirDrop in the app launcher)"
echo "  $menus/owly-drop-send.desktop  (Dolphin: right-click → Send with AirDrop)"
if ! busctl --system status io.github.t4t5.Owly >/dev/null 2>&1 \
    && [ ! -e /usr/share/dbus-1/system-services/io.github.t4t5.Owly.service ]; then
    echo
    echo "Owly isn't installed yet. AirDrop needs it for the radio:"
    echo "  yay -S owly"
fi
