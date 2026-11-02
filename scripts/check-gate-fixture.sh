#!/usr/bin/env bash
# Fails unless the dependency gate rejects the fixture crate for the right reason.
set -uo pipefail

output="$(scripts/check-server-deps.sh gate-fixture-server scripts/gate-fixture/Cargo.toml 2>&1)"
status=$?
if [[ $status -ne 1 ]] || ! grep -q "forbidden crate oxabl-cache-crypto" <<<"$output"; then
  echo "check-gate-fixture: the gate did not reject the fixture (exit $status):" >&2
  echo "$output" >&2
  exit 1
fi
echo "check-gate-fixture: the gate rejects the fixture"
