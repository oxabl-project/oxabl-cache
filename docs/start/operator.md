# Getting started: operator

The operator runs the server. This path ends with a running server, one tenant and the tenant
administrator's token, which you hand to the cache owner. You never hold a customer key
(data-handling.md).

You need: a host (or a cluster), the three binaries or the image, and a DNS name with TLS in front of
the public listener.

## 1. Install and host the server

Pick the shape that fits and follow its page. Each page installs the server, creates the key files,
runs `init` and starts the service:

| Shape | Page |
| --- | --- |
| One VM, container or systemd, SQLite and local disk | [deploy-container](../hosting/deploy-container.md) |
| A cluster, Helm chart, Postgres and S3 | [deploy-kubernetes](../hosting/deploy-kubernetes.md) |
| The hosted service, Cloudflare R2 and Hetzner | [deploy-hosted](../hosting/deploy-hosted.md) |
| One Fly.io machine, SQLite on a volume, packs in R2 | [deploy-fly](../hosting/deploy-fly.md) |

No release publishes the binaries yet. Build them from the repository:

```sh
cargo build --release --locked -p oxabl-cache-server -p oxabl-cache-admin -p oxabl-cache-cli
```

`oxabl-cache-server` runs the service. `oxabl-cache-admin` is yours. `oxabl-cache` is the member
command line for owners, CI and developers; the container image does not hold it.

`init` prints the first operator token once, on stdout. Keep it in a file of mode 0600 and in your
secret manager. It expires after one year. Before it does, mint a second token
(`oxabl-cache-admin operator-token create`) and revoke the first.

**Check.** The server is up when the readiness probe answers 200 and the admin command reports every
part ok:

```sh
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:9090/readyz
200

export OXABL_CACHE_ADMIN_SERVER=http://127.0.0.1:9091
export OXABL_CACHE_ADMIN_TOKEN_FILE=/etc/oxabl-cache/operator-token
oxabl-cache-admin health
metadata   ok
store      ok
gc_leader  true
```

The admin listener binds to loopback. Reach it from the host, or through a tunnel such as
`kubectl port-forward` or `fly proxy 9091`. `--token-file` always names a file: no flag takes the
token itself.

## 2. Create the first tenant

A tenant is one customer account. Give it the quotas it needs:

```sh
oxabl-cache-admin tenant create acme --hard-quota 20GiB --egress 500GiB
id            t_2h8k4m6p9q1r3s5t7v0w2x4y6z
name          acme
status        active
hard_bytes    21474836480
egress_bytes  536870912000
rate          50
burst         200
created_at    1794403838
```

The same name and limits again is a no-op. Write down the tenant id: the owner needs it.

## 3. Make the tenant administrator's token

`token create --tenant` mints a `tenant:admin` token. Its secret appears once. Write it straight to a
file:

```sh
( umask 077
  oxabl-cache-admin --json token create --tenant acme --expires 30d --name acme-admin \
    | jq -r .token > acme-admin.token )
```

Without `--expires`, the server's default of 90 days applies. The longest life is one year. Send the
file to the tenant administrator by a secure channel, together with the server's public URL and the
tenant id.

A tenant whose people sign in with an identity provider can have a human administrator instead:

```sh
oxabl-cache-admin tenant bootstrap-admin acme --issuer https://idp.example --subject <sub>
```

**Check.** The token reaches the public listener and names the tenant. The header file keeps the token
out of the process list:

```sh
( umask 077; printf 'Authorization: Bearer %s\n' "$(cat acme-admin.token)" > acme-admin.hdr )
curl -sS -H @acme-admin.hdr https://cache.example.com/v1/me
{"caches":[],"principal":"p_…","tenant":"t_2h8k4m6p9q1r3s5t7v0w2x4y6z","tenant_admin":true}
```

`"tenant_admin":true` and an empty `caches` list are what a new tenant shows. The next step belongs to
the [cache owner](owner.md).

## Later

- Suspend, resume, delete and change quotas: operations.md, "Tenant administration".
- Back up the metadata and test a restore: operations.md, "Backup and restore".
- Watch the probes and metrics: operations.md, "Observability".
- When a command fails: operations.md, "When something fails".
