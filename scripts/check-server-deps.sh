#!/usr/bin/env bash
# Fails when the normal dependency tree of a server package contains a client-side crate.
# Usage: check-server-deps.sh [package] [manifest-path]
set -euo pipefail

package="${1:-oxabl-cache-server}"
manifest="${2:-Cargo.toml}"
forbidden=(oxabl-cache-crypto oxabl-cache-client age)

tree="$(cargo tree --locked --manifest-path "$manifest" -p "$package" -e normal --prefix none)" || {
  echo "check-server-deps: cargo tree failed for $package" >&2
  exit 2
}

status=0
for crate in "${forbidden[@]}"; do
  if grep -qE "^${crate} v" <<<"$tree"; then
    echo "check-server-deps: $package depends on forbidden crate $crate" >&2
    status=1
  fi
done
exit "$status"
