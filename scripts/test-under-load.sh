#!/usr/bin/env bash
# Runs client behaviour tests on one CPU shared with busy loops, as a slow, busy
# runner would: the test binary and the servers it starts all share CPU 0.
# Usage: test-under-load.sh [busy-loops] [test filter...]; no filter runs the whole suite.
set -euo pipefail

loops="${1:-4}"
shift || true
filters=("$@")

cargo build --locked -p oxabl-cache-server -p oxabl-cache-admin
binary="$(cargo test --locked -p oxabl-cache-client --test behaviour --no-run --message-format=json |
  jq -r 'select(.reason == "compiler-artifact" and .target.name == "behaviour" and .executable != null) | .executable')"
if [[ -z "$binary" ]]; then
  echo "test-under-load: the behaviour test binary was not found" >&2
  exit 2
fi

busy=()
stop() { kill "${busy[@]}" 2>/dev/null || true; }
trap stop EXIT
for _ in $(seq "$loops"); do
  taskset -c 0 sh -c 'while :; do :; done' &
  busy+=("$!")
done

taskset -c 0 timeout 1800 "$binary" --test-threads 1 "${filters[@]}"
