#!/usr/bin/env bash
# Runs every deploy check that needs no container runtime and no cluster: the Helm chart, the
# OpenTofu module and the systemd units. Usage: check.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
"$here/helm/check.sh"
"$here/tofu/check.sh"
"$here/systemd/check.sh"
echo "deploy checks: ok"
