#!/bin/sh
# Drives the GUI against a headless peer over a veth pair, in a network
# namespace of its own, and saves screenshots: the GUI sending, then the GUI
# receiving, in Chinese and in English. No root, no radio, no display
# (offscreen).
#
#   scripts/gui-demo.sh OUTDIR     screenshots as send-{zh,en}.png and receive-{zh,en}.png
#
# docs/screenshots/ holds the README's, made this way.
set -eu

if [ "${KOWLY_NETNS:-}" != 1 ]; then
    exec env KOWLY_NETNS=1 unshare --user --map-root-user --net -- "$0" "$@"
fi

out=$(realpath "${1:-.}")
here=$(cd "$(dirname "$0")/.." && pwd)
bin=${BIN:-$here/target/debug/kowly}
work=$(mktemp -d)
trap 'kill $peer 2>/dev/null || true; rm -rf "$work"' EXIT

# The GUI's end is awdl0, as under owlyd, so the screenshots read as they
# would on a real radio; the headless peer's is peer0.
. "$here/scripts/veth-lib.sh"
veth_pair awdl0 peer0

# Everything the runs write stays in $work.
export HOME="$work/home" XDG_CONFIG_HOME="$work/config" XDG_STATE_HOME="$work/state"
export KOWLY_NO_BLE=1 QT_QPA_PLATFORM=offscreen QT_QPA_PLATFORMTHEME=kde QT_FORCE_STDERR_LOGGING=1
mkdir -p "$HOME"
head -c 20000000 /dev/urandom > "$work/IMG_0042.HEIC"
# What the GUI calls itself, rather than this machine's host name.
name="Linux Laptop"

printf 'hello from the phone\n' > "$work/note.txt"
for lang in zh en; do
echo "== [$lang] GUI sends to a headless \"Alex's iPhone\""
rm -rf "$work/phone"
"$bin" --headless receive --iface peer0 --name "Alex's iPhone" --dir "$work/phone" --accept \
    > "$work/peer.log" 2>&1 &
peer=$!
wait_for 30 "headless peer" grep -q '^radio up' "$work/peer.log"
KOWLY_IFACE=awdl0 "$bin" --dev-lang $lang --dev-name "$name" --dev-up --dev-stage "$work/IMG_0042.HEIC" \
    --dev-send-to "Alex's iPhone" --dev-screenshot "$out/send-$lang.png" --dev-wait 6000
kill $peer; wait $peer 2>/dev/null || true
cmp -s "$work/IMG_0042.HEIC" "$work/phone/IMG_0042.HEIC" && echo "file arrived intact" || echo "FILE MISSING"

echo "== [$lang] a headless \"Alex's iPhone\" sends to the GUI, which asks"
KOWLY_IFACE=awdl0 "$bin" --dev-lang $lang --dev-name "$name" --dev-up --dev-screenshot "$out/receive-$lang.png" --dev-wait 5000 &
gui=$!
sleep 1.5
"$bin" --headless send --iface peer0 --name "Alex's iPhone" --to "$name" --seconds 10 \
    "$work/note.txt" > "$work/send.log" 2>&1 || true
wait $gui || true
grep '^transfer' "$work/send.log" | tail -1
done
