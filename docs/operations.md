# Operations

One binary, `oxabl-cache-server`, runs every deployment shape. The operator never holds
a customer key; everything below works on ciphertext and account data.

Step-by-step guides:
- [Container and systemd](hosting/deploy-container.md): one VM, or two nodes on
  Postgres and S3.
- [Kubernetes](hosting/deploy-kubernetes.md): the Helm chart.
- [Hosted](hosting/deploy-hosted.md): Cloudflare R2 and Hetzner, with OpenTofu.

The image is built and smoke-tested on every pull request; the amd64 build also runs
the two-node Postgres and S3 smoke test (`deploy/container/cluster-smoke.sh`). From
`main` and `v*` tags it is pushed to `ghcr.io/oxabl-project/oxabl-cache-server`, as a
multi-arch manifest signed with cosign keyless, but only while the repository
variable `PUBLISH_IMAGE` is `true`. That variable is not set yet, so no image is
published: build one with `docker build -f deploy/container/Containerfile -t oxabl-cache-server .` and use
that tag wherever the guides name the ghcr image. Forks and fresh clones never try to
publish.

## Deployment shapes

| Shape | Metadata | Bytes | Nodes | Use |
| --- | --- | --- | --- | --- |
| Single binary | SQLite file | local directory | 1 | A team on one VM; systemd unit |
| Container | SQLite on a volume, or Postgres | S3-compatible bucket, or a volume | 1 | Docker or Podman hosts |
| Kubernetes (Helm chart) | Postgres | S3-compatible bucket | 2 or more | Self-hosted at scale |
| Hosted multi-tenant | Postgres per region | Cloudflare R2 bucket per region | 2 or more per region | The paid service (hosting/provider-analysis.md, section 6) |

- **TLS.** Built-in rustls from certificate files, or `tls.terminated_upstream =
  true` behind a proxy. The server refuses to bind a non-loopback address without
  one of the two. There is no built-in ACME; a proxy such as Caddy provides it.
- **Container image.** Static binary in a distroless image, non-root, read-only root
  filesystem. Writable paths exist only for the local store and SQLite.
- **Helm chart.** A Deployment (default 2 replicas, `PodDisruptionBudget`
  `minAvailable: 1`, HPA on CPU), a Service and Ingress for the public listener, a
  ClusterIP-only Service for the admin listener, a metrics port with an optional
  `ServiceMonitor`, and Secrets referenced by name and never templated with values.
  It has no CronJobs: GC and compaction run in-process under a Postgres advisory-lock
  leader, and migrations run at startup under the same lock.
- **Hosted service.** One deployment per data region (North America by default, EU
  for EU tenants), each with its own R2 bucket (EU jurisdiction for EU) and Postgres
  primary plus streaming replica on Hetzner nodes. A tenant is pinned to a region by
  being created through that region's admin endpoint.
- **Upgrades.** Rolling. A release's migrations are additive, so the previous release
  keeps working during the rollout. Destructive schema steps ship one release later.

## Configuration

TOML file plus `OXABL_CACHED_<SECTION>_<KEY>` environment overrides. The prefix is
deliberately distinct from the client's `OXABL_CACHE_*` variables. Secret values are
accepted only from `*_file` paths or environment variables; the parser rejects a
secret written inline in the file.

