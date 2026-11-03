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
- "base32" everywhere in this document is Crockford's alphabet
  (`0123456789abcdefghjkmnpqrstvwxyz`), lowercase, no padding. 16 bytes are read as
  one 128-bit big-endian integer and written as 26 characters, the first of which is
  `0` to `7`. Decoders accept only the lowercase canonical form, so every id has
  exactly one spelling.

### Error envelope

Every non-2xx response, binary endpoints included, is JSON:

```json
{"error": {"code": "quota_exceeded", "message": "cache over hard quota", "retryable": false, "request_id": "01jc4z5a9r0d6e7f8g9h0k1m2n"}}
```

A 409 also carries `error.reason`, which names the conflict: `pack_closed`,
`pack_changed`, `pack_incomplete` or `exists`. Clients act on `reason`, never on
`message`.

| Status | `code` | Retry |
| --- | --- | --- |
| 400 | `bad_request`, `unsupported_version`. Also returned for a missing `If-Match` where one is required (roster and dictionary PUT), and for a binary endpoint called without `content-type: application/vnd.oxabl-cache.v1` | no |
| 401 | `unauthenticated` (missing, expired or revoked credential) | after re-authenticating |
| 403 | `forbidden` (scope; also a bad or expired object-URL signature, with one generic message), `tenant_suspended` | no |
| 404 | `not_found` (also for another tenant's cache) | no |
| 409 | `conflict` (natural-key clash, or a pack conflict named in `reason`) | no |
| 412 | `precondition_failed` (`If-Match` on roster or dictionary) | after re-reading |
| 413 | `too_large` | no |
| 416 | `bad_request` (a `Range` outside the object, an end past the last byte, or several ranges) | no |
| 429 | `rate_limited`, with `Retry-After` | yes |
| 501 | `not_configured`: a feature this deployment has not configured, such as `POST /v1/auth/oidc` with no identity verifier, or an OIDC issuer or policy route | no |
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
| Rate per tenant (default) | 50 requests/s sustained, burst 200, on `/v1` API routes. Object routes are excluded; their signed URLs are the limit. Unauthenticated requests are outside it (no tenant is known yet); a front proxy or the platform limits them. |

## Authentication

`POST /v1/auth/oidc` (no bearer) with `{"tenant": "t_…", "id_token": "<JWT>"}`.
The client takes `tenant` from `.oxabl/cache.toml`. The token's `aud` must be exactly
`oxabl-cache:<tenant>`; this is not configurable, and no endpoint ever suggests an
audience to the client. The call returns 200 `{"token": "oxs_…", "expires_at": 1793000000, "grants": [{"cache": "c_…", "scopes": ["read", "write:trusted"]}]}`.
The exchange checks the token as follows:
- algorithm RS256 (2,048 to 8,192 bits), ES256 (P-256) or EdDSA (Ed25519); `none`,
  HMAC and every other `alg`, and any `crit` header, are refused at parse time;
- the token is at most 16 KiB;
- the key comes from the issuer's JWKS, with `use` equal to `sig` and a matching
  `alg` if present. A token without `kid` is tried only when exactly one key fits;
- `iss` exactly equals an issuer registered for the tenant (and is in the operator
  allowlist when one is set);
- `aud` is `oxabl-cache:<tenant>`, or a one-element array of it;
- `exp`, `iat` and `sub` are required and `nbf` is optional, all within 60 s of skew.

Then:
- **Workload session (1 h).** The token's scalar claims (strings, whole numbers,
  booleans) are matched against the tenant's workload policies; every condition of a
  policy must hold. Several matching policies give the union of their grants, and
  the oldest one is the session's principal.
- **Human session (12 h).** Otherwise, a registered, enabled user with that issuer
  and subject gets a session with their standing grants.
- **401.** Anything else, including an unknown tenant or one with no issuers. Every
  token failure gets the same 401 and the same message. A suspended tenant gets 403
  `tenant_suspended`.

