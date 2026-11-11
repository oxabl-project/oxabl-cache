# Operations

One binary, `oxabl-cache-server`, runs every deployment shape. The operator never holds
a customer key; everything below works on ciphertext and account data.

Step-by-step guides:
- [Container and systemd](hosting/deploy-container.md): one VM, or two nodes on
  Postgres and S3.
- [Kubernetes](hosting/deploy-kubernetes.md): the Helm chart.
- [Hosted](hosting/deploy-hosted.md): Cloudflare R2 and Hetzner, with OpenTofu.
- [Fly.io](hosting/deploy-fly.md): one machine, SQLite on a volume and packs in R2.

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
  keeps working during the rollout. Destructive schema steps ship one release later. A server
  refuses a database that a newer release wrote ("database schema version N is newer than this
  release knows"), so roll back the binary together with a database backup, never alone.

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
grace = "24h"                                 # at least url_lifetime + 2s; keep hours of gap
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
The server warns at start (`<file> is readable by other users; chmod 600 it`) for each secret file it reads that
anyone but its owner and the process's own group can read: the TLS key, the Postgres password file, the store's
secret (the URL key of a local-disk store, the secret key of an S3 store; an S3 store does not read the URL
key) and the claim-hash key. Mode 0440 with the process's group, as a Kubernetes Secret volume
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
  start cold ("Losing an owner key", below).

### Restore drill

Run the drill every month on a spare host, and never on the live data. A backup that has never been
opened is not a backup. The drill proves three things: the copy opens, the packs match the metadata,
and a client still fetches and verifies.

1. **Back up the metadata.** The live server keeps serving. Name each file by date: an existing
   target is refused.

   ```sh
   oxabl-cache-server --config /etc/oxabl-cache/server.toml backup --to /backups/meta-2026-11-11.db
   {"backup":"/backups/meta-2026-11-11.db"}
   ```

   Copy the file off the host. On Fly, daily volume snapshots add a second copy of the volume, but
   keep the `backup` job: a snapshot you never opened has the same problem.
2. **Back up the packs.** Local disk: `rsync -a /var/lib/oxabl-cache/objects/ /backups/objects/`,
   without `--delete`. S3: rely on the provider's durability, and turn on versioning if you want to
   undo a deletion.
3. **Keep the server's key files** in a secret manager: `url_key_file` and `claim_hash_key_file`. The
   claim-hash key cannot be rotated without re-entering every trust policy, so a restore without it
   loses them.
4. **Restore on the spare host.** Put the metadata copy where `[metadata] url` points, the packs where
   `[store] root` points, and the two key files where `[keys]` points. Use the production
   configuration with other listen addresses. Start it with `serve`.
5. **Check it.** Every line must match:

   ```sh
   oxabl-cache-admin health
   metadata   ok
   store      ok
   gc_leader  true

   oxabl-cache-admin tenant list          # the tenants you expect

   oxabl-cache-server --config drill.toml scrub
   {"cache":"c_6v2k9q3m8x1r4t7w0y5z2b8n4d","corrupt_segments":0,"entries_removed":0}

   OXABL_CACHE_SERVER=https://drill.example.com oxabl-cache verify --sample 20
   sampled      20
   verified     20
   not trusted  0
   ```

   `scrub` prints one line per cache. A non-zero `corrupt_segments` means a pack is damaged or
   missing: restore that pack from its copy. `verify` signs in with a real token and fetches real
   entries, so it proves tokens, roster and packs work together.
6. **Stop the spare server** and delete its copy of the data.

A real restore replaces `meta.db` on the live host while the server is stopped (above). After it:

- Revocations made since the backup are undone. Revoke those tokens again.
- A roster older than the `min_roster_version` in the repository makes clients refuse the cache. The
  owner publishes new roster versions (for example with `rotate`) until it is above the pin.
- A newer database than the bucket leaves entries that name missing packs. They give misses, never
  wrong hits (see "Restore semantics" above).

For Postgres, restore a base backup and its WAL to a spare instance, point a spare server at it and
the bucket, and run the same checks.

## Members, keys and tokens

All of these are owner or tenant administrator actions. The server never holds a key. Owner commands
need a token with `cache:admin`; token commands need `tenant:admin`.

### Rotating a member

Do this when a person leaves, or when a CI identity may have leaked.

1. Remove the member. The owner signs a roster without them in a new epoch:

   ```sh
   oxabl-cache --token-file owner.token member remove alice
   published roster v6 at epoch 2
   set in .oxabl/cache.toml:
   min_roster_version = 6
   then revoke the departed member's tokens and sessions on the server
   ```

   Add `--purge` to mark every entry of an older epoch dead. That closes the window in which a
   removed member who also steals the bucket could read old entries (data-handling.md).
2. Commit the new `min_roster_version` in `.oxabl/cache.toml`. Until you raise the pin, a client that
   holds no local state (a CI runner) accepts an older roster.
3. Revoke the member's tokens. The tenant administrator lists them, then deletes by id. A deleted
   token answers 401 on its next call:

   ```sh
   curl -sS -H @acme-admin.hdr https://cache.example.com/v1/tenant/tokens | jq -c '.tokens[] | {id,name,expires_at,revoked_at}'
   curl -sS -X DELETE -H @acme-admin.hdr https://cache.example.com/v1/tenant/tokens/<id>
   ```

   A person who signed in with an identity provider also has a user. `DELETE /v1/tenant/users/<id>`
   revokes their sessions (protocol.md, "Tenant administration").
4. For a `ci` member, also make a new identity, add it, make its token, replace both secrets in the CI
   system and revoke the old token (ci.md). A `ci` key that leaked signs entries clients accept, until
   the roster drops it.
5. Expect a cold cache. Lookup ids change with the epoch, so the next fill job rewrites everything and
   each developer fetches once more.

**Check.** The removed member's identity can no longer open the cache, and the others can:

```sh
oxabl-cache --token-file alice.token status          # as alice, with a token that is still valid
cache  off
reason  the identity is not in the roster
```

### Rotating the epoch

`rotate` starts a new epoch for the same members. Use it when a key may have leaked but nobody
left, or after a restore (above):

```sh
oxabl-cache --token-file owner.token rotate --purge
published roster v7 at epoch 3
set in .oxabl/cache.toml:
min_roster_version = 7
then revoke the departed member's tokens and sessions on the server
purged 1 entries of older epochs
```

The output repeats the hint about a departed member. A rotation removes nobody, so skip it. Commit
the new `min_roster_version`. The cache is cold until the next fill job. Until it runs, every fetch misses.
Without `--purge`, entries of older epochs stay until retention removes them, and `verify --sample`
can draw one and report `miss         corrupt: 1`. That is a stale entry, not damage.

### Renewing tokens

A token expires. The default life is 90 days, and the longest is one year. Renew before the date. The
tenant administrator reads every expiry:

```sh
curl -sS -H @acme-admin.hdr https://cache.example.com/v1/tenant/tokens | jq -r '.tokens[] | [.name, .expires_at] | @tsv'
```

`expires_at` is unix seconds. To renew one token:

1. Make a new token with the same name, scopes and caches (ci.md, step 3).
2. Replace the secret where the old one lived: the CI secret store, the developer's token file, the
   owner's token file.
3. Run `oxabl-cache status` with the new secret and check that the cache is `on`.
4. Revoke the old token (`DELETE /v1/tenant/tokens/<id>`).

Other credentials:

- **Tenant administrator token:** `oxabl-cache-admin token create --tenant <t> --expires 30d`, then
  revoke the old one as above.
- **Operator token:** `oxabl-cache-admin operator-token create`, then `operator-token revoke <id>`.
  The server refuses to revoke the last valid operator token, and `init` never mints another, so
  always create the new token first.
- **Sign-in sessions** need no renewal: `oxabl-cache login` makes a new one (12 hours).
- **Identities** do not expire.

### Losing an owner key

- **You have another owner key.** The other owner removes the lost key. This starts a new epoch, so
  the cache turns cold once:

  ```sh
  oxabl-cache --token-file owner.token --identity-file /media/offline/identity \
    owner remove <label of the lost owner> --purge
  published roster v8 at epoch 4
  set in .oxabl/cache.toml:
  owner_keys = ["ed25519:rM09A6LI2gBy8TBi+6s8EnXIISlRCXUXa0Bjpic0fuU"]
  min_roster_version = 8
  and register the keys on the server, as the tenant administrator:
  PUT /v1/tenant/caches/c_6v2k9q3m8x1r4t7w0y5z2b8n4d/owner-keys {"owner_keys":["accd3d03…"]}
  purged 0 entries of older epochs
  ```

  Then register the key list on the server with the printed call, commit the new
  `.oxabl/cache.toml`, and add a new second owner key (owner.md, step 4). A branch whose committed
  file pins only the removed key rejects every new roster and builds without the cache until that
  file is updated there. `status` says why. This fails closed.
- **A key leaked.** Do the same, and do it at once. Revoke the owner's tokens too.
- **Every owner key is lost.** The roster can no longer change, and nobody can restore it. Make a new
  cache: a new owner identity, `POST /v1/tenant/caches` with its key, `init`, new
  `.oxabl/cache.toml`, and add every member again. Delete the old cache with
  `DELETE /v1/tenant/caches/<c>` once the new one works. The new cache starts cold.

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

## When something fails

Find the message, read the cause, apply the fix. The client prints one line on stderr and exits 1
(refused or failed) or 2 (usage or configuration). `oxabl-cache status` also shows why a cache is off
and the last five failures. A build never fails because of the cache: it compiles what it could not
fetch.

### Credentials and files

| You see | Cause | Fix |
| --- | --- | --- |
| `no identity at <path>: create one with oxabl-cache identity new or pass --identity-file` | No identity file, and `OXABL_CACHE_IDENTITY` is not set. | Run `identity new`, or set the variable (CI), or pass `--identity-file`. |
| `no terminal to ask the passphrase on: pass --passphrase-file or set OXABL_CACHE_IDENTITY_PASSPHRASE` | An identity file needs its passphrase, and the job has no terminal. | In a job, use the identity string in `OXABL_CACHE_IDENTITY`. Elsewhere, pass `--passphrase-file`. |
| `identity: cannot unlock the identity file` | The passphrase is wrong. Often it belongs to another identity file, such as the owner's against a CI file. | Use the passphrase of the file you pass. Passphrases cannot be recovered. |
| `<file>: readable by group or others; chmod 600 it` | A secret file has group or other permission bits. | `chmod 600` the file. |
| `invalid input: not a member descriptor` | The descriptor is cut off. The real one is about 130 characters. | Copy all of it. |
| `no credentials: set OXABL_CACHE_TOKEN or log in` | No token, no ID token and no stored session. | Set `OXABL_CACHE_TOKEN`, pass `--token-file`, or run `login`. |
| `the cache server refused the credentials` | The token is wrong, expired or revoked, or the session ended. | Renew it ("Renewing tokens"), or run `login` again. |
| `the cache does not exist or is not granted` | The cache id in `.oxabl/cache.toml` is wrong, or the token holds no grant on that cache. | Check the id. Make a token that names the cache. |
| `the cache server is unreachable` | The server is down, the `server` URL is wrong, or the network or TLS failed. | `curl -i <server>/v1/health` answers 204 when it is up. For a private CA, pass `--ca-file`. |
| `the server URL must be https, or http on a loopback address` | `server` is plain `http` for a non-loopback host. | Use `https`. |

### The repository's pins and the roster

| You see | Cause | Fix |
| --- | --- | --- |
| `no .oxabl/cache.toml here or above: run in the repository or pass --repo` | The command ran outside the repository. | `cd` into it, or pass `--repo`. |
| `OXABL_CACHE_CACHE cannot override a trust field of .oxabl/cache.toml` | `OXABL_CACHE_TENANT`, `_CACHE`, `_OWNER_KEYS` or `_MIN_ROSTER_VERSION` is set. | Unset it. Only the committed file sets these. |
| `invalid cache configuration: owner_keys holds ed25519: and unpadded base64` | An owner key has padding, a missing prefix or the URL-safe alphabet. | Copy the key as the command printed it. |
| `roster: no valid signature by a pinned owner key` | No key in `owner_keys` signed the roster. A key was replaced or removed, or the pin is wrong. | Fix `owner_keys` in the file on that branch (see "Losing an owner key"). |
| `roster: version is below the pinned minimum or the high-water mark` | The server's roster is older than `min_roster_version`. The database may have been restored from an old backup, or a server withholds the newer roster. | After a restore, publish new roster versions (`rotate`). Otherwise investigate the server. Never lower the pin to silence it. |
| `the cache has no roster yet` | The owner has not run `init`. | Run `init` (owner.md, step 3). |
| `the identity is not in the roster` | The owner has not added you, or has removed you. | Send your descriptor to an owner. |
| `the identity is not a ci member of the roster` | The job runs in `ci` mode with a `developer` or `reader` identity. | Use the `ci` identity, or set `OXABL_CACHE_MODE=read-only`. |
| `the cache already has a roster` | `init` ran twice. | Use `member add`. |
| `the identity is not an owner of the roster, or its signing key is not in owner_keys` | A member command ran with an identity that is not an owner. | Use the owner's identity. |
| `the server refused the credential: publishing a roster needs cache:admin` | The token lacks `cache:admin`. | Make a token with `read` and `cache:admin`. |
| `another roster was published first; repeat the change` | Two owners changed the roster at once. | Run the command again. |

### Reads and writes

| You see | Cause | Fix |
| --- | --- | --- |
| `not stored: failed: rejected` | The token lacks the scope for the namespace: `write:trusted` for `ci`, `write:personal` for a developer. | Make a token with the right scope. Role and scope must agree. |
| Every fetch misses (`miss: notfound`) | The cache is cold: a rotation, a change of schema, toolchain or compile key, or retention removed unused entries. | Run the fill job on the protected branch. |
| `sampled 0`, exit 1 | `verify` found no trusted entry. | Run the fill job, then verify again. |
| `miss         corrupt: 1` in `verify` | The sample drew an entry of an older epoch, or a damaged pack. | `rotate --purge`, or wait for the fill job. If it persists, run `scrub` on the server. |
| `403 quota_exceeded` in a server log, misses in the client | The tenant's monthly egress quota is spent. | Raise it with `tenant quota`. |
| `429 rate_limited` | The tenant's request rate is spent. | Retry, or raise `--rate` and `--burst`. |
| `503 unavailable` | The metadata store or the object store is down. | Check `/readyz` on the metrics listener. |

The protocol's error table lists every status and code (protocol.md, "Error envelope").

### The server and the admin command

| You see | Cause | Fix |
| --- | --- | --- |
| `server.listen <addr> is not a loopback address: configure [tls] cert_file and key_file, or set tls.terminated_upstream = true` | A public bind without TLS. | Add `[tls]`, or set `terminated_upstream = true` behind a proxy. |
| `cannot bind server.listen <addr>: Address already in use` | Another process holds the port, or a second server runs. | Stop it, or change the address. The same message names `admin_listen` and `metrics_listen`. |
| `unknown field ... expected url_key_file` | A secret written inline in the configuration. | Use the `*_file` key, with a file of mode 0600. |
| `<file> is readable by other users; chmod 600 it` (warning at start) | The URL key file has group or other bits. | `chmod 600` it. |
| `an operator token already exists; init mints the first one only` | `init` ran twice. | Use the token you stored, or mint another with `operator-token create`. |
| `database schema version N is newer than this release knows` | An older binary meets a database that a newer release wrote. | Run the newer release, or restore a database backup from before the upgrade. |
| `bad request: <file> already exists` (from `backup`) | The target exists. | Name the file by date. |
| `oxabl-cache-admin: unauthenticated` (exit 1) | The operator token is wrong, expired or revoked. | Use a valid token file. |
| `oxabl-cache-admin: server unreachable` (exit 3) | Wrong `--server`, or the admin listener (loopback by default) is out of reach. | Run it on the host, or through a tunnel. |
| `/readyz` answers 503 | The database or the object store failed its 2 s probe. The body names which. | Fix that dependency. The server recovers on its own. |
