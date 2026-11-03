# Deploy as a container or a single binary

One node, SQLite for metadata and a local directory for packs. This is the shape for a team on one VM
(operations.md, "Deployment shapes"). Everything below works with the server as built today (`store.kind =
"fs"`, `sqlite://`). Postgres and S3 settings follow operations.md and are used by the Kubernetes and hosted
guides.

## What the image is

`ghcr.io/oxabl-project/oxabl-cache-server`, built from `deploy/container/Containerfile`:

- static musl binaries `oxabl-cache-server` and `oxabl-cache-admin` on `gcr.io/distroless/static`, with CA roots
  and no shell or package manager;
- user `65532`, root filesystem meant to run read-only; the only writable path is the volume at
  `/var/lib/oxabl-cache`;
- entrypoint `oxabl-cache-server`, default command `serve`, default config `/etc/oxabl-cache/server.toml`;
- ports 8443 (public), 9091 (admin), 9090 (metrics).

Tags: `main`, `sha-<commit>`, and on a release tag `vX.Y.Z`, `X.Y` and `latest`. Pin a digest in production.
Every pushed image is a multi-architecture manifest (amd64 and arm64) and is signed with cosign (keyless):

```sh
cosign verify ghcr.io/oxabl-project/oxabl-cache-server@sha256:<digest> \
  --certificate-identity-regexp 'https://github.com/oxabl-project/oxabl-cache/\.github/workflows/image\.yml@.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

The build is reproducible: base images are pinned by digest, the build uses `--locked`, and two clean builds with
the same `SOURCE_DATE_EPOCH` give the same image digest (`deploy/container/reproducible.sh`).

## Run it

`deploy/container/server.example.toml` is a ready single-node config. TLS terminates in a proxy in front of the
container, so the config says `tls.terminated_upstream = true`. Publish the admin and metrics ports on the host's
loopback only.

```sh
docker volume create oxabl-cache-data
docker run --rm --read-only --cap-drop ALL --security-opt no-new-privileges \
  -v oxabl-cache-data:/var/lib/oxabl-cache \
  -v "$PWD/server.toml:/etc/oxabl-cache/server.toml:ro" \
  ghcr.io/oxabl-project/oxabl-cache-server:latest init     # prints the operator token once

docker run -d --name oxabl-cache --restart unless-stopped \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  -v oxabl-cache-data:/var/lib/oxabl-cache \
  -v "$PWD/server.toml:/etc/oxabl-cache/server.toml:ro" \
  -p 127.0.0.1:8443:8443 -p 127.0.0.1:9091:9091 -p 127.0.0.1:9090:9090 \
  ghcr.io/oxabl-project/oxabl-cache-server:latest
```

- `init` creates the key files under the volume (mode 0600), migrates the schema and prints the first operator
  token on stdout. Store the token in a secret manager. A second `init` refuses.
- The named volume takes its ownership from the image. For a bind mount, `chown 65532:65532` the directory.
- The server refuses a secret written inline in the config and refuses a non-loopback bind without TLS or
  `terminated_upstream`; both are checked by `deploy/container/smoke.sh`.
- There is no `HEALTHCHECK`: the image has no shell or `curl`. Check with the metrics port
  (`curl -f http://127.0.0.1:9090/metrics`) or the admin route
  (`curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9091/admin/v1/health`).
- Stop with `docker stop`: SIGTERM drains requests for up to `server.shutdown_timeout`, flushes usage and exits 0.

Podman and systemd (Quadlet) run the same image, for example `/etc/containers/systemd/oxabl-cache.container`:

```ini
[Container]
Image=ghcr.io/oxabl-project/oxabl-cache-server@sha256:<digest>
ReadOnly=true
DropCapability=ALL
NoNewPrivileges=true
PublishPort=127.0.0.1:8443:8443
PublishPort=127.0.0.1:9091:9091
PublishPort=127.0.0.1:9090:9090
Volume=oxabl-cache-data.volume:/var/lib/oxabl-cache
Volume=/etc/oxabl-cache/server.toml:/etc/oxabl-cache/server.toml:ro
Exec=serve

[Service]
Restart=always

[Install]
WantedBy=multi-user.target
```

