#!/bin/sh
# Drives the GUI against a headless peer over a veth pair, in a network
# namespace of its own, and saves screenshots: the GUI sending, then the GUI
# receiving, in Chinese and in English. No root, no radio, no display
# (offscreen).
#
#   scripts/gui-demo.sh OUTDIR
set -eu

if [ "${OWLY_DROP_NETNS:-}" != 1 ]; then
    exec env OWLY_DROP_NETNS=1 unshare --user --map-root-user --net -- "$0" "$@"
fi

out=$(realpath "${1:-.}")
here=$(cd "$(dirname "$0")/.." && pwd)
bin=${BIN:-$here/target/debug/owly-drop}
work=$(mktemp -d)
trap 'kill $peer 2>/dev/null || true; rm -rf "$work"' EXIT

ip link set lo up
ip link add owa type veth peer name owb
for i in owa owb; do
    echo 0 > /proc/sys/net/ipv6/conf/$i/accept_dad
    ip link set $i up
done
for i in owa owb; do
    until ip -6 addr show dev $i | grep -q 'fe80.*scope link'; do sleep 0.1; done
done

# Everything the runs write stays in $work.
export HOME="$work/home" XDG_CONFIG_HOME="$work/config" XDG_STATE_HOME="$work/state"
export OWLY_DROP_NO_BLE=1 QT_QPA_PLATFORM=offscreen QT_QPA_PLATFORMTHEME=kde QT_FORCE_STDERR_LOGGING=1
mkdir -p "$HOME"
head -c 20000000 /dev/urandom > "$work/IMG_0042.HEIC"

printf 'hello from the phone\n' > "$work/note.txt"
for lang in zh en; do
echo "== [$lang] GUI sends to a headless \"Alex's iPhone\""
rm -rf "$work/phone"
"$bin" --headless receive --iface owb --name "Alex's iPhone" --dir "$work/phone" --accept \
    > "$work/peer.log" 2>&1 &
peer=$!
until grep -q '^radio up' "$work/peer.log"; do sleep 0.1; done
OWLY_DROP_IFACE=owa "$bin" --dev-lang $lang --dev-up --dev-stage "$work/IMG_0042.HEIC" \
    --dev-send-to "Alex's iPhone" --dev-screenshot "$out/send-$lang.png" --dev-wait 6000
kill $peer; wait $peer 2>/dev/null || true
cmp -s "$work/IMG_0042.HEIC" "$work/phone/IMG_0042.HEIC" && echo "file arrived intact" || echo "FILE MISSING"

echo "== [$lang] a headless \"Alex's iPhone\" sends to the GUI, which asks"
OWLY_DROP_IFACE=owb "$bin" --dev-lang $lang --dev-up --dev-screenshot "$out/receive-$lang.png" --dev-wait 5000 &
gui=$!
sleep 1.5
"$bin" --headless send --iface owa --name "Alex's iPhone" --to "$(hostname)" --seconds 10 \
    "$work/note.txt" > "$work/send.log" 2>&1 || true
wait $gui || true
grep '^transfer' "$work/send.log" | tail -1
done