Key sets are found through `<issuer>/.well-known/openid-configuration`, whose
`issuer` must equal the registered string, and then its `jwks_uri`. They are cached
per issuer URL for at most 1 h and shared by tenants, with one fetch at a time per
issuer. An unknown `kid` forces one refetch, unless the set was fetched less than
1 s ago. That cooldown means a stream of bad tokens cannot drive a fetch per
request, while a key rotation is still picked up within a second. A failed refetch
after the hour leaves no keys (fail closed). A failed forced refetch keeps the
still-valid set, and the token is refused.

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
will free: dead segment bytes inside live packs, plus every byte of packs in grace.
It excludes the framing of live packs (header, footer, trailer), which is never
freed, so `stored_bytes = live_bytes + pending_bytes + reclaimable_bytes + framing`.

### `GET|PUT /v1/caches/{cache}/roster`

GET returns the `OXRS` bytes with `ETag: "v<version>"`. PUT needs `cache:admin` and
`If-Match: "v<current>"`. A new cache has roster version 0 and epoch 0, meaning
"no roster yet". GET then answers 404, and the first PUT carries `If-Match: "v0"`.
The body's version must be `current + 1`. Its epoch must be `current` or
`current + 1`, except for the first roster, which must be version 1 with epoch 1. At
least one of its signatures must verify under an owner key registered for the cache. It answers 204 on success, 412 on a stale
`If-Match`, and 400 on a malformed, unsigned or out-of-range body. Clients still
verify the signature against their own pinned keys.

### `GET|PUT /v1/caches/{cache}/dictionary`

The `OXDP` pointer. PUT needs `write:trusted` and `If-Match: "v<current>"`, where
version 0 means no pointer yet (GET answers 404). The pointer's rules:
- its version must be `current + 1`;
- its epoch must equal the cache's current epoch, which is the current roster's
  epoch (clients check the same);
- its signer must be a `ci` or `owner` key of the current roster, so a pointer needs
  a roster first;
- the named segment must be a live (`dead_since IS NULL`) `dictionary` segment of
  this cache.

The server removes the pointer row when its dictionary is condemned by a confirmed
corruption (architecture.md, "Report"). The next pointer then starts again at
version 1. The pointer counts as a reference to its segment; moving it releases the
previous dictionary to the usual mark rules (architecture.md, "Mark").

Publishing has two steps:
1. Upload and commit a `trusted` pack whose segment table lists the dictionary
   segment with kind 1, and no entries. The commit credits it as live although no
   entry names it.
2. `PUT` the pointer naming that segment.

A dictionary segment that no pointer and no entry has referenced within the grace
is marked dead.

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

When the cache info reports `store.checksum_sha256: true`, `sha256` is **required**
for every pack, and the server signs it into the PUT URL, so the store refuses
different bytes. Today the server still accepts a reservation without it and then
signs no checksum header; the next server change refuses such a reservation with
400. When the capability is false, `sha256` is ignored. The response, in request
order:

```json
{"packs": [
  {"status": "ok", "pack_id": "a1b2…", "url": "https://…", "method": "PUT",
   "headers": {"content-length": "13107200", "if-none-match": "*", "x-amz-checksum-sha256": "…"},
   "expires_at": 1793000900},
  {"status": "committed", "pack_id": "c3d4…"},
  {"status": "quota_exceeded"}
]}
```

`client_ref` is 16 random bytes per pack, unique per `(cache, uploader, client_ref)`.
If a repeat finds its pack `deleting`, or still `pending` with an expired lease, the
whole request gets 409 `conflict` with `pack_closed`; the client starts again with
a fresh `client_ref`. Repeating it while the pack is `pending`
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

Request `OXCM`, response `OXCA`. Needs `write:trusted` or `write:personal` matching
the frame's namespace, and the caller must be the principal that uploaded the pack.
Both are checked first, before any pack lookup or store request, including for a
repeat; a failure gets 403. Each pack is committed exactly once. The pack must be in
the namespace named by the frame and be `pending`, with one exception: a repeat of
its commit. That pack must be present at exactly the
declared size, or the server answers 409 `conflict` with `pack_incomplete`.

