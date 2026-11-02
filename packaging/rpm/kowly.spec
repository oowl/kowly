# kowly for Fedora. packaging/package.sh rpm builds it: it stages the files
# with packaging/stage.sh and defines kowly_version, kowly_release,
# kowly_stage (the staged tree) and kowly_qt_version, so this only packs
# them.
Name: kowly
Version: %{kowly_version}
Release: %{kowly_release}%{?dist}
Summary: AirDrop for KDE Plasma
License: MIT
URL: https://github.com/oowl/kowly

# rpmbuild adds the libraries the binary links. The binary also uses Qt's
# private QML API, so it needs the very Qt it was built with.
Requires: owly
Requires: qt6-qtdeclarative%{?_isa} = %{kowly_qt_version}
Requires: kf6-kirigami
Recommends: qt6-qtwayland
Recommends: kf6-qqc2-desktop-style
Recommends: bluez
Recommends: xdg-utils
Suggests: dolphin

# The binary is stripped already, with no debug info to split off.
%global debug_package %{nil}

%description
Send files to and receive files from iPhones over AirDrop, with Owly's
radio service (owlyd) driving the Wi-Fi card. Drop files on the window or
use Dolphin's "Send with AirDrop"; accept or decline what comes in.

%install
cp -a %{kowly_stage}/. %{buildroot}/

%files
%{_bindir}/kowly
%{_datadir}/applications/io.github.oowl.kowly.desktop
%{_metainfodir}/io.github.oowl.kowly.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.oowl.kowly.svg
%dir %{_datadir}/kio
%dir %{_datadir}/kio/servicemenus
%{_datadir}/kio/servicemenus/kowly-send.desktop
%dir %{_docdir}/kowly
%doc %{_docdir}/kowly/README.md
%dir %{_datadir}/licenses/kowly
%license %{_datadir}/licenses/kowly/LICENSE
%license %{_datadir}/licenses/kowly/LICENSE-owly
