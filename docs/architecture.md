# Architecture

oxabl-cache is a remote compile cache for oxabl. A client library inside oxabl turns
compile keys into opaque lookup ids, seals artifact sets, and moves sealed bytes
between the build machine and an object store. A server authenticates callers, keeps
the index of sealed entries, enforces quotas and retention, and signs short-lived
object URLs. The server never holds a key that opens anything it stores.

Companion documents: [security.md](security.md) (threat model, cryptography),
[protocol.md](protocol.md) (HTTP API and byte formats), [client.md](client.md)
(client library), [operations.md](operations.md), [testing.md](testing.md), the
decision records in [adr/](adr/), and the cost model in
[hosting/provider-analysis.md](hosting/provider-analysis.md).

## Requirements map

| Requirement | Design element |
| --- | --- |
| Rust; small, single-purpose files; brief doc comments only | Crate layout below. Each crate has one responsibility, and modules are named for what they hold. |
| Black-box tests only, fuzzing welcome | testing.md: a harness over binaries, HTTP and the public client API; adversarial suite; fuzz targets |
| Zero-knowledge storage | security.md: client-side sealing, keyed lookup ids, Padmé padding, a field-by-field visibility table |
| Zero-trust integrity | security.md: signed records, trust tiers, roster pinned in the customer's repository |
| Corruption impossible | "Write and read paths", "Corruption-proofing summary" |
| No vendor lock-in | Portable S3 subset and local disk behind one port; provider extras in separate crates |
| Simple scaling | Stateless API nodes; SQLite to Postgres migration command |
| Performance and value | Packs on R2 with presigned URLs (ADR 0002); "Performance budget" |
| Tenants, caches, tokens, OIDC, quotas, usage | Domain model; protocol.md authentication and administration; operations.md usage ledger |
| One cache per codebase, content-addressed, shared across branches | Entries keyed by lookup id, independent of branch; no manifests (ADR 0007) |
| r-code is not deterministic | First write wins; trust comes from signatures, not reproducibility (ADR 0005) |
| Embedded source paths | Compile keys and relative-path compiles are oxabl's responsibility; the cache stores any valid output and verifies who produced it |

## Style: hexagonal core, small domain, one append-only ledger

The server is a hexagonal (ports and adapters) service: one core crate holds the
domain types, the use cases and the port traits; every piece of I/O (HTTP, SQL, disk,
S3, OIDC) is an adapter crate that depends on the core, never the reverse.

- **Why hexagonal.** The product runs on two metadata stores (SQLite, Postgres) and
  two byte stores (local disk, S3-compatible) with identical behaviour. Ports make
  that a property of the build: the core cannot name an adapter, and one black-box
  suite runs against every adapter combination.
- **Why a small domain, not full DDD.** The domain has few invariants (first write
  wins, nothing live is deleted, quotas, scopes). Aggregates are plain structs with
  the methods that guard those invariants; no domain events, factories or
  specification objects.
- **Why not event sourcing.** Entries are immutable inserts and the only mutable
  state is retention bookkeeping. Event sourcing would add projections, replay and
  snapshots for no invariant it protects. The one append-only piece is the usage
  ledger, because billing needs idempotent, auditable sums.
- **Why not a plain layered service.** Layers let SQL and S3 details leak upward;
  with four adapters that is where divergence bugs would live.

ADR: [0001](adr/0001-hexagonal-core.md).

## Domain model

