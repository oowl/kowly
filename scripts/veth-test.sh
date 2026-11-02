#!/bin/sh
# Sends files between two `kowly --headless` over a veth pair, in a
# network namespace of its own, the way owly's own tests do: no root, no
# radio, no iPhone. Three cases: accepted up front, accepted when asked, and
# declined when asked. Exits 0 when all pass.
set -eu

if [ "${KOWLY_NETNS:-}" != 1 ]; then
    exec env KOWLY_NETNS=1 unshare --user --map-root-user --net -- "$0" "$@"
fi

here=$(cd "$(dirname "$0")/.." && pwd)
bin=${BIN:-$here/target/debug/kowly}
work=$(mktemp -d)
rx=
trap '[ -n "$rx" ] && kill $rx 2>/dev/null; rm -rf "$work"' EXIT

. "$here/scripts/veth-lib.sh"
veth_pair owa owb
echo "veth pair up"

# Keep the test's TLS identity and Bluetooth out of the user's.
export XDG_STATE_HOME="$work/state" KOWLY_NO_BLE=1
head -c 3000000 /dev/urandom > "$work/photo.bin"
failed=0

# case NAME RECEIVER-FLAGS EXPECTED-SENDER-EXIT EXPECT-FILE(yes|no)
case_() {
    name=$1 flags=$2 want=$3 file=$4
    rm -rf "$work/in"
    "$bin" --headless receive --iface owb --name "Test Receiver" --dir "$work/in" $flags --once \
        > "$work/rx.log" 2>&1 &
    rx=$!
    if ! wait_for 30 "receiver" grep -q '^radio up' "$work/rx.log"; then
        sed 's/^/  rx| /' "$work/rx.log"
        failed=1
        kill $rx 2>/dev/null
        rx=
        return
    fi
    got=0
    timeout 60 "$bin" --headless send --iface owa --name "Test Sender" --to "Test Receiver" --seconds 15 \
        "$work/photo.bin" > "$work/tx.log" 2>&1 || got=$?
    # The receiver ends after one transfer, or never if none began.
    stop_after 15 $rx
    wait $rx 2>/dev/null || true
    rx=
    if [ -f "$work/in/photo.bin" ] && cmp -s "$work/photo.bin" "$work/in/photo.bin"; then arrived=yes; else arrived=no; fi
    last=$(grep '^transfer' "$work/tx.log" | tail -1 | sed 's/.*"phase":"\([a-z]*\)".*/\1/')
    if [ "$got" = "$want" ] && [ "$arrived" = "$file" ]; then
        echo "PASS  $name: sender ended '$last', file arrived: $arrived"
    else
        echo "FAIL  $name: sender exit $got (want $want), ended '$last', file arrived: $arrived (want $file)"
        sed 's/^/  tx| /' "$work/tx.log"; sed 's/^/  rx| /' "$work/rx.log"
        failed=1
    fi
}

case_ "auto-accept"       "--accept"     0 yes
case_ "accepted on ask"   "--answer yes" 0 yes
case_ "declined on ask"   "--answer no"  1 no
exit $failed
