# Protocol

HTTP API and byte formats, version 1. The control plane uses JSON. The data plane
(lookup, commit) uses the binary frames defined below, because a 10,000-id lookup is
320 KB as binary and about 900 KB as hex JSON, and its records are incompressible
ciphertext anyway.

## Conventions

- Base path `/v1`. A breaking change is a new path prefix and new format versions.
- TLS 1.3 only. `Authorization: Bearer <token>` on every call except object URLs,
  which carry their own signature.
- Binary content type: `application/vnd.oxabl-cache.v1`. JSON: `application/json`.
- Every response carries `x-request-id`. Clients may send one; the server
  replaces it if it is not 26 base32 characters.
- Binary integers are big-endian. `bytes16` and `bytes32` are fixed-length raw bytes.
  `str16` is a `u16` length followed by UTF-8.
- Every binary structure starts with a 4-byte ASCII magic, a `u8` version and a `u8`
  flags byte. Unknown versions, unknown flag bits, non-zero reserved bytes and
  trailing bytes are rejected.
- Ids in JSON and URLs are lowercase hex (pack ids 32 characters, segment and lookup
  ids 64). Cache ids are `c_` plus 26 base32 characters, encoding 16 random bytes.
  Tenant ids are `t_` plus 26 base32 characters.

### Error envelope

Every non-2xx response, binary endpoints included, is JSON:

```json
{"error": {"code": "quota_exceeded", "message": "cache over hard quota", "retryable": false, "request_id": "01jc4z5a9r0d6e7f8g9h0k1m2n"}}
```

| Status | `code` | Retry |
| --- | --- | --- |
| 400 | `bad_request`, `unsupported_version` | no |
| 401 | `unauthenticated` (missing, expired or revoked credential) | after re-authenticating |
| 403 | `forbidden` (scope), `tenant_suspended` | no |
| 404 | `not_found` (also for another tenant's cache) | no |
| 409 | `conflict` (natural-key clash) | no |
| 412 | `precondition_failed` (`If-Match` on roster or dictionary) | after re-reading |
| 413 | `too_large` | no |
| 429 | `rate_limited`, with `Retry-After` | yes |
| 503 | `unavailable` (metadata or store down), with `Retry-After` | yes |
| 500 | `internal` | yes, with backoff |

Batch endpoints return 200 with per-item statuses. Whole-request errors are reserved
for authentication, malformed input and limits. Clients treat any error on a read
path as a miss.

### Limits

| Limit | Value |
| --- | --- |
| Lookup ids per request | 10,000 |
| Entries per commit, segments per pack | 2,000 |
| Pack size | 32 MiB (single PUT) |
| Files per artifact set; path length; total path bytes per set | 16; 1,024 bytes; 3,072 bytes |
| Sealed record | 4,096 bytes. Derived: 284 fixed bytes (72-byte header, 16-byte tag, 132-byte `OXEP` fields, 64-byte signature), plus 10 bytes per file, plus the paths. 284 + 16 × 10 + 3,072 = 3,516, which rounds up to 3,584. |
| Plaintext per artifact set (client-enforced on decode) | 64 MiB default, configurable up to 1 GiB |
| JSON body; roster | 1 MiB; 1 MiB and 1,000 members |
| Presigned URL lifetime; pending pack lease | 15 min; 1 h |
| Rate per tenant (default) | 50 requests/s sustained, burst 200, on `/v1` API routes. Object routes are excluded; their signed URLs are the limit. |

## Authentication

`POST /v1/auth/oidc` (no bearer) with `{"tenant": "t_…", "id_token": "<JWT>"}`
returns 200 `{"token": "oxs_…", "expires_at": 1793000000, "grants": [{"cache": "c_…", "scopes": ["read", "write:trusted"]}]}`.
It answers 401 if the token is invalid or no trust policy matches. Workload sessions
last 1 h and human sessions 12 h.

`DELETE /v1/auth/session` revokes the calling session (204).

Token formats: API tokens `oxc_<id 16 base32>_<secret 52 base32>`, sessions
`oxs_<…>`, operator tokens `oxo_<…>`. The prefixes allow secret scanning.

## Cache data plane

All routes are under `/v1/caches/{cache}`. Scope `read` is needed unless noted.

### `GET /v1/caches/{cache}`

```json
{"id": "c_…", "display_name": "…", "epoch": 3, "roster_version": 12,
 "dictionary_version": 2,
 "store": {"kind": "s3", "if_none_match": true, "checksum_sha256": false},
 "limits": {"lookup_ids": 10000, "commit_entries": 2000, "pack_bytes": 33554432},
 "quota": {"hard_bytes": 21474836480, "live_bytes": 5368709120, "pending_bytes": 0,
           "stored_bytes": 7516192768, "reclaimable_bytes": 2147483648}}
```

`live_bytes` drives eviction and admission. `stored_bytes` is what occupies storage
and is billed. `reclaimable_bytes` is the part of it that compaction and the sweep
will free.

### `GET|PUT /v1/caches/{cache}/roster`

GET returns the `OXRS` bytes with `ETag: "v<version>"`. PUT needs `cache:admin` and
`If-Match: "v<current>"`. The body's version must be `current + 1`, its epoch
`current` or `current + 1`, and at least one of its signatures must verify under an
owner key registered for the cache. It answers 204 on success, 412 on a stale
`If-Match`, and 400 on a malformed, unsigned or out-of-range body. Clients still
verify the signature against their own pinned keys.

### `GET|PUT /v1/caches/{cache}/dictionary`

The `OXDP` pointer, under the same `If-Match` rules. PUT needs `write:trusted`, and
the named segment must be `live` in this cache.

### `POST /v1/caches/{cache}/lookup`

Request `OXLQ`, response `OXLR`. Flag bit 0 asks for records (fetch mode); bit 1
asks for presigned GET URLs, one per pack involved. Without bit 0 the response
carries presence only (plan mode). Every hit updates the entry's `access_day` (in
`entry_access`, at most once a day). The
call is safe to retry.

