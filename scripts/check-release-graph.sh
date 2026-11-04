#!/usr/bin/env bash
# Fails when release-please's cargo-workspace plugin could not read the workspace:
# a member without a literal version, a member missing from the release config,
# or a cycle among members, dev-dependencies included.
# Usage: check-release-graph.sh [manifest-path] [release-please-config]
set -euo pipefail

manifest="${1:-Cargo.toml}"
config="${2:-release-please-config.json}"

metadata="$(cargo metadata --locked --format-version 1 --no-deps --manifest-path "$manifest")" || {
  echo "check-release-graph: cargo metadata failed" >&2
  exit 2
}
root="$(jq -r '.workspace_root' <<<"$metadata")"
packages="$(jq -r '.packages | length' <<<"$metadata")"
if [[ "$packages" -eq 0 ]]; then
  echo "check-release-graph: no workspace members found" >&2
  exit 2
fi

status=0
while IFS=$'\t' read -r name path; do
  dir="${path%/Cargo.toml}"
  rel="${dir#"$root"/}"
  if ! grep -qE '^version = "[^"]+"' "$path"; then
    echo "check-release-graph: $name has no literal version in $rel/Cargo.toml" >&2
    status=1
  fi
  if ! jq -e --arg p "$rel" '.packages | has($p)' "$root/$config" >/dev/null; then
    echo "check-release-graph: $rel is not a package in $config" >&2
    status=1
  fi
done < <(jq -r '.packages[] | [.name, .manifest_path] | @tsv' <<<"$metadata")

cycle="$(jq -r '
  [.packages[].name] as $members
  | [.packages[] | .name as $from | .dependencies[]
      | select(.name as $d | $members | index($d)) | [$from, .name]]
  | reduce .[] as $e ({}; .[$e[0]] += [$e[1]]) as $graph
  | def reach($n; $seen):
      if ($seen | index($n)) then $seen
      else reduce ($graph[$n] // [])[] as $m ($seen + [$n]; reach($m; .)) end;
  [$members[] as $m | select([($graph[$m] // [])[] as $d | reach($d; [])] | add // [] | index($m)) | $m]
  | join(" ")' <<<"$metadata")"
if [[ -n "$cycle" ]]; then
  echo "check-release-graph: these members are on a dependency cycle: $cycle" >&2
  status=1
fi
exit "$status"
