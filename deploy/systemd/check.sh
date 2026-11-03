#!/usr/bin/env bash
# Verifies the unit files and fails if a unit's systemd-analyze exposure score is above the limit.
# Usage: check.sh [max-exposure]   (default 2.0; lower is tighter)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
limit="${1:-2.0}"
root="${SMOKE_DIR:-$here/../../target/systemd-root}"
fail() { echo "FAIL: $*" >&2; exit 1; }

rm -rf "$root"
mkdir -p "$root/etc/systemd/system" "$root/usr/local/bin"
install -m 0755 /dev/null "$root/usr/local/bin/oxabl-cache-server"
cp "$here"/*.service "$root/etc/systemd/system/"
# --root hides the analyzer's own units; give it the targets every service depends on.
units="$(dirname "$(readlink -f "$(command -v systemd-analyze)")")/../example/systemd/system"
[ -f "$units/sysinit.target" ] || fail "no systemd unit directory next to $(command -v systemd-analyze); run inside nix develop"
mkdir -p "$root/usr/lib/systemd/system"
cp "$units"/*.target "$units"/*.slice "$root/usr/lib/systemd/system/"
root="$(cd "$root" && pwd)"

for unit in oxabl-cache.service oxabl-cache-init.service; do
  systemd-analyze verify --root="$root" "$unit" || fail "systemd-analyze verify $unit"
  report="$(systemd-analyze security --offline=true --root="$root" --no-pager "$unit")"
  score="$(printf '%s\n' "$report" | sed -n 's/^→ Overall exposure level for [^:]*: \([0-9.]*\) .*/\1/p')"
  [ -n "$score" ] || fail "no exposure score for $unit"
  awk -v s="$score" -v l="$limit" 'BEGIN { exit !(s <= l) }' || fail "$unit exposure $score is above $limit"
  echo "ok $unit (exposure $score, limit $limit)"
done
