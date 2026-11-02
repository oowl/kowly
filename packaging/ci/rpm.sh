#!/bin/sh
# In a fedora:44 container, as root: installs what the build needs and runs
# packaging/package.sh rpm.
set -eu
dnf install -y -q --setopt=install_weak_deps=False \
    cargo rust gcc-c++ rpm-build qt6-qtbase-devel qt6-qtdeclarative-devel
exec "$(dirname "$0")/../package.sh" rpm
