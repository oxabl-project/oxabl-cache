#!/usr/bin/env bash
# Formats, initialises (no backend, no state) and validates the hosted module and its examples,
# then lints them. Nothing is planned or applied. Usage: check.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
fail() { echo "FAIL: $*" >&2; exit 1; }

tofu fmt -check -recursive "$here" >/dev/null || fail "tofu fmt: run tofu fmt on the files it lists"

for dir in "$here/hosted" "$here/hosted/examples/prototype" "$here/hosted/examples/region"; do
  name="${dir#"$here"/}"
  tofu -chdir="$dir" init -backend=false -input=false -no-color >/dev/null || fail "tofu init $name"
  tofu -chdir="$dir" validate -no-color >/dev/null || fail "tofu validate $name"
  echo "ok $name"
done

tofu -chdir="$here/hosted" test -no-color >/dev/null || fail "tofu test (rerun without redirect to see the assertion)"
echo "ok tofu test"

if command -v tflint >/dev/null; then
  tflint --chdir="$here/hosted" --init >/dev/null
  tflint --chdir="$here/hosted" --recursive --minimum-failure-severity=warning || fail "tflint"
  echo "ok tflint"
fi
echo "tofu checks: ok"