```toml
[server]
listen = "0.0.0.0:8443"
admin_listen = "127.0.0.1:9091"                # default
metrics_listen = "127.0.0.1:9090"              # default
public_url = "https://cache.example.com"       # optional; required for a wildcard listen
url_lifetime = "15m"                           # presigned and object-URL lifetime
shutdown_timeout = "10s"                       # drain bound on SIGTERM/SIGINT

[tls]
cert_file = "/etc/oxabl-cache/tls.crt"
key_file = "/etc/oxabl-cache/tls.key"

[metadata]
url = "sqlite:///var/lib/oxabl-cache/meta.db"
# Postgres instead:
# url = "postgres://oxabl@db.internal/oxabl_cache"  # never a password in the URL
# password_file = "/run/secrets/db-password"
# ca_file = "/etc/oxabl-cache/db-ca.pem"             # private CA; default: web roots
# max_connections = 16
# statement_timeout = "120s"                       # positive, at most 1h
# idle_in_transaction_timeout = "60s"              # positive, at most 1h

[store]
kind = "fs"                                   # or "s3"
root = "/var/lib/oxabl-cache/objects"
# kind = "s3": endpoint, region, bucket, prefix, path_style,
#              access_key_id_file, secret_access_key_file

[keys]
url_key_file = "/run/secrets/url-key"         # HMAC for local-disk object URLs
claim_hash_key_file = "/run/secrets/claim-key" # keyed hashes of OIDC policy claims

[gc]
grace = "24h"                                 # at least url_lifetime + 1s; keep hours of gap
lease = "1h"
interval = "10m"
compaction_live_ratio = 0.5
orphan_scan = "7d"
batch = 500                                   # objects or rows per teardown step

[defaults]
retention_days = 30
hard_quota = "20GiB"
soft_quota_ratio = 0.9
rate = 50
burst = 200

[oidc]
# When set (the hosted service), only these issuers may be registered.
issuer_allowlist = ["https://token.actions.githubusercontent.com", "https://gitlab.com"]
# Private ranges an issuer may resolve to (self-hosted IdP on an internal network).
allowed_private_ranges = []
# Resolvers and extra CA bundle for issuer fetches; defaults: system resolver, system roots.
dns_servers = []
extra_ca_file = ""

[log]
level = "info"
format = "json"
```

`[metadata]` rules:
- SQLite-only keys are refused on a Postgres URL, and the other way round.
- A `postgres://` URL that carries a password is refused ("use password_file").
- TLS to Postgres (rustls) is required unless the URL says `sslmode=disable`. A URL
  without `sslmode`, or with `prefer`, still requires it, so the default fails closed.
  The server trusts the web roots, or only `ca_file` when it is set.
- `max_connections` sizes the server's own pool (default 16).
- `statement_timeout` (default 120 s) and `idle_in_transaction_timeout` (default
  60 s) are durations, positive and at most 1 h, and apply to Postgres only (SQLite
  refuses them like any other Postgres key). A statement or an idle transaction past
  its bound ends with 503 for that request, and the server recovers on its own. The
  idle-in-transaction bound is also how long a frozen former GC leader can delay a
  takeover (architecture.md, "Fencing"). TCP keepalives are 30 s.
- The database is opened first, so an unreachable database stops startup before
  anything else. Migrations run at startup in one transaction under an advisory lock
  (architecture.md, "The port and its transactions").

`[keys] claim_hash_key_file` holds 64 hex characters, and `init` creates it. Without
it, creating a workload policy answers 501 `not_configured`, while the OIDC exchange
still opens human sessions.

`[oidc]` is live:
- `dns_servers` takes `ip` or `ip:port` (default port 53), queried over UDP;
  truncated answers are refused.
- `extra_ca_file` is a PEM bundle added to the system roots, read at start.
- `allowed_private_ranges` takes CIDR blocks or single addresses.

The URL key may list two keys during rotation; both are accepted and the first signs.
The server warns at start (`readable by other users; chmod 600 it`) when the URL key file can be read by anyone
but its owner and the process's own group: mode 0440 with the process's group, as a Kubernetes Secret volume
with `fsGroup` has, passes; any other group, world or group-write bit warns. The
0440-with-a-foreign-group case is not covered by a test yet (testing.md).
The claim-hash key is stable: rotating it means re-entering trust policies.

`public_url` defaults to `http(s)://<bound address>`, which is what makes
`listen = "127.0.0.1:0"` usable in tests. A wildcard listen address (`0.0.0.0`,
`[::]`) needs an explicit `public_url`, because no URL can be derived from it.

Subcommands:

- `init` is the bootstrap command. It creates missing key files (mode 0600), migrates
  the schema, and prints the first operator token once, on stdout. It refuses if an
  operator token already exists.
- `serve` binds the three listeners, then prints one ready line on stdout:
  `{"ready":{"listen":"…","admin_listen":"…","metrics_listen":"…","public_url":"…"}}`.
  stdout carries nothing else, so a supervisor or test harness can read the first
  line. On SIGTERM or SIGINT it stops accepting, drains requests for up to
  `server.shutdown_timeout`, stops background jobs, flushes access days and usage,
  and exits 0.
