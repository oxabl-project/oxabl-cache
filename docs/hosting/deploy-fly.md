# Deploy on Fly.io

One Fly Machine runs the server with SQLite on a Fly volume, and the packs go to a Cloudflare R2 bucket. This is
the single-node shape of deploy-container.md with `store.kind = "s3"`. SQLite allows one machine: never scale the
app beyond one.

## What is in the repository

- `deploy/fly/fly.toml.in`: the Fly app. It builds `deploy/container/Containerfile` with `RUNTIME_USER=0:0`,
  because a Fly volume is owned by root and cannot take the image's user. The machine is its own microVM.
- `deploy/fly/server.toml.in`: the server config. The Fly edge terminates TLS
  (`tls.terminated_upstream = true`). Fly publishes only the `http_service` port; admin (9091) and metrics (9090)
  stay on the private network.
- `deploy/fly/deploy.sh`: renders both files and runs `fly deploy`.

The rendered `fly.toml` and `server.toml`, and `deploy/fly/deploy.env`, are ignored by git. Your app name,
region, domain, account id and bucket stay on your machine.

## Values

Set them in the environment, or copy `deploy/fly/deploy.env.example` to `deploy/fly/deploy.env`:

| Variable | Meaning |
|---|---|
| `FLY_APP` | the Fly app name |
| `FLY_REGION` | the primary region, e.g. `ord` |
| `DOMAIN` | the public host name; the server's `public_url` is `https://$DOMAIN` |
| `CLOUDFLARE_ACCOUNT_ID` | 32 hex characters; the R2 endpoint is `https://<id>.r2.cloudflarestorage.com` |
| `R2_BUCKET` | the bucket for the packs |

## First deploy

1. Create the app and its volume (the volume name is `oxabl_cache_data` in `fly.toml.in`):

   ```sh
   fly apps create "$FLY_APP"
   fly volumes create oxabl_cache_data -a "$FLY_APP" -r "$FLY_REGION" -s 3
   ```

2. Create the R2 bucket and an R2 API token with object read and write on that bucket only.
3. Set the secrets. Fly writes each one, base64-decoded, to a file under `/run/secrets/oxabl-cache/`:

   ```sh
   fly secrets set -a "$FLY_APP" --stage \
     URL_KEY="$(openssl rand -hex 32 | base64 -w0)" \
     CLAIM_KEY="$(openssl rand -hex 32 | base64 -w0)" \
     S3_ACCESS_KEY_ID="$(printf %s "$R2_ACCESS_KEY_ID" | base64 -w0)" \
     S3_SECRET_ACCESS_KEY="$(printf %s "$R2_SECRET_ACCESS_KEY" | base64 -w0)"
   ```

4. Deploy: `deploy/fly/deploy.sh`. Extra arguments go to `fly deploy`.
5. Mint the first operator token, once: `fly ssh console -a "$FLY_APP" -C "oxabl-cache-server init"`. It
   keeps the key files from the secrets and prints the token on stdout. Store it in a secret manager.
6. Point `DOMAIN` at the app (`fly certs add -a "$FLY_APP" "$DOMAIN"`, then the DNS records it names).
7. Reach admin through a proxy: `fly proxy 9091 -a "$FLY_APP"`, then the tenant steps of operations.md,
   "Tenant administration".

## Notes

- Fly's anycast edge accepts a TCP connection on any port of the app's address. Only the `http_service` port
  reaches the server; a scanner that reports another open port (for example RDP) gets no answer from it.
- Clients download packs from R2 directly, so pack downloads do not show in `fly logs`.
- The readiness check is `GET /readyz` on the metrics port (database and store).
