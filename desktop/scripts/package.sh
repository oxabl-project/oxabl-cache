#!/usr/bin/env bash
# Builds the release binary against glibc 2.34, so it runs on any current Linux desktop, and
# packs it with its desktop entry, icon and installer into target/dist/.
# Usage: scripts/package.sh [version]
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"
version="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
target=x86_64-unknown-linux-gnu
RUSTFLAGS="-L $(pkg-config --variable=libdir xkbcommon) -C link-arg=-Wl,--allow-shlib-undefined" \
  cargo zigbuild --locked --release --target "$target.2.34"
binary="target/$target/release/oxabl-cache-desktop"
newest="$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)"
[ "$newest" = "GLIBC_2.34" ] || { echo "the binary needs $newest, more than glibc 2.34" >&2; exit 1; }
name="oxabl-cache-desktop-$version-x86_64-linux"
stage="target/dist/$name"
rm -rf "$stage"
install -Dm755 "$binary" "$stage/bin/oxabl-cache-desktop"
install -Dm644 packaging/org.oxabl.Cache.desktop "$stage/share/applications/org.oxabl.Cache.desktop"
install -Dm644 packaging/org.oxabl.Cache.svg "$stage/share/icons/hicolor/scalable/apps/org.oxabl.Cache.svg"
install -Dm755 packaging/install.sh "$stage/install.sh"
tar -C target/dist -czf "target/dist/$name.tar.gz" "$name"
sha256sum "target/dist/$name.tar.gz" | sed "s|target/dist/||" > "target/dist/$name.tar.gz.sha256"
echo "target/dist/$name.tar.gz"