### `POST /v1/caches/{cache}/uploads`

Needs `write:trusted` or `write:personal`, matching `namespace`.

```json
{"namespace": "trusted",
 "packs": [{"client_ref": "9f3c…(32 hex)", "size": 13107200, "sha256": "base64…"}]}
```

`sha256` is required only when `store.checksum_sha256` is true. The response, in
request order:

```json
{"packs": [
  {"status": "ok", "pack_id": "a1b2…", "url": "https://…", "method": "PUT",
   "headers": {"content-length": "13107200", "if-none-match": "*", "x-amz-checksum-sha256": "…"},
   "expires_at": 1793000900},
  {"status": "committed", "pack_id": "c3d4…"},
  {"status": "quota_exceeded"}
]}
```

`client_ref` is 16 random bytes per pack. Repeating it while the pack is `pending`
and has never been committed returns the same `pack_id` with a fresh URL, so retries
never strand quota. Repeating it after the first commit returns `committed` and no
URL. No PUT URL is ever issued for a committed pack. The `if-none-match` and checksum
headers appear only when the store supports them (`store.if_none_match`,
`store.checksum_sha256`).

### `PUT <url>` (object store, or the server's object route in local-disk mode)

The body is the `OXPK` bytes, with exactly the signed headers. S3 stores answer
200 when the object is durable. The local route answers 200 after fsync and rename,
400 if the size differs, and 409 if the pack is no longer `pending`.

### `POST /v1/caches/{cache}/packs/{pack_id}/commit`

Request `OXCM`, response `OXCA`. The pack must be uploaded by the caller and in the
namespace named by the frame. It must be `pending`, or `live` and still
`committable`; otherwise the server answers 409 `conflict` with `pack_closed`. It must
also be present at exactly the declared size, or 409 with `pack_incomplete`. A retry
that sees a different object ETag than the first commit recorded gets 409 with
`pack_changed`, and the server queues a scrub of the pack. Per-entry statuses:

| Status | Meaning |
| --- | --- |
| 0 `committed` | Inserted, or already present naming the same segment (a retry). |
| 1 `exists` | Another write won; this segment is dead. |
| 2 `invalid` | Malformed record header, wrong epoch, segment index out of range, or the segment id already recorded elsewhere in the cache (a different pack, offset or length). |
| 3 `missing_dictionary` | The dictionary segment is not `live`. |

Retrying a commit is safe and returns the same statuses.

### `POST /v1/caches/{cache}/urls`

`{"packs": ["a1b2…"]}` returns `{"urls": {"a1b2…": {"url": "…", "expires_at": …}}}`,
for refreshing expired GET URLs. Packs the caller cannot read are omitted.

### `POST /v1/caches/{cache}/reports`

`{"pack_id": "…", "segment_id": "…", "kind": "hash_mismatch"}` returns 202. The server
verifies the report itself before acting (architecture.md, "Report").

