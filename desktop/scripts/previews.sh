#!/usr/bin/env bash
# Renders every named scene of the app to PNG with a software Vulkan driver, no display needed.
# Usage: scripts/previews.sh [binary] [out-dir] [time]
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
bin="${1:-$here/target/debug/oxabl-cache-desktop}"
out="${2:-$here/target/previews}"
at="${3:-2.5}"
mkdir -p "$out"
: "${OXABL_DESKTOP_LAVAPIPE_ICD:?run inside the devshell (nix develop)}"
export LD_LIBRARY_PATH="${OXABL_DESKTOP_VULKAN_LIBS}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export VK_ICD_FILENAMES="$OXABL_DESKTOP_LAVAPIPE_ICD"
for scene in $("$bin" --list-scenes | tr ' ' '\n' | grep -v '^$'); do
  "$bin" --snap "$out/$scene.png" --scene "$scene" --at "$at" --scale 2 >/dev/null 2>"$out/$scene.log" \
    || { echo "scene $scene failed:"; cat "$out/$scene.log"; exit 1; }
done
ls "$out"/*.png
