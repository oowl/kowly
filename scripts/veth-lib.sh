# Sourced by veth-test.sh and gui-demo.sh, inside their network namespace:
# a veth pair with link-local addresses, and waits that give up.

# wait_for SECONDS WHAT COMMAND...: runs COMMAND until it succeeds; fails,
# naming WHAT, once SECONDS have passed.
wait_for() {
    limit=$1 what=$2
    shift 2
    end=$(($(date +%s) + limit))
    until "$@"; do
        if [ "$(date +%s)" -ge "$end" ]; then
            echo "FAIL: no $what after ${limit}s" >&2
            return 1
        fi
        sleep 0.1
    done
}

has_link_local() {
    ip -6 addr show dev "$1" 2>/dev/null | grep -q 'fe80.*scope link'
}

# veth_pair A B: a veth pair A–B, up, with link-local addresses and DAD off.
veth_pair() {
    ip link set lo up
    # A container may come with IPv6 off, and a new namespace takes its
    # defaults from there.
    for conf in all default; do
        echo 0 > /proc/sys/net/ipv6/conf/$conf/disable_ipv6 2>/dev/null || true
    done
    ip link add "$1" type veth peer name "$2"
    for i in "$1" "$2"; do
        echo 0 > /proc/sys/net/ipv6/conf/$i/disable_ipv6
        echo 0 > /proc/sys/net/ipv6/conf/$i/accept_dad
        ip link set "$i" up
    done
    for i in "$1" "$2"; do
        wait_for 10 "IPv6 link-local address on $i" has_link_local "$i" || {
            ip addr >&2
            return 1
        }
    done
}

# stop_after SECONDS PID: kills PID if it's still running after SECONDS.
stop_after() {
    (sleep "$1" && kill "$2" 2>/dev/null) &
}