| Concept | Meaning | Server can read |
| --- | --- | --- |
| Tenant | A customer account: status, quotas, users, OIDC trust policies. | yes |
| Cache | One per codebase. Random id (`c_` + 26 base32 chars), display name, retention, quota, roster version, dictionary pointer. | yes |
| Principal | Who calls: a **user** (human OIDC identity), a **workload** (CI OIDC trust policy) or the owner of a **token**. | yes |
| Grant | Principal × cache × scopes (`read`, `write:personal`, `write:trusted`, `cache:admin`); tenant-wide `tenant:admin`. | yes |
| Namespace | Where an entry lives: `trusted` (CI) or `personal(user)` (one per developer). | yes |
| Entry | `(cache, namespace, lookup_id) → sealed record`, plus the ids of the segment and dictionary it names, its epoch and sizes. Immutable once committed. | ids and sizes |
| Segment | One sealed, compressed artifact set (kind `artifact`) or a compression dictionary (kind `dictionary`), named by the BLAKE3 hash of its sealed bytes. Lives at `(pack, offset, length)`. A segment is **live** if and only if `dead_since IS NULL`. | id, kind, size, location |
| Pack | An immutable object of 5 to 32 MiB holding segments back to back, with a plaintext footer index of segment ids and offsets. State `pending`, `live` or `deleting`. Committed exactly once. | id, size, live bytes, ETag |
| Roster | Signed, versioned list of the cache's members and their public keys, plus the cache root key wrapped to those members. Opaque to the server except its version. | version |
| Epoch | Generation of the cache root key. Rotation starts a new epoch, and lookup ids change with it. | number |
| Usage | Append-only ledger rows: tenant, cache, day, metric, value. | yes |
| Quota | Per tenant and per cache: live bytes (soft and hard), a stored-bytes ceiling, monthly egress bytes, request rate. | yes |

Invariants the core enforces:

1. An entry is inserted at most once per `(cache, namespace, lookup_id)`. The first
   commit wins; later commits are told `exists`. The database arbitrates this; the
   object store is never asked for conditional writes.
2. A pack is committed exactly once: by the principal that uploaded it, while it is
   `pending`, and after it is verified present at its declared size. The pack flips
   to `live` in the same transaction. The server records that commit's request
   digest and per-entry statuses. A repeat with the same digest is answered from the
   record and changes nothing. A different body gets 409 `pack_closed`.
3. A segment id is unique within a cache. A commit may name a segment id only if it
   is new to the cache. Retries never reach insertion (invariant 2), and no path ever
   clears `dead_since`: a dead segment is never revived.
4. A pack is deleted only when no entry names any of its segments and it has been
   dead for the grace period, or it is `pending` with an expired lease.
5. Writes into `trusted` need `write:trusted`; writes into `personal(u)` need
   `write:personal` and the caller to be `u`. A pack never mixes namespaces. Reads see
   `trusted` and the caller's own `personal` namespace only.
6. Every query carries a tenant scope; a cache id from another tenant is "not found".

## Crate layout

Every crate is named `oxabl-cache-<part>`. oxabl depends only on the published
`oxabl-cache-client` (`oxabl_cache_client`).

```
crates/
  oxabl-cache-format         byte formats: magic, versions, encode/decode, size limits; no keys
  oxabl-cache-crypto         client-only: key hierarchy, lookup ids, seal/open, signatures, roster
  oxabl-cache-client         public client library used by oxabl
  oxabl-cache-cli            `oxabl-cache` binary (member CLI): identities, roster, rotation, login
  oxabl-cache-core           domain, use cases, ports (MetadataStore, ObjectStore, IdentityVerifier, Clock)
  oxabl-cache-meta-sqlite    MetadataStore on SQLite (single node)
  oxabl-cache-meta-postgres  MetadataStore on Postgres (multi node)
  oxabl-cache-store-fs       ObjectStore on a local directory; its URLs are served by oxabl-cache-http
  oxabl-cache-store-s3       ObjectStore on the portable S3 subset (SigV4, presigned URLs, Range)
  oxabl-cache-oidc           IdentityVerifier: JWT and JWKS validation, issuer guard, trust policies
  oxabl-cache-http           axum routes, extractors, error envelope, rate limiting
  oxabl-cache-server         `oxabl-cache-server` binary: config, wiring, serve, migrate, backup, scrub
  oxabl-cache-admin          `oxabl-cache-admin` binary: operator CLI over the admin API
tests/e2e                    black-box harness (testing.md)
fuzz/                        cargo-fuzz targets
```

Dependency direction (arrows mean "depends on"):

```
oxabl-cache-cli ─► oxabl-cache-client ─► oxabl-cache-crypto ─► oxabl-cache-format
oxabl-cache-server ─► oxabl-cache-http ─► oxabl-cache-core ─► oxabl-cache-format
oxabl-cache-server ─► {meta-sqlite, meta-postgres, store-fs, store-s3, oidc} ─► oxabl-cache-core
oxabl-cache-admin ─► (HTTP only; no server crate)
```

