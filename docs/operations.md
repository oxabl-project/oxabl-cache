# Operations

One binary, `oxabl-cache-server`, runs every deployment shape. The operator never holds
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
url = "sqlite:///var/lib/oxabl-cache/meta.db"   # or postgres://user@host/db
password_file = "/run/secrets/db-password"   # postgres only

[store]
kind = "fs"                                   # or "s3"
root = "/var/lib/oxabl-cache/objects"
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

The URL key may list two keys during rotation; both are accepted and the first signs.
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
- The others: `migrate --from a.toml --to b.toml`, `backup --to <file>` (SQLite
  online backup), `scrub [--cache c_…]`, `check-store` (run the S3 probe and print
  the capabilities).

## Backup and restore

- **What matters:** metadata. That covers tenants, users, tokens, policies, rosters,
  and the entry index. Packs are reproducible: CI rebuilds them, at the cost of a
  cold cache.
- **Postgres:** base backups plus WAL archiving to a bucket (or the provider's PITR),
  restore-tested monthly. **SQLite:** `oxabl-cache-server backup` hourly, copied off the
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
| `oxabl_cache_http_requests_total`, `oxabl_cache_http_request_seconds` | `route`, `status` |
| `oxabl_cache_lookup_ids_total` | `result` = hit, miss |
| `oxabl_cache_commit_entries_total` | `status` |
| `oxabl_cache_packs`, `oxabl_cache_pack_bytes` | `state` |
| `oxabl_cache_store_request_seconds`, `oxabl_cache_store_errors_total` | `op` |
| `oxabl_cache_metadata_query_seconds` | `op` |
| `oxabl_cache_gc_leader`, `oxabl_cache_gc_last_run_timestamp`, `oxabl_cache_gc_swept_packs_total`, `oxabl_cache_gc_compacted_bytes_total` | |
| `oxabl_cache_reports_total` | `outcome` = confirmed, unconfirmed |
| `oxabl_cache_oidc_exchanges_total` | `result` |

Tenant and cache ids are not metric labels; per-tenant figures live in the usage
ledger. Suggested alerts: any confirmed report; no GC leader for 30 min; store error
rate above 1%; pending packs older than twice the lease; p99 lookup above 1 s.

Logs are JSON lines on **stderr** (stdout belongs to the ready line and to `init`'s
token). Fields: `ts` (unix ms), `level`, `event`, `request_id`, `method`, `route`
(the route template, never the concrete path), `status`, `duration_ms`, `tenant`,
`cache`, `principal`, and counts and sizes (`ids`, `hits`, `entries`, `packs`,
`bytes_in`, `bytes_out`). Never logged:
tokens, session ids, JWTs, OIDC claims, URL query strings, lookup ids, segment ids,
record or roster bytes. A canary test enforces this (testing.md).

## Tenant administration

`oxabl-cache-admin` talks to the admin listener with an operator token (`--token-file`).

```
oxabl-cache-admin tenant create acme --region na --hard-quota 20GiB --egress 500GiB
oxabl-cache-admin tenant list | show <t> | suspend <t> | resume <t> | delete <t>
oxabl-cache-admin tenant quota <t> --hard-quota 50GiB --rate 100
oxabl-cache-admin tenant bootstrap-admin <t> --issuer https://idp.example --subject <sub>
oxabl-cache-admin token create --tenant <t> --scopes tenant:admin --expires 30d
oxabl-cache-admin usage export --month 2026-10 --format csv > usage-2026-10.csv
oxabl-cache-admin cache scrub <c>
oxabl-cache-admin health
```

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
