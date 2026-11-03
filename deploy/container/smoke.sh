#!/usr/bin/env bash
# Runs a built image the way an operator would and fails on any deviation:
# non-root, read-only root filesystem, init, serve, health, metrics, graceful stop,
# and refusal of an inline secret. Usage: smoke.sh [image]
set -euo pipefail

image="${1:-oxabl-cache-server:dev}"
here="$(cd "$(dirname "$0")" && pwd)"
work="${SMOKE_DIR:-$here/../../target/container-smoke}"
name="oxabl-cache-smoke-$$"
volume="$name-data"
mkdir -p "$work"
work="$(cd "$work" && pwd)"

cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker volume rm -f "$volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

hardening=(--read-only --cap-drop ALL --security-opt no-new-privileges)
config=(-v "$here/server.example.toml:/etc/oxabl-cache/server.toml:ro")
data=(-v "$volume:/var/lib/oxabl-cache")
run() { docker run --rm "${hardening[@]}" "$@"; }

docker volume create "$volume" >/dev/null

[ "$(docker image inspect "$image" --format '{{.Config.User}}')" = "65532:65532" ] || fail "image does not run as 65532"
run --entrypoint /bin/sh "$image" -c true >/dev/null 2>&1 && fail "the image has a shell"

token="$(run "${config[@]}" "${data[@]}" "$image" init 2>/dev/null)"
case "$token" in oxo_*) ;; *) fail "init printed no operator token" ;; esac
run "${config[@]}" "${data[@]}" "$image" init >/dev/null 2>&1 && fail "a second init was accepted"

docker run -d --name "$name" "${hardening[@]}" "${config[@]}" "${data[@]}" \
  -p 127.0.0.1::9091 -p 127.0.0.1::9090 "$image" >/dev/null
for _ in $(seq 1 50); do
  docker logs "$name" 2>/dev/null | grep -q '"ready"' && break
  sleep 0.2
done
docker logs "$name" 2>/dev/null | grep -q '"ready"' || fail "no ready line"
[ "$(docker inspect "$name" --format '{{.HostConfig.ReadonlyRootfs}}')" = "true" ] || fail "root filesystem is writable"
admin="$(docker port "$name" 9091/tcp | head -1)"
metrics="$(docker port "$name" 9090/tcp | head -1)"

[ "$(curl -s -o /dev/null -w '%{http_code}' "http://$admin/admin/v1/health")" = "401" ] || fail "health answered without a token"
health="$(curl -sf -H "Authorization: Bearer $token" "http://$admin/admin/v1/health")"
case "$health" in *'"metadata":"ok"'*'"store":"ok"'* | *'"store":"ok"'*'"metadata":"ok"'*) ;; *) fail "health is not ok: $health" ;; esac
curl -sf "http://$metrics/metrics" | grep -q '^oxabl_cache_gc_leader 1' || fail "metrics lack the gc leader"

docker stop -t 15 "$name" >/dev/null
[ "$(docker inspect "$name" --format '{{.State.ExitCode}}')" = "0" ] || fail "no clean exit on SIGTERM"
docker rm "$name" >/dev/null

sed 's|^url_key_file.*|url_key = "0000000000000000000000000000000000000000000000000000000000000000"|' \
  "$here/server.example.toml" > "$work/inline-secret.toml"
if run -v "$work/inline-secret.toml:/etc/oxabl-cache/server.toml:ro" "${data[@]}" "$image" serve >/dev/null 2>&1; then
  fail "a secret written inline in the config was accepted"
fi
if run -e OXABL_CACHED_KEYS_URL_KEY=00 "${config[@]}" "${data[@]}" "$image" serve >/dev/null 2>&1; then
  fail "a secret given as an environment value was accepted"
fi

echo "container smoke: ok ($image)"