`oxabl-cache-format` is the only crate both sides share. It parses outer headers and pack
footers (magic, version, sizes, offsets) so the server can reject malformed input and
compact packs, but it handles no keys. A build gate (testing.md) fails if
`oxabl-cache-server`'s dependency tree contains `oxabl-cache-crypto`, `oxabl-cache-client` or
`age`.

The S3 adapter uses only the portable subset: `PutObject`, `GetObject` with
`Range`, `HeadObject`, `DeleteObjects`, `ListObjectsV2`, presigned PUT and GET.
It is built on `rusty-s3` for signing plus `reqwest`. The AWS SDK is rejected: it is
large and AWS-shaped, and its default checksum behaviour breaks other S3-compatible
stores. A provider feature beyond the subset (Azure Blob's own API, R2 jurisdictions)
goes into its own `oxabl-cache-store-<provider>` crate, never into the core. The adapter
probes the bucket at startup (presigned round trip, ranged read, batch delete, list,
ETag on `HEAD`) and refuses to start on a failure. Two capabilities are optional and
detected by the probe: `If-None-Match: *` on PUT and `x-amz-checksum-sha256`. When
present, the server signs them into PUT URLs as extra guards; correctness never
depends on them.

## Storage

### Bytes go directly between client and store

In S3 mode the server hands out presigned PUT and GET URLs (15-minute lifetime) and
bytes never pass through an API node. Proxying would make the API nodes
bandwidth-bound and, on hyperscalers, pay egress twice
(hosting/provider-analysis.md, table 3.2).

In local-disk mode the server is the store. It issues URLs of the same shape,
pointing at its own `/v1/caches/{cache}/objects/{pack_id}` route and signed with an
HMAC key only the server holds. The client path is identical in both modes: it
receives URLs and performs PUT and ranged GET on them.

### Packs of independently sealed segments

Each artifact set (the `.r` plus optional XREF and debug list) becomes one
**segment**: compressed with zstd, padded, sealed, named by `BLAKE3(sealed bytes)`.
A writer appends segments into **packs** with a target of 16 MiB and a hard cap of
32 MiB; a segment larger than the target gets a pack of its own. The server assigns
each pack a random 128-bit id, so object keys are unique and never rewritten, and no
conditional PUT is needed. Readers fetch segments with ranged GETs on one presigned
URL per pack (the range is a header, so one URL serves any range), coalescing
neighbouring segments with gaps of 1 MiB or less.

Per tenant-month in the hosting model (200 CI builds, 95 cold fetches,
750 incremental fetches), from hosting/provider-analysis.md section 1:

| | One object per artifact set | Packs (chosen) |
| --- | --- | --- |
| Class A requests (PUT) | 80,000 | ~490 |
| Class B requests (GET) | 1,100,000 | ~21,800 |
| R2 bucket bill at 100 tenants | $75 | $7 |
| Cold fetch of 10,000 sets | 10,000 GETs; 157 rounds at concurrency 64 | ~72 to 120 ranged GETs, bandwidth-bound |
| GC | delete an object when its entry dies | compaction (below) |

One object per artifact set is rejected: on R2 it costs 10x the bucket bill, and the
latency floor of 10,000 requests per cold fetch becomes visible on any link over
50 ms RTT. Packs cost compaction, which the server can do **without keys**, because
records name segments by hash and never by location.

The cross-file compression gain (zstd at 5x per file against 10.5x over a long
window) is taken by a **per-cache zstd dictionary**. CI trains a 112 KiB dictionary
from sampled artifacts and stores it as a sealed segment, and each entry names the
dictionary it was compressed with. Segments stay independently decodable, so ranged
reads still work. On synthetic data a trained dictionary raised the ratio from 2.27x
to 2.90x. The default is gated on a measured gain of at least 1.3x on a real codebase
(operations.md, "Compression dictionary"). Compressing a whole pack as one stream is
rejected: a 50-artifact incremental fetch would then download and decompress whole
packs. ADR: [0002](adr/0002-packed-segments.md).

### Object layout

S3: `v1/<cache-id>/<pack-id-hex>`. Local disk:
`<root>/v1/<cache-id>/<pack-id[0..2]>/<pack-id-hex>`, with a sibling `tmp/`
directory. Packs are immutable: never modified, appended or overwritten.

## Metadata: SQLite for one node, Postgres for many

Both run behind the `MetadataStore` port with the same schema and a common SQL
subset. The black-box suite runs in full against both.

- SQLite (WAL, `synchronous=FULL`) keeps the single binary self-contained for a team
  that runs one node on local disk.
- Postgres is what stateless API nodes share. Its transactional inserts and
  `= ANY($1)` index probes answer a 10,000-id lookup in tens of milliseconds.
- redb is rejected: a key-value store needs a second, hand-written query layer with
  no SQL parity to Postgres. Postgres-only is rejected because the single binary
  would then need a database server. SQLite-only is rejected because nodes cannot
  share it.

Growth from one node to many is `oxabl-cache-server migrate --from a.toml --to b.toml`.
It runs offline and copies metadata (SQLite to Postgres) and packs (disk to S3),
re-hashing every segment against its id from the pack footer and comparing row counts
per table. It refuses to finish on any difference. The schema uses no SQLite-only
types (ids are bytes or text, times are integer seconds), so nothing is converted.
During the window, builds see misses, never errors. The path has its own black-box
test. ADR: [0003](adr/0003-metadata-sqlite-and-postgres.md).

Core tables: `tenants`, `caches`, `users`, `workload_policies`, `tokens`,
`sessions`, `grants`, `entries`, `segments`, `packs`, `rosters`,
`dictionary_pointers`, `usage_ledger`.

- `entries`: key `(cache_id, ns, lookup_id)`, where `ns` is `0` for trusted and the
  user id for personal. Columns `segment_id`, `dict_id`, `epoch`, `record` (sealed
  bytes), `created_at`. Insert-only.
- `entry_access`: same key, one column `access_day`, index `(cache_id, access_day)`.
  The row is inserted with the entry at commit (`access_day` = the commit day), so
  every entry has one.
  It is a narrow table, so the daily touch never rewrites a row that carries a
  512-byte record.
- `segments`: key `(cache_id, segment_id)` (the uniqueness of invariant 3); columns
  `pack_id`, `offset`, `length`, `kind` (`artifact` or `dictionary`), `created_at`,
  `dead_since`. Live means `dead_since IS NULL`.
- `packs`: key `(cache_id, pack_id)`; columns `ns`, `uploader`, `client_ref`, `size`,
  `etag`, `live_bytes`, `state`, `lease_until`, `dead_since`, `commit_digest` (BLAKE3
  of the `OXCM` body) and `commit_statuses` (the `OXCA` bytes returned).
- `caches` carries `live_bytes` (sum of live segment lengths) and `pending_bytes`,
  updated in the same transactions as the rows they sum.
- `nodes`: `node_id`, `heartbeat_at`, written every 10 s, for the live node count
  (rate limits, operations.md).

## Write and read paths

**Write.** The commit order is fixed: bytes are durable before the entry that names
them.

1. The client seals each artifact set into a segment and appends segments into packs
   locally (client.md).
2. `POST uploads` declares each pack's size. The server reserves quota, inserts a
   `pending` pack row with a 1-hour lease, and returns a pack id and a presigned PUT.
   A PUT URL is issued only while the pack is `pending` and has never been committed.
   A `client_ref` retry after the first commit returns `committed` and no URL. Where
   the probe found `If-None-Match`, it is signed into the PUT, so the store also
   refuses a second write.
3. The client PUTs the pack. S3 acknowledges only durable objects. The local adapter
   writes into `tmp/`, fsyncs, renames and fsyncs the directory before answering 200,
   and refuses any PUT once the pack is not `pending`.
4. `POST commit` for that pack, once: the segment table (id, offset, length, kind)
   and up to 2,000 sealed records. Under a row lock on the pack, the server first
   handles a repeat. If the pack already has a `commit_digest`, it re-checks the
   object's ETag; a different ETag gets 409 `pack_changed` and queues a scrub. It
   then returns `commit_statuses` if the digest matches, or 409 `pack_closed` if it
   does not. Otherwise it checks:
   - the pack is `pending` (otherwise 409 `pack_closed`);
   - it belongs to the caller and to the namespace in the frame;
   - `HEAD` (or `stat`) reports exactly the declared size. The ETag is recorded;
   - every segment is in bounds, and no two overlap;
   - every segment id is new to the cache (invariant 3). A segment id already
     recorded makes that segment, and every entry naming it, `invalid`. This stops a
     writer from claiming another namespace's segment;
   - a `dictionary` segment appears only in a `trusted` commit by a caller with
     `write:trusted`, and no entry names it as its own segment.

   In one transaction the server then:
   - inserts the segments. An `artifact` segment whose entries all lost or are
     `invalid` gets `dead_since = now`. A `dictionary` segment is exempt: it is
     inserted live and referenced by nothing yet (see "Mark");
   - inserts the entries with `ON CONFLICT DO NOTHING`, and an `entry_access` row
     (`access_day = today`) for each entry that won;
   - sets the pack `live` and credits `live_bytes` (pack and cache) for every live
     segment: winning artifact segments and every dictionary segment;
   - records `commit_digest` and `commit_statuses`.

   If the pack's credited `live_bytes` is 0 after the commit (every entry lost),
   the pack gets `dead_since = now` in the same transaction, so the sweep removes it
   after the grace. Packs of losing writers never linger. The server returns
   `committed`, `exists` or `invalid` per entry, and returns exactly the same bytes
   to any retry.

A crash before step 4 leaves a `pending` pack that the lease expiry reclaims. A crash
inside step 4 rolls the transaction back. Concurrent writers of the same key both
upload; one insert wins, every later reader sees the winner, and the loser's segment
is dead from birth and reclaimed by compaction or pack deletion. The server never
reads pack contents on the write path. A pack whose bytes do not match its declared
segment ids produces misses on read and is then removed by the report path below.

**Read.** `POST lookup` returns, per id, presence or the sealed record with its
segment location, plus one presigned GET per pack involved. The client opens and
verifies the record, range-reads the segment, checks `BLAKE3` against the signed
segment id, decrypts and decompresses into temporary files beside the targets, checks
the zstd frame checksum and the signed sizes, and only then renames. Any failure is
a miss (client.md, "Install").

**Report.** A client that finds a segment whose bytes do not hash to its id posts a
report. The server range-reads that segment itself, hashes it, and only on a mismatch
deletes every entry naming a corrupt segment of that pack after verifying the whole
pack. The next producer then rewrites the key. Reports that the server cannot
confirm (a signature or key failure on the client) are counted and change nothing.
Each principal may have at most 10 report verifications per minute.

## Garbage collection, compaction and retention

GC runs inside the server, on one node at a time, elected with a Postgres advisory
lock (with SQLite, the single process runs it). ADR: [0006](adr/0006-gc-leases-grace.md).

- **Retention.** An entry whose `access_day` is older than the cache's retention
  (default 30 days) is deleted. Lookup hits set `entry_access.access_day` at most
  once per entry per day, batched per node every 10 s.
- **Quota eviction** works on **live bytes**, the sum of the lengths of segments that
  entries still name. Deleting an entry lowers that sum in the same transaction, so
  eviction always reaches its target. When live bytes exceed 90% of the hard quota,
  the oldest entries by `access_day` are deleted until live bytes are at or below
  80%. Personal namespaces go before trusted. Bytes still waiting for compaction or
  the grace are reported separately as `reclaimable_bytes` and never drive eviction.
- **Epoch purge.** After a key rotation, `cache:admin` may delete every entry of
  epochs below N; otherwise retention expires them.
- **Mark.** Deleting an entry, in the same transaction, sets `dead_since` on its
  artifact segment and subtracts the segment's length from its pack's `live_bytes`.
  A dictionary segment is referenced by the dictionary pointer and by every entry
  whose `dict_id` names it. It gets `dead_since` only when neither references it:
  - when the last such entry is deleted while the pointer names another
    dictionary;
  - when the pointer moves away while no entry names it;
  - when it was committed but never referenced and is older than the grace (a
    publish whose pointer update never happened).

  A pack whose `live_bytes` reaches 0 gets `dead_since = now` under the same lock.
- **Compaction** (every 10 min). Candidates are `live` packs whose live fraction is
  below 50%, that are older than the 24 h grace, and that hold at least 4 MiB in
  total. The worker range-reads each live segment, verifies `BLAKE3` against its id
  (corrupt ones follow the report path), and writes the survivors into a new pack
  of the same namespace with a fresh id and footer. Its reads use `If-Match` with the
  recorded ETag. The new pack is recorded as committed by the GC, so it never accepts
  a client commit. One transaction then moves each segment's location if it is
  still in the source pack and still live (`dead_since IS NULL`), keeping its kind,
  credits the new pack, and sets the source pack's
  `live_bytes = 0, dead_since = now`. Compaction never needs a key,
  never changes a segment's bytes, and never changes a record.
- **Sweep** (every 10 min). In one transaction, select packs with `dead_since` older
  than the grace, or `pending` with an expired lease, and set them `deleting`. Then
  delete the objects, then the rows.
- **Safety.** Deleting an entry never touches bytes. A commit names only the
  caller's own `pending` pack plus, through `dict_id`, a dictionary segment that
  must be live. The transaction holds both rows' locks, so a commit and a sweep can
  never interleave on the same pack, and a dictionary cannot die while an entry is
  being committed against it. The grace (24 h) is far longer than a URL's
  lifetime (15 min), so a reader that resolved a location just before compaction or
  eviction still finds the bytes where its URL points.
- **Orphans.** Weekly, the sweeper lists the cache prefix and deletes objects that
  have no pack row and are older than twice the lease. This covers a lost database
  and a crash between deleting an object and deleting its row.
- **Quotas.** `uploads` refuses a pack when `live_bytes + pending_bytes + size`
  would exceed the hard quota, with two rules that protect the trusted tier:
  - **Personal cap.** Each user's personal namespace (its live plus pending bytes)
    is capped at 10% of the cache's hard quota. A personal upload over the cap is
    refused, so 25 developers doing full builds cannot fill the cache.
  - **Trusted admission evicts personal entries.** A `trusted` upload that would
    exceed the hard quota deletes personal entries, oldest `access_day` first, in its
    own admission transaction, until the pack fits. Personal bytes freed this way
    leave `live_bytes` at once (the usual mark rules), so CI is never starved by
    personal writes while it waits for the GC interval. Personal **pending** bytes
    cannot be evicted (they are mid-upload). A trusted pack that still does not fit
    once every personal live entry is gone is admitted anyway, as long as trusted
    live plus trusted pending bytes stay within the hard quota. The overshoot is
    bounded by the personal pending bytes, which are at most 10% of hard per user
    and expire with their 1-hour lease, and by the stored-bytes ceiling below. A
    trusted pack is refused only when trusted bytes alone would exceed the quota.

  `uploads` also refuses a pack when stored bytes (every pack byte
  that is `pending`, `live` or in grace) would exceed 3x the hard quota. That ceiling
  is an abuse guard only: compaction keeps steady-state storage under twice the live
  bytes, plus one grace window of churn. The client treats a refusal as "not stored",
  and a build never fails because the cache is full. Billing uses stored bytes
  (operations.md).

## Corruption-proofing summary

| Threat | Mechanism |
| --- | --- |
| Partial upload | Commit requires the stored size to equal the declared size. Where the store enforces checksums, the signed SHA-256 also rejects a wrong body. |
| Bytes not durable when named | Commit verifies the pack is present before inserting entries; the local adapter fsyncs before answering. |
| Crash between upload and commit | `pending` packs expire and are swept; no entry ever named them. |
| Concurrent writers | Unique pack ids; `ON CONFLICT DO NOTHING` on entries; the first commit wins. |
| Torn read | Packs are immutable. The client hashes each segment and authenticates every chunk before any rename. |
| Bit rot at rest | The client detects it and reports it; the server confirms and removes the entries. Compaction re-verifies everything it copies. A local-disk scrub re-hashes every pack weekly. |
| Wrong artifact | The signed record binds the compile key (through the lookup id), epoch, namespace kind, segment id and sizes. The segment id binds every byte. Any mismatch is a miss. |
| A writer claims another namespace's segment | Segment ids are unique per cache; such a commit is `invalid` (invariant 3). |
| A live pack rewritten after commit | PUT URLs exist only before the commit; `If-None-Match` where supported; the ETag is recorded at commit and checked by repeats, scrub and compaction. |
| Lost or rolled-back database | The orphan sweep reclaims bytes. Entries naming missing packs produce misses and reports, which delete them. Never a wrong hit. |

## Scaling

API nodes are stateless and share only Postgres and the object store. JWKS, roster
versions and rate-limit buckets are cached per node and rebuildable. In S3 mode an
API node moves only metadata: a 10,000-id lookup is ~320 KB in and up to ~5.3 MB out.
Compaction is the only byte traffic through a node: at most the live half of each
compacted pack, about 2 GB per tenant-month in the hosting model.

Throughput is bounded by Postgres. A 10,000-id probe is estimated at ~20 to 40 ms of
database CPU, so a 4-vCPU primary would serve about 100 to 200 such probes per
second. The hosting model needs far less: about 2,000 full-graph lookups (20,000 ids)
per tenant-month, so 500 tenants average 0.4 per second and peak at about
4 per second (10x), which is 8 probes of 10,000 ids. The prototype measures the
probe cost. The load test (testing.md) gates the modelled peak, and separately
requires headroom of 10x that peak (80 probes per second). Beyond that, read replicas
serve `lookup`, and tenants are sharded by tenant id across Postgres clusters behind
the same port; neither is built until load demands it. Local-disk mode is single-node by design; its growth path is
the migration above.

## Performance budget

Assumptions: 1 Gbit/s link (~117 MB/s of goodput), 30 ms RTT, warm TLS
connections (the first handshake adds ~60 ms once), object-store time to first
byte of 50 ms (to be measured in the prototype), and an 8-core client. A full set
is 10,000 artifact sets: 0.9 GB compressed, 0.92 GB sealed (padding plus tags),
4.3 GB plain, in ~72 packs, installed as up to 30,000 files (with XREF and debug
lists). CPU rates are from security.md, "Performance cost".

| Scenario | Breakdown | Estimate |
| --- | --- | --- |
| Full cold fetch | Open (OIDC exchange, roster, cache info): ~0.1 s. Lookup of 10,000 with records: 320 KB up, ~40 ms of database, 5.3 MB down: ~0.12 s. ~120 ranged GETs of ≤ 8 MiB over 32 connections: latency floor ~4 waves × 80 ms = 0.3 s, transfer 0.92 GB / 117 MB/s = 7.9 s. Decompression (4.3 core-s) plus security (0.8 core-s) is ~0.65 s on 8 cores, pipelined. Install: 4.3 GB of writes overlap the transfer. With `Durability::Caller` (oxabl's stage), one `syncfs` at the end flushes ~4.3 GB at ~1 to 2 GB/s: **+2 to 4 s**. With `Durability::PerFile`, 30,000 fsyncs at 32 in parallel cost ~1 s on local NVMe and ~4 s on cloud block storage (~4 ms each), mostly overlapped. | **~8.5 s** transfer-bound, **~11 to 12 s** with install made durable. With a 1.6x dictionary gain: ~5.5 s and ~8 s. |
| Incremental fetch of 50 | Lookup of 50: ~40 ms. Segments from 1 to 3 CI packs, worst case 50 parallel ranges: 30 ms RTT + 50 ms first byte + 4.6 MB / 117 MB/s ≈ 0.12 s. Install of 22 MB: milliseconds. | **~0.2 s** (+0.1 s open) |
| CI upload of 10,000 | zstd-3 of 4.3 GB at 200 to 350 MB/s per core: 12 to 22 core-s, plus ~1.7 core-s of input checks and sealing, 1.7 to 3 s on 8 cores, pipelined. 72 PUTs of ~12.5 MB, 8 in flight: 0.92 GB / 117 MB/s = 7.9 s. Each commit (HEAD + ~140-row insert) ~0.1 s, overlapped; the last one adds ~0.1 s. | **~8.5 to 10 s**, bandwidth-bound with ≥ 4 cores |
| 10 Gbit/s, same region (for scale) | Transfer ~0.85 s. CPU then dominates: decompression plus security is ~5.1 core-s, ~0.65 s on 8 cores. Security is ~15% of client CPU. | ~1.5 s plus install |
| Lookup of 20,000 | ids 1.6 ms. Two parallel requests of 10,000: 640 KB up (5.5 ms), ~40 ms of database each, presence replies of 50 KB each: ~0.08 s. Fetch mode returns 2 × 5.3 MB of records (~0.1 s) and verifies 20,000 signatures (0.7 core-s, ~90 ms on 8 cores). | **~0.08 s** plan, **~0.2 s** fetch |
