#!/usr/bin/env bash
# Installs the single-node chart into a throwaway kind cluster, runs init in the pod and reads
# the admin health route through the admin Service, then removes the object store's directory
# on the volume and expects the pod to go NotReady (and stay running), and Ready again once the
# directory is back. Needs docker, kind, kubectl and helm.
# Usage: kind-smoke.sh [image-name:tag]   (a local image, default oxabl-cache-server:dev)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
chart="$here/oxabl-cache"
image="${1:-oxabl-cache-server:dev}"
cluster="oxabl-cache-smoke-$$"
work="${SMOKE_DIR:-$here/../../target/kind-smoke}"
mkdir -p "$work"
kubeconfig="$(cd "$work" && pwd)/kubeconfig-$$"
export KUBECONFIG="$kubeconfig"
ns=oxabl-cache
fail() { echo "FAIL: $*" >&2; exit 1; }

cleanup() {
  [ -n "${forward:-}" ] && kill "$forward" 2>/dev/null || true
  kind delete cluster --name "$cluster" >/dev/null 2>&1 || true
  rm -f "$KUBECONFIG"
}
trap cleanup EXIT

kind create cluster --name "$cluster" --wait 120s --quiet >/dev/null
kind load docker-image "$image" --name "$cluster" >/dev/null
kubectl create namespace "$ns" >/dev/null
kubectl -n "$ns" create secret generic oxabl-cache-keys \
  --from-literal=url-key="$(openssl rand -hex 32)" \
  --from-literal=claim-key="$(openssl rand -hex 32)" >/dev/null

repository="docker.io/library/${image%%:*}"
helm install smoke "$chart" -n "$ns" -f "$chart/examples/single-node.yaml" \
  --set image.repository="$repository" --set image.tag="${image##*:}" --set image.pullPolicy=Never \
  --set ingress.enabled=false --wait --timeout 180s >/dev/null || fail "the release did not become ready"

deployment="deploy/smoke-oxabl-cache"
token="$(kubectl -n "$ns" exec "$deployment" -- /usr/local/bin/oxabl-cache-server init 2>/dev/null)"
case "$token" in oxo_*) ;; *) fail "init printed no operator token" ;; esac

kubectl -n "$ns" port-forward svc/smoke-oxabl-cache-admin 0:9091 >"$work/forward-$$.log" 2>&1 &
forward=$!
port=""
for _ in $(seq 1 50); do
  port="$(sed -n 's/^Forwarding from 127.0.0.1:\([0-9]*\) .*/\1/p' "$work/forward-$$.log" | head -1)"
  [ -n "$port" ] && break
  sleep 0.2
done
[ -n "$port" ] || fail "no port forward"
health="$(curl -sf -H "Authorization: Bearer $token" "http://127.0.0.1:$port/admin/v1/health")" || fail "health route"
case "$health" in *'"metadata":"ok"'*) ;; *) fail "health is not ok: $health" ;; esac
rm -f "$work/forward-$$.log"

pod_ready() { kubectl -n "$ns" get pod -l app.kubernetes.io/instance=smoke -o jsonpath='{.items[0].status.conditions[?(@.type=="Ready")].status}'; }
volume="$(kubectl get pv -o jsonpath='{.items[0].spec.hostPath.path}')"
[ -n "$volume" ] || fail "the volume has no host path"
node="$cluster-control-plane"
[ "$(pod_ready)" = True ] || fail "the pod is not Ready before the store breaks"

docker exec "$node" rm -rf "$volume/objects"
kubectl -n "$ns" wait pod -l app.kubernetes.io/instance=smoke --for=condition=Ready=false --timeout=60s >/dev/null \
  || fail "the pod stayed Ready with its object store directory gone"
restarts="$(kubectl -n "$ns" get pod -l app.kubernetes.io/instance=smoke -o jsonpath='{.items[0].status.containerStatuses[0].restartCount}')"
[ "$restarts" = 0 ] || fail "the pod restarted ($restarts) instead of only leaving the Service"

docker exec "$node" sh -c "mkdir -p '$volume/objects/tmp' && chown -R 65532:65532 '$volume/objects'"
kubectl -n "$ns" wait pod -l app.kubernetes.io/instance=smoke --for=condition=Ready --timeout=60s >/dev/null \
  || fail "the pod did not become Ready again"
echo "kind smoke: ok ($image)"
