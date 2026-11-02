# Operations

One binary, `oxcache-server`, runs every deployment shape. The operator never holds
a customer key; everything below works on ciphertext and account data.

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
  primary plus streaming replica on Hetzner nodes. A tenant is pinned to a region at
  creation.
- **Upgrades.** Rolling. A release's migrations are additive, so the previous release
  keeps working during the rollout. Destructive schema steps ship one release later.

## Configuration

TOML file plus `OXCACHE_<SECTION>_<KEY>` environment overrides. Secret values are
accepted only from `*_file` paths or environment variables; the parser rejects a
secret written inline in the file.

```toml
[server]
listen = "0.0.0.0:8443"
admin_listen = "127.0.0.1:9091"
metrics_listen = "127.0.0.1:9090"
public_url = "https://cache.example.com"

[tls]
cert_file = "/etc/oxcache/tls.crt"
key_file = "/etc/oxcache/tls.key"

[metadata]
url = "sqlite:///var/lib/oxcache/meta.db"   # or postgres://user@host/db
password_file = "/run/secrets/db-password"   # postgres only

[store]
kind = "fs"                                   # or "s3"
root = "/var/lib/oxcache/objects"
# kind = "s3": endpoint, region, bucket, prefix, path_style,
#              access_key_id_file, secret_access_key_file

[keys]
url_key_file = "/run/secrets/url-key"         # HMAC for local-disk object URLs
claim_hash_key_file = "/run/secrets/claim-key" # keyed hashes of OIDC policy claims

[gc]
grace = "24h"
lease = "1h"
interval = "10m"
compaction_live_ratio = 0.5
orphan_scan = "7d"

[defaults]
retention_days = 30
hard_quota = "20GiB"
soft_quota_ratio = 0.9
rate = 50
burst = 200

[log]
level = "info"
format = "json"
```

The URL key may list two keys during rotation; both are accepted and the first signs.
The claim-hash key is stable: rotating it means re-entering trust policies.

Subcommands: `serve`, `init` (create the schema and print the first operator token
once), `migrate --from a.toml --to b.toml`, `backup --to <file>` (SQLite online
backup), `scrub [--cache c_…]`, `check-store` (run the S3 probe and print the
capabilities).

## Backup and restore

- **What matters:** metadata. That covers tenants, users, tokens, policies, rosters,
  and the entry index. Packs are reproducible: CI rebuilds them, at the cost of a
  cold cache.
- **Postgres:** base backups plus WAL archiving to a bucket (or the provider's PITR),
  restore-tested monthly. **SQLite:** `oxcache-server backup` hourly, copied off the
  host.
- **Packs:** provider durability for S3. Local packs are immutable, so filesystem
  snapshots or `rsync` without `--delete` are consistent and incremental.
- **Restore semantics.** A database older than the bucket leaves orphan packs, which
  the weekly scan removes. A database newer than the bucket leaves entries naming
  missing packs; they produce misses and reports, and the reports delete them. Either
  way the result is misses, never wrong hits. testing.md covers both.
- **Customer keys** are never backed up by the operator. Customers must keep at least
  two owner identities (for example one offline). If every owner identity is lost,
  the roster can no longer change: create a new cache, set new `owner_keys`, and
  start cold. The CLI keeps a copy of the last roster so an owner can re-publish it
  after a server restore.

## Observability

Prometheus metrics on `metrics_listen`:

| Metric | Labels |
| --- | --- |
| `oxcache_http_requests_total`, `oxcache_http_request_seconds` | `route`, `status` |
| `oxcache_lookup_ids_total` | `result` = hit, miss |
| `oxcache_commit_entries_total` | `status` |
| `oxcache_packs`, `oxcache_pack_bytes` | `state` |
| `oxcache_store_request_seconds`, `oxcache_store_errors_total` | `op` |
| `oxcache_metadata_query_seconds` | `op` |
| `oxcache_gc_leader`, `oxcache_gc_last_run_timestamp`, `oxcache_gc_swept_packs_total`, `oxcache_gc_compacted_bytes_total` | |
| `oxcache_reports_total` | `outcome` = confirmed, unconfirmed |
| `oxcache_oidc_exchanges_total` | `result` |

Tenant and cache ids are not metric labels; per-tenant figures live in the usage
ledger. Suggested alerts: any confirmed report; no GC leader for 30 min; store error
rate above 1%; pending packs older than twice the lease; p99 lookup above 1 s.

Logs are JSON lines on stdout with `ts`, `level`, `request_id`, `route`, `status`,
`duration_ms`, `tenant`, `cache`, `principal`, counts and byte sizes. Never logged:
tokens, session ids, JWTs, OIDC claims, URL query strings, lookup ids, segment ids,
record or roster bytes. A canary test enforces this (testing.md).

## Tenant administration

`oxcache-admin` talks to the admin listener with an operator token (`--token-file`).

```
oxcache-admin tenant create acme --region na --hard-quota 20GiB --egress 500GiB
oxcache-admin tenant list | show <t> | suspend <t> | resume <t> | delete <t>
oxcache-admin tenant quota <t> --hard-quota 50GiB --rate 100
oxcache-admin tenant bootstrap-admin <t> --issuer https://idp.example --subject <sub>
oxcache-admin token create --tenant <t> --scopes tenant:admin --expires 30d
oxcache-admin usage export --month 2026-10 --format csv > usage-2026-10.csv
oxcache-admin cache scrub <c>
oxcache-admin health
```

Customers then manage their own caches, tokens, users, grants and OIDC policies
through the tenant API (protocol.md) or the `oxabl-cache` CLI.

## Usage and quotas

The ledger is append-only. Each node keeps counters in memory and flushes them every
60 s as rows `(node_id, flush_seq, tenant, cache, day, metric, value)`, unique on
everything but the value, so a retried flush is idempotent. A node crash loses at
most 60 s of counters, which is acceptable for monthly billing. The GC leader writes
a stored-bytes snapshot per cache at 00:00 UTC.

| Metric | Source |
| --- | --- |
| `stored_byte_days` | daily snapshot of pending, live and in-grace pack bytes |
| `ingress_bytes` | sizes of committed packs |
| `egress_bytes` | segment lengths returned in fetch-mode lookups (S3: an upper-bound estimate, since bytes bypass the server); bytes served (local disk: exact) |
| `requests`, `lookup_ids`, `hits`, `entries_committed`, `compaction_bytes` | counters |

`usage export` sums the ledger per tenant, cache and metric for a calendar month in
UTC. Billing scripts consume the CSV.

Quotas:

- **Stored bytes**, hard and soft, per tenant and per cache. `uploads` refuses over
  the hard quota. Above the soft quota, eviction runs (architecture.md).
- **Monthly egress** per tenant. Over the quota, fetch-mode lookups answer 403
  `quota_exceeded` and plan mode keeps working. Clients see misses.
- **Request rate** per tenant: a token bucket per node, configured as the tenant rate
  divided by the node count.

## Compression dictionary

`oxabl-cache dict train --outputs <dir>` samples up to 2,000 artifact sets, trains on
80% and measures the ratio on the held-out 20%. `--publish` uploads the dictionary
only if the measured gain is at least 1.3x over zstd-3 without it. CI retrains after
a toolchain or schema change (a full rebuild). Older dictionaries stay alive while
entries name them.
