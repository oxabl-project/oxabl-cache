#!/bin/sh
# Installs Oxabl Cache for the current user (or under PREFIX) and registers it with the desktop.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
install -Dm755 "$here/bin/oxabl-cache-desktop" "$prefix/bin/oxabl-cache-desktop"
install -Dm644 "$here/share/applications/org.oxabl.Cache.desktop" "$prefix/share/applications/org.oxabl.Cache.desktop"
install -Dm644 "$here/share/icons/hicolor/scalable/apps/org.oxabl.Cache.svg" \
  "$prefix/share/icons/hicolor/scalable/apps/org.oxabl.Cache.svg"
sed -i "s|^Exec=.*|Exec=$prefix/bin/oxabl-cache-desktop|" "$prefix/share/applications/org.oxabl.Cache.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qt "$prefix/share/icons/hicolor" || true
echo "Installed. Open Oxabl Cache from your launcher, or run $prefix/bin/oxabl-cache-desktop"
