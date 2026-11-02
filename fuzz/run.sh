#!/usr/bin/env bash
# Runs every fuzz target for N seconds (default 30) and prints its exec count.
# The libFuzzer binaries need the host C++ runtime, so they are built with the host
# compiler; only the cargo-fuzz binary comes from the devshell.
set -euo pipefail

seconds="${1:-30}"
cd "$(dirname "$0")"
cargo_fuzz="${CARGO_FUZZ:-$(nix develop .. --command which cargo-fuzz)}"
fuzz() { rustup run nightly "$cargo_fuzz" fuzz "$@"; }

status=0
for target in $(fuzz list); do
  if out="$(fuzz run "$target" -- -max_total_time="$seconds" 2>&1)"; then
    echo "$target: $(grep '^Done ' <<<"$out")"
  else
    echo "$out" >&2
    echo "fuzz: $target failed" >&2
    status=1
  fi
done
exit "$status"
