#!/bin/sh
# In a debian:trixie container, as root: installs what the build needs and
# runs packaging/package.sh deb. Debian's Rust is older than Owly's 1.88,
# so Rust comes from rustup.
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
    build-essential ca-certificates curl dpkg-dev pkg-config \
    qt6-base-dev qt6-declarative-dev qt6-declarative-dev-tools >/dev/null
if [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable
fi
. "$HOME/.cargo/env"
export QMAKE=/usr/lib/qt6/bin/qmake
exec "$(dirname "$0")/../package.sh" deb
