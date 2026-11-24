#!/usr/bin/env bash
# Lints and renders the chart for every example values file, validates the manifests against
# the Kubernetes 1.31 schemas, and fails if the chart renders a Secret or a pod that is not
# hardened. Usage: check.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
chart="$here/oxabl-cache"
k8s_version="${K8S_VERSION:-1.31.0}"
fail() { echo "FAIL: $*" >&2; exit 1; }

for values in "$chart"/examples/*.yaml; do
  name="$(basename "$values" .yaml)"
  helm lint "$chart" -f "$values" --strict >/dev/null || fail "helm lint $name"
  rendered="$(helm template check "$chart" -f "$values" --namespace oxabl-cache)"

  printf '%s\n' "$rendered" \
    | kubeconform -strict -summary -kubernetes-version "$k8s_version" -skip ServiceMonitor \
    || fail "kubeconform $name"

  secrets="$(printf '%s\n' "$rendered" | awk -v RS='\n---\n' '/(^|\n)kind: Secret(\n|$)/')"
  [ -z "$secrets" ] || fail "$name renders a Secret: secrets are referenced by name, never templated"

  deployment="$(printf '%s\n' "$rendered" | awk '/^kind: Deployment$/{f=1} f')"
  for needle in 'readOnlyRootFilesystem: true' 'allowPrivilegeEscalation: false' 'runAsNonRoot: true' \
      '- ALL' 'type: RuntimeDefault'; do
    printf '%s\n' "$deployment" | grep -q -- "$needle" || fail "$name: pod lacks $needle"
  done
  for probe in startupProbe readinessProbe; do
    printf '%s\n' "$deployment" | grep -A3 "$probe:" | grep -q 'path: /readyz' || fail "$name: $probe is not /readyz"
  done
  printf '%s\n' "$deployment" | grep -A3 'livenessProbe:' | grep -q 'path: /livez' || fail "$name: livenessProbe is not /livez"
  echo "ok $name"
done

if helm template check "$chart" -f "$chart/examples/single-node.yaml" --set keys.existingSecret= >/dev/null 2>&1; then
  fail "a chart without the keys Secret rendered"
fi
if helm template check "$chart" -f "$chart/examples/single-node.yaml" --set publicUrl= >/dev/null 2>&1; then
  fail "a chart without publicUrl rendered"
fi
sn_pg="$(helm template check "$chart" -f "$chart/examples/single-node-postgres.yaml")"
printf '%s\n' "$sn_pg" | grep -q 'url = "postgres://' || fail "single-node-postgres: metadata is not Postgres"
printf '%s\n' "$sn_pg" | grep -q 'kind = "fs"' || fail "single-node-postgres: store is not the PVC"
printf '%s\n' "$sn_pg" | grep -q 'path: db-password' || fail "single-node-postgres: the database password is not mounted"
if printf '%s\n' "$sn_pg" | grep -q 's3-access-key-id'; then
  fail "single-node-postgres mounts S3 credentials"
fi
sn="$(helm template check "$chart" -f "$chart/examples/single-node.yaml")"
printf '%s\n' "$sn" | grep -q 'url = "sqlite://' || fail "single-node: metadata is not SQLite"
if printf '%s\n' "$sn" | grep -q 'db-password'; then
  fail "single-node mounts a database password"
fi
if helm template check "$chart" -f "$chart/examples/single-node-postgres.yaml" \
    --set metadata.postgres.passwordSecret.name= >/dev/null 2>&1; then
  fail "single-node with a Postgres URL and no password Secret rendered"
fi
echo "helm checks: ok"