- `backup --to <file>` (SQLite only) copies the metadata while the server keeps
  serving. It opens the database read-only on a second connection, runs
  `VACUUM INTO` a staging file created with mode 0600, then `PRAGMA
  integrity_check`. It then fsyncs and renames the file, and fsyncs the directory.
  An existing target is refused. On Postgres it refuses, naming base backups and
  WAL archiving instead.
- `migrate --from a.toml --to b.toml` moves a single node to Postgres and S3
  (architecture.md, "Growth"). `a.toml` is the SQLite deployment, `b.toml` the empty
  Postgres one, each a full server configuration read without the environment
  overlay. Stop the server first. Every committed pack is verified against its
  segment ids and footer and copied to the target store, then the metadata is copied
  and checked table by table with a row count and digest; the metadata commits last.
  A damaged or missing pack refuses the migration, naming it, and leaves the target
  without a metadata row. Packs written to a target store before a refusal are
  orphans, which the orphan scan removes.
- `scrub [--cache c_…]` prints one JSON line per cache.
- `check-store` runs the S3 probe and prints the capabilities.

## Backup and restore

- **What matters:** metadata. That covers tenants, users, tokens, policies, rosters,
  and the entry index. Packs are reproducible: CI rebuilds them, at the cost of a
  cold cache.