### `POST /v1/caches/{cache}/purge`

Needs `cache:admin`. `{"below_epoch": 4}` or `{"namespace": "personal", "user": "me"}`
returns 202 `{"entries_marked": 10342}`.

### Local-disk object route

`PUT|GET /v1/caches/{cache}/objects/{pack_id}?exp=<unix>&sig=<base64url>`, where
`sig = HMAC-SHA256(url_key, method ‖ path ‖ exp ‖ content-length-or-empty)`. GET
honours a single `Range: bytes=a-b` (206) and answers 416 outside the object.

## Tenant administration (JSON, scope `tenant:admin`)

| Route | Purpose |
| --- | --- |
| `GET, POST /v1/tenant/caches`; `DELETE /v1/tenant/caches/{cache}` | Create (display name, retention days, quota, at least one owner public key) or delete a cache. Deletion is asynchronous: entries go at once, bytes after the grace. |
| `GET, PUT /v1/tenant/caches/{cache}/owner-keys` | The owner public keys the server uses to check roster signatures. |
| `GET, POST /v1/tenant/tokens`; `DELETE /v1/tenant/tokens/{id}` | The secret is returned once on creation. |
| `GET, POST /v1/tenant/users`; `DELETE /v1/tenant/users/{id}` | Human principals, keyed by issuer and subject. Deleting a user revokes their sessions. |
| `GET, PUT /v1/tenant/grants` | Principal × cache × scopes. |
| `GET, POST, DELETE /v1/tenant/oidc/issuers`, `…/oidc/policies` | Issuer allowlist and workload trust policies (exact claim conditions, scopes). A new issuer must pass the SSRF guard and, on the hosted service, the operator allowlist (security.md). For a shared issuer, a policy without an immutable id claim is refused with 400. A workload policy may grant only `read`, `write:personal` and `write:trusted`. |
| `GET /v1/tenant/usage?month=2026-10` | The tenant's own usage. |

Creates use natural keys (name, issuer URL): re-posting identical content returns
200, and different content returns 409.

## Operator administration

A separate listener (default `127.0.0.1:9091`) with operator tokens:

| Route | Purpose |
| --- | --- |
| `GET, POST /admin/v1/tenants`; `PATCH, DELETE /admin/v1/tenants/{t}` | Create, suspend or resume, delete. |
| `PUT /admin/v1/tenants/{t}/quota` | Live-byte quotas (soft and hard), monthly egress, rate. |
| `GET /admin/v1/usage?month=2026-10&format=csv` | One row per tenant, cache and metric: `month,tenant,cache,metric,value`. |
| `POST /admin/v1/caches/{c}/scrub` | Re-verify every pack of a cache now. |
| `GET /admin/v1/health` | Store, metadata and GC leader status. |

## Byte formats

All magics are 4 ASCII bytes followed by `u8 version = 1` and `u8 flags = 0`.

### `OXLQ` lookup request

| Field | Type |
| --- | --- |
| magic, version, flags | `OXLQ`, 1, bit0 records, bit1 urls |
| namespaces | `u8`: bit0 trusted, bit1 own personal |
| reserved | `u8` = 0 |
| count | `u32` ≤ 10,000 |
| lookup ids | `bytes32 × count` |

### `OXLR` lookup response

| Field | Type |
| --- | --- |
| magic, version, flags | `OXLR`, 1, echo of request flags |
| reserved | `u16` = 0 |
| pack_count | `u32` |
| packs | `pack_count × (bytes16 pack_id, str16 url, u64 expires_at)`; url empty without bit1 |
| hit_count | `u32` |
| hits | `hit_count × (u32 index, u8 ns)`, plus, with bit0, `(u32 pack_idx, u64 offset, u32 length, u16 record_len, record)` |

`index` refers to the request order. One id may hit in both namespaces. The client
tries `trusted` first, then `personal`.

### `OXCM` commit request

| Field | Type |
| --- | --- |
| magic, version, flags | `OXCM`, 1, 0 |
| ns | `u8`: 0 trusted, 1 personal |
| reserved | `u8` = 0 |
| pack_size | `u64` |
| seg_count | `u32` ≤ 2,000 |
| segments | `seg_count × (bytes32 segment_id, u64 offset, u32 length)` |
| entry_count | `u32` ≤ 2,000 |
| entries | `entry_count × (bytes32 lookup_id, u32 segment_idx, bytes32 dict_id or zero, u32 epoch, u16 record_len, record)` |

### `OXCA` commit response

