#!/usr/bin/env bash
# Renders fly.toml and server.toml from the environment and deploys the Fly app (docs/hosting/deploy-fly.md).
# Needs FLY_APP, FLY_REGION, DOMAIN, CLOUDFLARE_ACCOUNT_ID and R2_BUCKET; deploy/fly/deploy.env (git-ignored)
# may set them. Extra arguments go to fly deploy.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
if [[ -f deploy/fly/deploy.env ]]; then
  # shellcheck source=/dev/null
  source deploy/fly/deploy.env
fi
: "${FLY_APP:?set FLY_APP, the Fly app name}" "${FLY_REGION:?set FLY_REGION, e.g. ord}"
: "${DOMAIN:?set DOMAIN, e.g. cache.example.com}" "${CLOUDFLARE_ACCOUNT_ID:?set CLOUDFLARE_ACCOUNT_ID}"
: "${R2_BUCKET:?set R2_BUCKET, the R2 bucket for the packs}"
[[ $FLY_APP =~ ^[a-z0-9][a-z0-9-]*$ ]] || { echo "FLY_APP is not a Fly app name" >&2; exit 1; }
[[ $FLY_REGION =~ ^[a-z]{3}$ ]] || { echo "FLY_REGION is not a three-letter Fly region" >&2; exit 1; }
[[ $CLOUDFLARE_ACCOUNT_ID =~ ^[0-9a-f]{32}$ ]] || { echo "CLOUDFLARE_ACCOUNT_ID is not a 32-hex account id" >&2; exit 1; }
render() {
  sed -e "s|@FLY_APP@|$FLY_APP|" -e "s|@FLY_REGION@|$FLY_REGION|" -e "s|@CLOUDFLARE_ACCOUNT_ID@|$CLOUDFLARE_ACCOUNT_ID|" \
    -e "s|@DOMAIN@|$DOMAIN|" -e "s|@R2_BUCKET@|$R2_BUCKET|" "$1.in" > "$1"
  if grep -q '@[A-Z0-9_]*@' "$1"; then
    echo "$1 has an unrendered value" >&2
    exit 1
  fi
}
render deploy/fly/fly.toml
render deploy/fly/server.toml
exec fly deploy . -c deploy/fly/fly.toml --ha=false "$@"
