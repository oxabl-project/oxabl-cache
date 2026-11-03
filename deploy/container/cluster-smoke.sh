#!/usr/bin/env bash
# Runs two server containers on one Postgres (TLS, a CA file) and one S3 store (Garage), the shape
# of the Kubernetes and hosted deployments: both become ready, exactly one holds the garbage
# collection lease, init mints the operator token, and the admin health route reports the store
# and the database. Needs docker, garage, postgres and openssl (the devshell has them).
# Usage: cluster-smoke.sh [image]
set -euo pipefail

image="${1:-oxabl-cache-server:dev}"
here="$(cd "$(dirname "$0")" && pwd)"
work="${SMOKE_DIR:-$here/../../target/cluster-smoke}"
rm -rf "$work"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
fail() { echo "FAIL: $*" >&2; exit 1; }

containers=()
cleanup() {
  for c in "${containers[@]}"; do docker rm -f "$c" >/dev/null 2>&1 || true; done
  stop_backends
  docker run --rm -v "$work:/w" docker.io/library/alpine:3.21 chown -R "$(id -u):$(id -g)" /w >/dev/null 2>&1 || true
}

# shellcheck source=../lib/backends.sh
. "$here/../lib/backends.sh"
trap cleanup EXIT

mkdir -p "$work/etc"
start_backends "$work" 127.0.0.1 127.0.0.1
pg_port="$(cat "$work/pg-port")"
s3_port="$(cat "$work/s3-port")"
pub1="$(free_port)"; adm1="$(free_port)"; met1="$(free_port)"
pub2="$(free_port)"; adm2="$(free_port)"; met2="$(free_port)"

config() {
  cat <<EOF
[server]
listen = "127.0.0.1:$1"
admin_listen = "127.0.0.1:$2"
metrics_listen = "127.0.0.1:$3"
public_url = "https://cache.example.com"
shutdown_timeout = "5s"

[metadata]
url = "postgres://oxabl_cache@127.0.0.1:$pg_port/oxabl_cache"
password_file = "/run/secrets/oxabl-cache/db-password"
ca_file = "/run/secrets/oxabl-cache/db-ca.pem"

[store]
kind = "s3"
endpoint = "http://127.0.0.1:$s3_port"
region = "garage"
bucket = "oxabl-cache"
path_style = true
access_key_id_file = "/run/secrets/oxabl-cache/s3-access-key-id"
secret_access_key_file = "/run/secrets/oxabl-cache/s3-secret-access-key"

[keys]
url_key_file = "/run/secrets/oxabl-cache/url-key"
claim_hash_key_file = "/run/secrets/oxabl-cache/claim-key"
EOF
}
config "$pub1" "$adm1" "$met1" > "$work/etc/node1.toml"
config "$pub2" "$adm2" "$met2" > "$work/etc/node2.toml"
# Secrets are mounted read-only; the container user (65532) reads them through a bind mount.
docker run --rm -v "$work:/w" docker.io/library/alpine:3.21 sh -c 'chown -R 65532:65532 /w/secrets /w/etc && chmod 0700 /w/secrets' >/dev/null

hardening=(--network host --read-only --cap-drop ALL --security-opt no-new-privileges
  -v "$work/secrets:/run/secrets/oxabl-cache:ro")

token="$(docker run --rm "${hardening[@]}" -v "$work/etc/node1.toml:/etc/oxabl-cache/server.toml:ro" "$image" init 2>"$work/init.err")" \
  || { cat "$work/init.err" >&2; fail "init"; }
case "$token" in oxo_*) ;; *) fail "init printed no operator token" ;; esac
docker run --rm "${hardening[@]}" -v "$work/etc/node1.toml:/etc/oxabl-cache/server.toml:ro" "$image" check-store \
  | grep -q '"kind"' || fail "check-store"

for n in 1 2; do
  docker run -d --name "oxabl-cache-smoke-$n-$$" "${hardening[@]}" \
    -v "$work/etc/node$n.toml:/etc/oxabl-cache/server.toml:ro" "$image" >/dev/null
  containers+=("oxabl-cache-smoke-$n-$$")
done
for n in 1 2; do
  for _ in $(seq 1 100); do
    docker logs "oxabl-cache-smoke-$n-$$" 2>&1 | grep -q '"ready"' && break
    sleep 0.2
  done
  docker logs "oxabl-cache-smoke-$n-$$" 2>&1 | grep -q '"ready"' || { docker logs "oxabl-cache-smoke-$n-$$" >&2; fail "node $n is not ready"; }
done

health="$(curl -sf -H "Authorization: Bearer $token" "http://127.0.0.1:$adm1/admin/v1/health")" || fail "health route"
case "$health" in *'"metadata":"ok"'*'"store":"ok"'* | *'"store":"ok"'*'"metadata":"ok"'*) ;; *) fail "health is not ok: $health" ;; esac

leaders=0
for _ in $(seq 1 100); do
  leaders=0
  for m in "$met1" "$met2"; do
    curl -sf "http://127.0.0.1:$m/metrics" | grep -q '^oxabl_cache_gc_leader 1' && leaders=$((leaders + 1))
  done
  [ "$leaders" -eq 1 ] && break
  sleep 0.2
done
[ "$leaders" -eq 1 ] || fail "expected one garbage collection leader, found $leaders"

echo "cluster smoke: ok ($image)"