- **Postgres:** base backups plus WAL archiving to a bucket (or the provider's PITR),
  restore-tested monthly. **SQLite:** `oxabl-cache-server backup --to <file>`
  hourly, copied off the host. To restore, stop the server, put the copy in place of
  `meta.db`, and start it. Every pack committed before the backup began is there
  whole. Later ones are orphans, which the scan removes.
- **Packs:** provider durability for S3. Local packs are immutable, so filesystem
  snapshots or `rsync` without `--delete` are consistent and incremental.
- **Restore semantics.** A database older than the bucket leaves orphan packs, which
  the weekly scan removes. A database newer than the bucket leaves entries naming
  missing packs; they produce misses and reports, and the reports delete them. Either
  way the result is misses, never wrong hits. testing.md covers both.
- **Customer keys** are never backed up by the operator. Customers must keep at least
  two owner identities (for example one offline). If every owner identity is lost,
  the roster can no longer change: create a new cache, set new `owner_keys`, and
  start cold.

## Observability

Probes, all on the metrics listener (never on the public one):
- `GET /livez` answers 200 while the process is up.
- `GET /readyz` checks the metadata store (live node count) and the object store, in
  parallel, each within 2 s. It answers 200, or 503 `unavailable` naming what
  failed, with `Retry-After`. An S3 store is checked with a `HEAD` of a random key.
  Some stores answer that `HEAD` the same way for a deleted bucket as for a missing
  key, so readiness may stay 200 after the bucket is deleted; the bucket probe at
  startup and the store error metrics catch it.
  The local-disk store is checked by creating and removing a file in its `tmp`
  directory, so a missing or unmounted root, or one that cannot be written, fails
  readiness. No user data is written.

`GET /v1/health` on the public listener is process-only (204, no credential, no
dependency; protocol.md "Health"). It is for load balancers, not for readiness.
`GET /admin/v1/health` uses the same probe as `/readyz` and also reports the GC
leader.

Prometheus metrics on `metrics_listen`:

| Metric | Labels |
| --- | --- |
| `oxabl_cache_http_requests_total`, `oxabl_cache_http_request_seconds` | `route`, `status` |
| `oxabl_cache_lookup_ids_total` | `result` = hit, miss |
| `oxabl_cache_commit_entries_total` | `status` |
| `oxabl_cache_packs`, `oxabl_cache_pack_bytes` | `state` |
| `oxabl_cache_store_request_seconds`, `oxabl_cache_store_errors_total` | `op` |
| `oxabl_cache_metadata_query_seconds` | `op` |
| `oxabl_cache_gc_fenced_total` (GC transactions refused because another node took over) | |
| `oxabl_cache_gc_leader`, `oxabl_cache_gc_last_run_timestamp`, `oxabl_cache_gc_swept_packs_total`, `oxabl_cache_gc_compacted_bytes_total` | |
| `oxabl_cache_reports_total` | `outcome` = confirmed, unconfirmed |
| `oxabl_cache_oidc_exchanges_total` | `result` |

Tenant and cache ids are not metric labels; per-tenant figures live in the usage
ledger. Suggested alerts: any confirmed report; no GC leader for 30 min; a rising
`oxabl_cache_gc_fenced_total` (two nodes both believed they led, so check the
database connection and the leader's node); store error rate above 1%; pending packs older than twice the lease; p99 lookup above 1 s.

Logs are JSON lines on **stderr** (stdout belongs to the ready line and to `init`'s
token). Fields: `ts` (unix ms), `level`, `event`, `request_id`, `method`, `route`
(the route template, never the concrete path), `status`, `duration_ms`, `tenant`,
`cache`, `principal`, and counts and sizes (`ids`, `hits`, `entries`, `packs`,
`bytes_in`, `bytes_out`). Never logged:
tokens, session ids, JWTs, OIDC claims, URL query strings, lookup ids, segment ids,
record or roster bytes. A canary test enforces this (testing.md).

## Tenant administration

`oxabl-cache-admin` talks to one deployment's admin listener with an operator token.
A region is a separate deployment with its own admin endpoint, so there is no region
flag: the operator points `--server` at that region's admin listener. A tenant lives
in the region whose admin endpoint created it.

```
oxabl-cache-admin tenant create acme --hard-quota 20GiB --egress 500GiB
oxabl-cache-admin tenant list | show <t> | suspend <t> | resume <t> | delete <t>
oxabl-cache-admin operator-token create | list | revoke <id>
oxabl-cache-admin tenant quota <t> --hard-quota 50GiB --rate 100 --egress none
oxabl-cache-admin tenant bootstrap-admin <t> --issuer https://idp.example --subject <sub>
oxabl-cache-admin token create --tenant <t> --scopes tenant:admin --expires 30d
oxabl-cache-admin usage export --month 2026-10 --format csv > usage-2026-10.csv
oxabl-cache-admin cache scrub <c>
oxabl-cache-admin health
```

Settings and conventions:
- `--server`, or `OXABL_CACHE_ADMIN_SERVER`.
- `--token-file`, or `OXABL_CACHE_ADMIN_TOKEN_FILE`: always a path, never the token
  itself. There is no token flag, and the token never appears in output or in the
  process's arguments.
- `--ca-file` adds a CA to the system roots. `--json` gives machine-readable
  output.
- `<t>` is a tenant id or name, resolved through `GET /admin/v1/tenants`.
- Sizes take binary units only (`20GiB`), or `none` to clear a limit.
- `tenant quota` re-sends the current value of every flag left out.
- `token create` accepts only `--scopes tenant:admin`.
- `tenant bootstrap-admin` calls `POST /admin/v1/tenants/{t}/admins`.
- `usage export` writes CSV `month,tenant,cache,metric,value`, sorted by tenant,
  cache and metric. The CLI validates the header and every row, and refuses a
  malformed answer rather than printing it.
  `--format json` prints `{"month": …, "usage": [{tenant, cache, metric, value}, …]}`
  in the same order. The global `--json` means `--format json`; `--json --format csv`
  is a usage error (exit 2).
- Redirects are never followed, so the bearer never travels to another host.
- Exit codes: 0 success; 1 the server refused (`code: message` from the envelope);
  2 usage or configuration error; 3 server unreachable (connect, TLS or transport).

- `tenant delete` tombstones the tenant and all its caches at once (protocol.md).
  Its credentials stop working, and the GC teardown removes its data after the
  grace. There is no operator `cache delete`: deleting a cache belongs to the
  tenant's own API.
- `operator-token revoke` refuses to revoke the last valid operator token (409
  `last_operator_token`). Mint a second token first, because `init` never mints
  again once a token exists.

Customers then manage their own caches, tokens, users, grants and OIDC policies
through the tenant API (protocol.md) or the `oxabl-cache` CLI.

## Usage and quotas

The ledger is append-only. Each node keeps counters in memory and flushes every 60 s.
It writes only non-zero deltas, as rows `(node_id, flush_seq, tenant, cache, day,
metric, value)`. A node crash loses at most 60 s of counters, which is acceptable for
monthly billing.

Identity and order:

- `node_id` is 16 random bytes drawn at each process start. It is never a hostname,
  a pod ordinal or a configured value, so a restarted process can never collide with
  its predecessor's high-water mark.
- `flush_seq` starts at 1 for each `node_id` and rises by one per flush.
- Flushes are serial. A failed flush is retried with the same `flush_seq` and the
  same rows until it succeeds. Counters that arrive meanwhile wait for the next
  flush, so flush N+1 is never sent before N is acknowledged.

Each flush is one transaction that:

- reads and raises the node's high-water mark in `ledger_nodes(node_id,
  max_flush_seq)`. A flush whose `flush_seq` is not above the mark is acknowledged
  and ignored, so a retried flush is idempotent even after a roll-up has removed its
  rows. Every ignored flush is counted in `oxabl_cache_ledger_ignored_flushes_total`.
  With serial flushes, that counter rises only for retries of a flush that had in
  fact been written. Rows of `ledger_nodes` whose node has not flushed for 2 days
  are pruned; any later flush from such a node is for a closed day and is refused by
  the next rule;
- refuses rows for a closed day (`ledger_days.closed_at` set). Those counters are
  dropped and counted in `oxabl_cache_ledger_late_dropped_total`. A day closes one
  hour after it ends, so only a node that could not flush for an hour loses
  counters.

At 01:00 UTC the GC leader closes the previous day and rolls it up. In one
transaction it sets `closed_at` and replaces that day's rows with one row per
tenant, cache and metric (`node_id = 0`). The table therefore holds at most one day of fine-grained rows plus
one row per tenant, cache, metric and day. The GC leader also writes a stored-bytes
snapshot per cache at 00:00 UTC.

| Metric | Source |
| --- | --- |
| `stored_byte_days` | daily snapshot of pending, live and in-grace pack bytes |
| `ingress_bytes` | sizes of committed packs |
| `egress_bytes` | segment lengths returned in fetch-mode lookups (S3: an upper-bound estimate, since bytes bypass the server); bytes served (local disk: exact) |
| `requests`, `lookup_ids`, `hits`, `entries_committed`, `compaction_bytes` | counters |

`usage export` sums the ledger per tenant, cache and metric for a calendar month in
UTC. Billing scripts consume the CSV.

Quotas:

- **Live bytes**, hard and soft, per tenant and per cache: the bytes that entries
  still name. `uploads` refuses a pack that would take live plus pending bytes over
  the hard quota. Above 90% of hard, eviction runs down to 80% (architecture.md).
  Each user's personal namespace is capped at 10% of the cache's hard quota. A
  `trusted` upload that would exceed the hard quota first evicts personal entries,
  oldest first, in its own admission transaction (architecture.md, "Quotas").
  Bytes awaiting compaction or the grace (`reclaimable_bytes`) are reported
  separately and billed as stored bytes, but never drive eviction.
- **Stored-bytes ceiling**: 3x the hard quota, counting every byte that is `pending`,
  `live` or in grace. An abuse guard that steady-state use never reaches.
- **Monthly egress** per tenant. Over the quota, fetch-mode lookups answer 403
  `quota_exceeded` and plan mode keeps working. Clients see misses.
- **Request rate** per tenant on `/v1` API routes: a token bucket per node, sized as
  the tenant rate divided by the number of live nodes (heartbeat within 30 s in the
  `nodes` table), recomputed every 10 s. Autoscaling therefore keeps the tenant-wide
  limit stable. Object routes in local-disk mode are not rate-limited per tenant;
  their signed, short-lived URLs bound them, so a parallel cold fetch never sees
  429.

## Compression dictionary

`oxabl-cache dict train --outputs <dir>` samples up to 2,000 artifact sets, trains on
80% and measures the ratio on the held-out 20%. `--publish` uploads the dictionary
only if the measured gain is at least 1.3x over zstd-3 without it.

Publishing commits a `trusted` pack holding only the dictionary segment (kind
`dictionary`, no entries), then moves the pointer (protocol.md, "dictionary"). CI
retrains after a toolchain or schema change (a full rebuild).

A dictionary stays live while the pointer or any entry names it. It dies when both
have moved on, or after the grace if it was never referenced. Its pack is then
swept or compacted like any other.