A repeat of the commit of an already-committed pack (keyed by pack id) is handled
first:
- if the object's ETag differs from the one recorded, the server answers 409
  `pack_changed` and queues a scrub;
- if the request body's BLAKE3 equals the recorded digest, it returns the recorded
  `OXCA` bytes unchanged;
- otherwise it answers 409 `pack_closed`.

Retries therefore see exactly the first answer, even after the pack's segments have
died, the pack has closed, or GC has run. Per-entry statuses:

| Status | Meaning |
| --- | --- |
| 0 `committed` | Inserted. |
| 1 `exists` | Another write won; this segment is dead. |
| 2 `invalid` | Malformed record header, wrong epoch, segment index out of range, an entry naming a `dictionary` segment as its own, or a segment id already recorded in the cache. |
| 3 `missing_dictionary` | `dict_id` names no live `dictionary` segment of this cache. |

Retrying a commit is safe and returns byte-identical statuses.

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
`sig = HMAC-SHA256(url_key, method "\n" path "\n" exp "\n" content-length)`. The
four fields are joined with a newline byte (0x0A): `method` in upper case, `path`
without the query, `exp` and the content length in decimal ASCII. For GET the
content length is the empty string, so the message ends with `\n`. The separators
keep the digits of `exp` and the length apart. A bad signature or an expired URL
gets 403 `forbidden`. GET honours a single `Range: bytes=a-b` (206); a range outside
the object, an end past the last byte, or several ranges get 416 with code
`bad_request`. A PUT must carry the signed `Content-Length`, equal to the reserved
size.

## Tenant administration (JSON, scope `tenant:admin`)

| Route | Purpose |
| --- | --- |
| `GET, POST /v1/tenant/caches`; `DELETE /v1/tenant/caches/{cache}` | Create (display name, retention days, quota, at least one owner public key) or delete a cache. Deletion is asynchronous: entries go at once, bytes after the grace. |
| `GET, PUT /v1/tenant/caches/{cache}/owner-keys` | The owner public keys the server uses to check roster signatures. |
| `GET, POST /v1/tenant/tokens`; `DELETE /v1/tenant/tokens/{id}` | The secret is returned once on creation. |
| `GET, POST /v1/tenant/users`; `DELETE /v1/tenant/users/{id}` | Human principals, keyed by issuer and subject. Deleting a user revokes their sessions. |
| `GET, PUT /v1/tenant/grants` | Principal × cache × scopes. |
| `GET, POST, DELETE /v1/tenant/oidc/issuers` | The tenant's issuers. POST `{"issuer": "https://…", "shape": "github" \| "gitlab" \| "custom", "id_claims": ["…"]}` (`shape` defaults to `custom`; `id_claims` is required for `custom`). 201 when new, 200 when identical, 409 for the same issuer with another shape or claims, 400 when the SSRF guard or the operator allowlist refuses it (one generic message). DELETE `{"issuer": "…"}` returns 204 and revokes every session opened through its policies and users. |
| `GET, POST, DELETE /v1/tenant/oidc/policies` | Workload trust policies. POST `{"name": "…", "issuer": "…", "claims": {"repository_id": 123456789, "ref": "refs/heads/main"}, "grants": [{"cache": "c_…", "scopes": ["read", "write:trusted"]}]}`. Claim values are a string, an integer or a boolean, compared as text, and stored only as keyed hashes. 201, 200 for an identical policy, 409 for the same name with other content. 400 when no immutable id claim is matched, when an admin scope is requested (only `read`, `write:personal`, `write:trusted`), or when the issuer is not registered. 501 `not_configured` without `[keys] claim_hash_key_file`. GET lists names, issuers, claim names and grants, never values. DELETE `{"name": "…"}` returns 204 and revokes the policy's sessions. |
| `GET /v1/tenant/usage?month=2026-10` | The tenant's own usage. |

Creates use natural keys (name, issuer URL): re-posting identical content returns
200, and different content returns 409.

