#!/bin/sh
# Builds kowly and installs it for this user: no root. Owly itself (owlyd)
# is a separate, system-wide install: `yay -S owly`. For a system-wide
# kowly, use a package instead (packaging/).
#
#   scripts/install.sh            build and install
#   scripts/install.sh uninstall  remove what install put in place
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
app_id=io.github.oowl.kowly
data=${XDG_DATA_HOME:-$HOME/.local/share}
bin="$HOME/.local/bin/kowly"
desktop="$data/applications/$app_id.desktop"
icon="$data/icons/hicolor/scalable/apps/$app_id.svg"
menu="$data/kio/servicemenus/kowly-send.desktop"

refresh() {
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$data/applications" || true
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
}

# What versions before the rename to kowly installed.
remove_old() {
    rm -f "$HOME/.local/bin/owly-drop" "$data/applications/owly-drop.desktop" \
        "$data/kio/servicemenus/owly-drop-send.desktop"
}

if [ "${1:-}" = uninstall ]; then
    rm -f "$bin" "$desktop" "$icon" "$menu"
    remove_old
    refresh
    echo "removed kowly (received files and settings are kept)"
    exit 0
fi

cd "$here"
cargo build --release
remove_old
install -Dm755 target/release/kowly "$bin"
install -Dm644 "packaging/linux/$app_id.svg" "$icon"
# The full path, so neither needs ~/.local/bin on the session's PATH.
mkdir -p "$(dirname "$desktop")" "$(dirname "$menu")"
sed "s|^Exec=kowly|Exec=$bin|" "packaging/linux/$app_id.desktop" > "$desktop"
sed "s|^Exec=kowly|Exec=$bin|" packaging/linux/kowly-send.desktop > "$menu"
# KIO only runs service menus in the user's own directory when executable.
chmod +x "$menu"
refresh

echo "installed:"
echo "  $bin"
echo "  $desktop  (kowly in the app launcher)"
echo "  $menu  (Dolphin: right-click → Send with AirDrop)"
if ! busctl --system status io.github.t4t5.Owly >/dev/null 2>&1 \
    && [ ! -e /usr/share/dbus-1/system-services/io.github.t4t5.Owly.service ]; then
    echo
    echo "Owly isn't installed yet. kowly needs it for the radio:"
    echo "  yay -S owly"
fi