`deploy/tofu/hosted/templates/` holds a complete, tested set of Quadlet files, including Caddy.

## TLS

The server refuses to bind a non-loopback address without TLS. There is no built-in ACME. Pick one:

**Caddy in front (recommended).** Caddy obtains and renews the certificate and forwards to the server on
loopback:

```
cache.example.com {
	reverse_proxy 127.0.0.1:8443
}
```

Leave Caddy's access log off: presigned object URLs carry their signature in the query string, and the server
never logs query strings. Set `server.public_url = "https://cache.example.com"`; clients use it to build object
URLs. A server on loopback needs no `[tls]` section; a container publishing to a non-loopback address inside its
own network needs `tls.terminated_upstream = true`, as in the example.

**Built-in rustls.** Mount a certificate chain and key and set
`tls.cert_file` and `tls.key_file`. TLS 1.3 only, HTTP/2 and HTTP/1.1. The admin listener then speaks TLS too, so
reach it with a host name the certificate covers. Reload by restarting.

## systemd, without a container

For one VM without a container runtime, `deploy/systemd/` holds two units and an example config. Build the
binary (`cargo build --release --locked -p oxabl-cache-server`, or copy it out of the image with `docker cp`),
install it as `/usr/local/bin/oxabl-cache-server`, then:

```sh
install -d -m 0700 /etc/oxabl-cache/secrets
install -m 0644 deploy/systemd/server.example.toml /etc/oxabl-cache/server.toml    # edit public_url
( umask 077; openssl rand -hex 32 > /etc/oxabl-cache/secrets/url-key
              openssl rand -hex 32 > /etc/oxabl-cache/secrets/claim-key )
install -m 0644 deploy/systemd/oxabl-cache.service deploy/systemd/oxabl-cache-init.service /etc/systemd/system/
systemctl daemon-reload

systemctl start oxabl-cache-init      # writes the operator token to /etc/oxabl-cache/operator-token
cat /etc/oxabl-cache/operator-token && shred -u /etc/oxabl-cache/operator-token

systemctl enable --now oxabl-cache
```

The unit runs as a dynamic user (`oxabl-cache`) with `ProtectSystem=strict`, its state in `StateDirectory`
(`/var/lib/oxabl-cache`, mode 0700), the two secret files delivered through `LoadCredential` (the service never
reads `/etc/oxabl-cache/secrets`), no capabilities, a system call filter (`@system-service`, without
`@privileged` and `@resources`), `NoNewPrivileges`, `MemoryDenyWriteExecute`, private `/tmp`, `/dev` and user
namespace, and no home directories. `systemd-analyze security` scores it 1.1 (OK). The init unit has the same
sandbox and its stdout goes to a root-only file, so the token never reaches the journal.

The unit leaves outbound traffic open because OIDC issuers are fetched over HTTPS. For a host without OIDC add
`IPAddressDeny=any` and `IPAddressAllow=localhost`. For built-in TLS on port 443 add
`AmbientCapabilities=CAP_NET_BIND_SERVICE` and `CapabilityBoundingSet=CAP_NET_BIND_SERVICE`, or keep 8443 and
forward.

## Two nodes on Postgres and S3

`deploy/container/cluster-smoke.sh` starts two containers of the image against one TLS Postgres (`ca_file`) and
one Garage bucket, the shape of the Kubernetes and hosted deployments. It checks that both become ready, that
`init` and `check-store` work, that the admin health route reports the database and the store, and that exactly
one node holds the garbage-collection lease.

## Backups and upgrades

- **What matters** is the metadata (SQLite file). Packs are reproducible, and CI rebuilds them at the cost of a
  cold cache. `oxabl-cache-server backup` has no implementation yet, so
  until it is, snapshot the volume while the server is stopped, or use filesystem snapshots (WAL mode makes a
  crash-consistent snapshot recoverable).
- **Packs on local disk** are immutable: `rsync` without `--delete` or a filesystem snapshot is consistent.
- **Upgrades** replace the image tag (or the binary) and restart. Migrations are additive and run at startup.
- **Verify a node** with `oxabl-cache-server scrub` (re-verifies stored packs) and, on an S3 store,
  `check-store` (prints the store's capabilities).