Immutable id claims by issuer shape. A policy must match at least one of these
exactly:

| Shape | Immutable id |
| --- | --- |
| `github` | `repository_id`, or `repository_owner_id` together with `repository` |
| `gitlab` | `project_id`, or `namespace_id` together with `project_path` |
| `custom` | one of the claims declared in `id_claims` at registration (at least one) |

## Operator administration

A separate listener (default `127.0.0.1:9091`) with operator tokens:

| Route | Purpose |
| --- | --- |
| `GET, POST /admin/v1/tenants`; `PATCH, DELETE /admin/v1/tenants/{t}` | Create, suspend or resume, delete. |
| `PUT /admin/v1/tenants/{t}/quota` | Live-byte quotas (soft and hard), monthly egress, rate. |
| `POST /admin/v1/tenants/{t}/admins` | `{"issuer": "https://…", "subject": "…"}` makes that OIDC subject the tenant's administrator, with a tenant-wide `tenant:admin` grant. 201 `{"id": …}` when new, 200 with the same id when it already is, 409 if that subject exists with another role. An issuer not yet registered for the tenant is vetted (SSRF guard, operator allowlist; 400 if refused) and registered as `custom` with `sub` as its id claim. Operator tokens only; a tenant token gets 401. |
| `POST /admin/v1/tenants/{t}/tokens` | Create a `tenant:admin` token for the tenant; the secret is returned once (`oxabl-cache-admin tenant bootstrap-admin` and `token create --tenant`). |
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
| packs | `pack_count × (bytes16 pack_id, str16 url, u64 expires_at)`. In plan mode (no bit0) `pack_count` is 0. In fetch mode without bit1, each url is empty and `expires_at` is 0 |
| hit_count | `u32` |
| hits | `hit_count × (u32 index, u8 ns)`, plus, with bit0, `(u32 pack_idx, u64 offset, u32 length, u16 record_len, record)` |
| dict_count | `u32`; 0 without bit0 |
| dictionaries | `dict_count × (bytes32 dict_id, u32 pack_idx, u64 offset, u32 length)` |

`index` refers to the request order. One id may hit in both namespaces. The client
tries `trusted` first, then `personal`.

In fetch mode (bit0), `dictionaries` lists every distinct live dictionary segment
named by a returned entry's `dict_id`, once each. Each dictionary's pack is in
`packs`, with its URL under bit1, exactly like an entry's pack. The server reads
`dict_id` from the `entries` row, which is plaintext metadata. The client trusts
nothing about the location: it verifies the dictionary segment's BLAKE3 against the
signed `dict_id` in the record it opened. A dictionary segment is readable by every
principal with `read` on the cache, whatever the namespace of the entry that names
it. Mark keeps a dictionary live while any entry names it. If one is ever absent
all the same, the entries that need it are misses.

### `OXCM` commit request

| Field | Type |
| --- | --- |
| magic, version, flags | `OXCM`, 1, 0 |
| ns | `u8`: 0 trusted, 1 personal |
| reserved | `u8` = 0 |
| pack_size | `u64` |
| seg_count | `u32` ≤ 2,000 |
| segments | `seg_count × (bytes32 segment_id, u64 offset, u32 length, u8 kind)`, with kind 0 `artifact` or 1 `dictionary`; kind 1 is allowed only when `ns` is 0 |
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

Clients also refuse a roster that breaks any of these rules:
- at least one member;
- at least one signing key and at least one age recipient per member;
- labels unique, signing keys unique and recipients unique, across the whole
  roster;
- every recipient an X25519 age recipient (`age1…`).

Among the signatures, those by unpinned keys and pinned ones that fail to verify are
ignored. At least one valid signature by a pinned owner key is required.

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

The identity string is emitted in upper case (`OXABL-CACHE-IDENTITY-1…`), and the
member descriptor in lower case (`oxabl-cache-member-1…`). Parsers accept either in
upper or lower case (Bech32 forbids mixed case). The `x25519_public` field is a raw
32-byte key, so a descriptor of this version names an X25519 recipient only.