`OXCA`, 1, 0, `u16` reserved, `u32 count`, then `count × u8` status.

### `OXPK` pack object

```
header   "OXPK" | u8 1 | u8 0 | u16 0 | bytes16 cache_id                      24 bytes
body     segment ‖ segment ‖ …                                              (each an OXSG)
footer   n × (bytes32 segment_id, u64 offset, u32 length)
trailer  u32 n | u64 footer_offset | bytes32 BLAKE3(footer) | "OXPF"         48 bytes
```

Offsets are absolute in the object. The server reads the footer to compact and
scrub, and checks it against the segment table committed for the pack.

### `OXSG` sealed segment

```
header   "OXSG" | u8 1 | u8 alg = 1 | u8 chunk_log2 = 16 | u8 0 | bytes32 salt | bytes32 K_commit   72 bytes
body     chunk_0 ‖ … ‖ chunk_last          chunk = AES-256-GCM(K_enc, nonce_i, AD, ≤ 64 KiB) ‖ tag16
         nonce_i = u88 i ‖ u8 (1 if last else 0);  AD = BLAKE3(header)
```

`alg = 1` means AES-256-GCM, BLAKE3 key derivation, 64 KiB STREAM. Plaintext:

```
"OXSP" | u8 1 | u8 codec (0 none, 1 zstd, 2 zstd with dictionary) | u64 compressed_len | compressed | zero padding to Padmé(length)
```

zstd frames are written with the content checksum flag set, and readers require it.

The decompressed stream is the concatenation of the files, in record order. Segment
id = `BLAKE3(header ‖ body)`.

### `OXER` sealed record

```
"OXER" | u8 1 | u8 alg = 1 | u16 0 | bytes32 salt | bytes32 K_commit | AES-256-GCM(K_enc, zero nonce, AD, plaintext) ‖ tag16
AD = "oxabl-cache v1 record" ‖ cache_id ‖ u32 epoch ‖ u8 ns_kind ‖ lookup_id
```

Plaintext `OXEP`:

| Field | Type |
| --- | --- |
| magic, version, flags | `OXEP`, 1, 0 |
| segment_id, segment_len | `bytes32`, `u32` |
| dict_id | `bytes32`, zero if none |
| plain_len | `u64` (sum of file sizes) |
| created_at, roster_version | `i64` unix seconds, `u64` |
| signer | `bytes32` Ed25519 public key |
| file_count | `u16` ≤ 16 |
| files | `file_count × (str16 path, u64 size)`; paths are unique within the set |
| signature | `bytes64`, Ed25519 over `"oxabl-cache v1 entry" ‖ cache_id ‖ u32 epoch ‖ u8 ns_kind ‖ lookup_id ‖ every preceding OXEP byte` |
| padding | zero bytes so the sealed record length is a multiple of 256; every byte must be zero |

### `OXRS` roster

| Field | Type |
| --- | --- |
| magic, version, flags | `OXRS`, 1, 0 |
| cache_id, roster_version, epoch | `bytes16`, `u64`, `u32` |
| member_count | `u16` ≤ 1,000 |
| members | `(str16 label, u8 role [0 owner, 1 ci, 2 developer, 3 reader], u8 n, n × bytes32 sign_key, u8 m, m × str16 age_recipient)` |
| keyring | `u32 len` and an age file (binary format) encrypting `CRK ‖ cache_id ‖ u32 epoch` to every recipient |
| sig_count | `u8`, 1 to 4 |
| signatures | `sig_count × (bytes32 owner key, bytes64 Ed25519)`, each over `"oxabl-cache v1 roster" ‖ every byte before sig_count` |

The server sees member labels and public keys. Labels are customer-chosen and need
not be names.

### `OXDP` dictionary pointer

`OXDP`, 1, 0, `bytes16 cache_id`, `u32 epoch`, `u64 version`, `bytes32 dict_segment_id`,
`bytes32 signer`, `bytes64` Ed25519 over `"oxabl-cache v1 dictionary" ‖ every
preceding byte`. Only `ci` or `owner` signers are accepted. The dictionary segment's
plaintext is the raw zstd dictionary with codec 0.

### Identity strings

A member identity is Bech32 with HRP `oxabl-cache-identity-` and payload
`u8 1 ‖ x25519_secret ‖ ed25519_seed`. A member public descriptor (for
`member add`) uses HRP `oxabl-cache-member-` and payload
`u8 1 ‖ ed25519_public ‖ x25519_public`.
