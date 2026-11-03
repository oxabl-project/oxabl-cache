#!/usr/bin/env bash
# Builds the image twice without a cache and fails unless both builds have the same digest.
# Usage: reproducible.sh   (needs docker buildx; takes a few minutes)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
work="${SMOKE_DIR:-$root/target/container-reproducible}"
epoch="${SOURCE_DATE_EPOCH:-1700000000}"
rm -rf "$work"
mkdir -p "$work"

for run in a b; do
  docker buildx build --no-cache --provenance=false --sbom=false \
    --build-arg SOURCE_DATE_EPOCH="$epoch" -f "${CONTAINERFILE:-$here/Containerfile}" \
    --output "type=oci,dest=$work/$run.tar,rewrite-timestamp=true" "$root" >"$work/$run.log" 2>&1
done

digest() { tar -xOf "$work/$1.tar" index.json | sed -n 's/.*"digest":"\(sha256:[0-9a-f]*\)".*/\1/p' | head -1; }
a="$(digest a)"
b="$(digest b)"
[ -n "$a" ] || { echo "FAIL: no digest" >&2; exit 1; }
[ "$a" = "$b" ] || { echo "FAIL: builds differ: $a $b" >&2; exit 1; }
echo "reproducible: ok ($a)"
