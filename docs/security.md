# Security

Customers treat their ABL source as their most valuable IP. r-code embeds source
paths and string literals; XREF output describes program structure; a debug list
holds the full source text. The cache stores all of these, so it is designed so that
the operator, the server, the storage provider and anyone who steals their data
learn nothing beyond sizes and timing. The client also does not trust the server for
integrity. ADRs: [0004](adr/0004-crypto-scheme.md), [0005](adr/0005-trust-tiers.md).

## Actors and trust

| Actor | Trusted for |
| --- | --- |
| Cache owner (customer) | Everything: holds the owner signing key, manages the roster. |
| CI producer (protected-branch jobs) | Writing the trusted namespace: holds a `ci` signing key. |
| Developer | Reading trusted entries; writing their own personal namespace. |
| Reader (PR jobs, read-only tools) | Reading only. |
| Server, operator, storage provider | **Availability only.** Neither confidentiality nor integrity. |
| Network attacker, other tenants, departed members | Nothing. |

The root of trust is the customer's repository. `.oxabl/cache.toml` is committed
there and pins the cache's owner public keys and minimum roster version. Whoever can
change that file can already change what the build compiles, so no weaker root
exists.

## What the server and storage see

| Item | Visible to server and storage | Form |
| --- | --- | --- |
| Source, paths, file names, compile inputs, branch names | **No** | never sent; paths exist only inside sealed records |
| r-code, XREF, debug lists | **No** | AES-256-GCM ciphertext inside segments |
| Compile key | **No** | only the keyed lookup id, a PRF output |
| Lookup id | yes | 32 pseudorandom bytes, stable for the life of an epoch |
| Entry record | size only | sealed, padded to a 256-byte multiple |
| Segment | id, padded length, pack location | id = BLAKE3 of ciphertext; length Padmé-padded |
| Pack grouping | yes | shows which entries one upload produced together |
| Dictionary | that one exists, its id and size | sealed segment |
| Roster | version, member labels, roles, public keys | signed; the keyring inside is age-encrypted |
| Epoch number | yes | integer |
| Tenant, cache display name, user emails, token names | yes | operator-visible account data; customers choose cache names |
| OIDC claims (repository, ref) | transiently, in memory, during exchange | never persisted or logged; trust policies store keyed hashes of the expected values |
| Times, counts, access pattern | yes | when entries are written and read, how many, by whom |

**A full leak of buckets plus database** gives: ciphertext, entry counts per cache
(about the number of compile units), Padmé size classes of each artifact set, which
entries were uploaded together, access days, and account data. It gives no source,
paths, names or r-code, and no way to test a guess about a file, because lookup ids
need the customer's key.

**The sample route.** `GET /v1/caches/{cache}/sample` gives any `read` token up to
1,000 live trusted lookup ids per call, so a token alone, without a key, can enumerate
the ids of a cache and download the ciphertext behind them (tenant egress quota and
request rate apply). That is the same ciphertext a bucket plus database leak gives and
no plaintext. The ids are not secret from the server, never logged, and the client
verifies every record it fetches by them, so a lying server cannot make `verify` pass.

**Size padding.** Each segment's plaintext is padded to its Padmé length, which
leaks O(log log L) bits of the size. On 100,000 synthetic sizes, log-uniform between
4 KB and 1.4 MB, Padmé cost **1.2%** extra bytes and reduced 78,408 distinct sizes to
204 classes. Rounding up to a power of two would cost 47% and is rejected, because
bandwidth is the product's cost driver. Hiding the entry count with dummy entries is
also rejected: it would cost real storage and bandwidth to hide a number (the count
of compile units) that is no secret.

## Key hierarchy

```
CRK_e  cache root key, 32 random bytes, one per epoch e (only members hold it)
 ├─ lookup_key  = BLAKE3.derive_key("oxabl-cache v1 lookup",  CRK_e)
 ├─ segment_key = BLAKE3.derive_key("oxabl-cache v1 segment", CRK_e)
 └─ record_key  = BLAKE3.derive_key("oxabl-cache v1 record",  CRK_e)

per object, with salt = 32 random bytes from the OS CSPRNG:
   K_enc    = BLAKE3.derive_key("oxabl-cache v1 enc",    sub_key ‖ salt)
   K_commit = BLAKE3.derive_key("oxabl-cache v1 commit", sub_key ‖ salt)
```

**Member identities.** Each member has one identity: an X25519 secret (an age
identity) to receive the root key, and an Ed25519 secret to sign entries. The public
halves are listed in the roster. An identity is a single Bech32 string
(`OXABL-CACHE-IDENTITY-1…`).

**Roster.** A versioned document listing members (`name`, `role` of `owner`, `ci`,
`developer` or `reader`, signing keys, age recipients), the current epoch, the cache
id, and the keyring: `CRK_e` encrypted with age to every member's recipient. It
carries one or more owner signatures. A client ignores every signature by an
unpinned key, and every pinned signature that fails to verify. It accepts the roster
only if at least one signature by a pinned owner key verifies.

Beyond the wire format, a valid roster has:
- at least one member;
- at least one signing key and one age recipient per member;
- unique labels, unique signing keys and unique recipients across members;
- X25519 recipients only.

The member descriptor carries 32 raw bytes of X25519 key, so age plugin recipients
cannot appear in a roster of this format version.

The server stores the roster and accepts a new version only as `current + 1`, from
`cache:admin`, with an epoch of at most `current + 1`. It also requires a valid
signature by one of the cache's owner public keys, which the tenant admin registers
when creating the cache. These checks need public keys only, so a stolen
`cache:admin` token cannot brick the cache with an unsigned roster or a huge epoch.
The server can neither read the keyring nor forge a roster.

**Distribution.**

- CI: the identity is a CI secret (`OXABL_CACHE_IDENTITY`). A `ci` identity is only
  exposed to protected-branch jobs, through environment-scoped secrets. Pull-request
  jobs get a `reader` identity or none. Fork pull requests receive no secrets on the
  supported CI systems and run without the cache.
- Developer: `oxabl-cache identity new` writes the identity to
  `~/.config/oxabl-cache/identity`. The file holds only the identity string
  encrypted with age (binary format) to a passphrase (age's scrypt recipient, work
  factor 2^18, about 1 s to unlock once per session). There is no plaintext
  identity file and no loader for one. CI passes the identity string in
  `OXABL_CACHE_IDENTITY` instead. The developer sends the public part to an owner,
  who runs `oxabl-cache member add`.
- An OIDC-bound unwrap is rejected. It needs a service that holds key material and
  releases it for an OIDC token; if the operator runs that service, the operator can
  unwrap, which breaks zero-knowledge. Key release tied to a customer's own KMS or
  hardware token (age plugin recipients) needs a new member-descriptor version that
  can carry plugin recipients; it is not part of format version 1.
- Argon2id is not used: age has no Argon2id stanza, and a standard, audited format
  is worth more than the choice of KDF. Argon2id at m = 256 MiB, t = 3, p = 4 took
  1.08 s on this machine, about the same unlock cost as scrypt at 2^18.

**Rotation and revocation.**

| Event | Action | Cost |
| --- | --- | --- |
| Add a member | Same epoch; roster `v+1` with `CRK_e` re-wrapped to the new recipient set. | none |
| Remove a member (departed developer) | Disable their server principal (tokens and sessions die at once). Start a new epoch: fresh `CRK_{e+1}`, wrapped to the remaining members. Bump `min_roster_version` in the repository. Optionally `purge` epochs below `e+1`. | Cold cache: the next CI full build uploads ~0.9 GB in ~10 s on top of its compile, and each developer does one cold fetch (~9 s). |
| CI key compromise | As removal, plus a new `ci` signing key. Clients reject entries signed by a key that is not in the current roster, whatever their epoch. | as above |
| Planned owner-key change | Add the new key to `owner_keys` on every live branch, and sign rosters with both the old and the new key. Drop the old key once no branch still pins it alone. | none |
| Owner key compromise | Remove the key from `owner_keys` in `.oxabl/cache.toml` (a repository commit) and from the server's owner keys, then sign a new roster without it and rotate. Branches whose committed file pins only the removed key reject every new roster and run without the cache until that file is updated there. This fails closed, and `oxabl-cache status` says why. | as above |

A removed member who kept `CRK_e` can read epoch-`e` ciphertext only by also
stealing the bucket. Purging old epochs closes that window. They never see a later
epoch.

## Lookup ids

`lookup_id = BLAKE3.keyed_hash(lookup_key, compile_key)`, 32 bytes. Because
`lookup_key` never leaves members, the server cannot compute the lookup id of a
guessed source and test for its presence (a confirmation attack). The same id is used
in every namespace. Rotation changes all ids, which is what makes a rotation cold.

## Encryption

- **AEAD: AES-256-GCM** (aws-lc-rs) with single-use keys. Every segment and record
  has its own salt, so `K_enc` is used for one object only. Nonces can then be
  deterministic counters with no reuse risk and no 2^32-message limit. On this machine
  aws-lc-rs AES-256-GCM runs at 5.1 GB/s per core against 1.7 GB/s for
  ChaCha20-Poly1305, and every supported build host (x86-64 with AES-NI, ARMv8 with
  AES) accelerates AES. XChaCha20-Poly1305 with random nonces is rejected as a
  third the speed with no added safety once keys are single-use.
- **Key commitment.** The header carries `K_commit`, and the opener compares it in
  constant time before decrypting. GCM alone is not key-committing; this stops a
  ciphertext crafted to open under two keys (two epochs, two members) from yielding
  two plaintexts.
- **Segments** use a STREAM construction with 64 KiB chunks. The nonce is a 88-bit
  big-endian chunk counter followed by a last-chunk flag byte, and the associated
  data of each chunk is `BLAKE3(segment header)`. Truncation, reordering and header
  swaps fail authentication.
- **Records** are one AEAD message. Their associated data is
  `"oxabl-cache v1 record" ‖ cache_id ‖ epoch ‖ ns_kind ‖ lookup_id`, so a record
  moved to another key, cache, epoch or namespace kind fails to open.
- **No convergent encryption.** r-code is not byte-deterministic, so equal artifacts
  under different keys almost never happen, and the same key is already deduplicated
  by first write wins. Convergent encryption would buy nothing and would reveal
  equality to the server.

## Compression before encryption

The plaintext of a segment is `zstd(level 3 by default, optional per-cache
dictionary)` over the concatenated files, then Padmé padding. Compression-oracle
attacks (CRIME and BREACH) need an attacker who injects chosen plaintext into the
victim's compression context and watches sizes over many trials. The server cannot
inject anything. Members who can write already hold the key. The dictionary is
trained only from the cache's own trusted artifacts and is itself sealed. What
remains is the compressed size, which padding coarsens. Accepted.

## Integrity without trusting the server

The plaintext of each record holds: segment id, segment length, dictionary id,
plaintext length, each file's relative path and size, the producer's signing key, the roster version at signing time, and a creation time. All of it is
signed with Ed25519 over a domain-separated transcript that also covers `cache_id`,
`epoch`, `ns_kind` and `lookup_id`. The signature sits inside the ciphertext.
Records carry no per-file hashes. The signed segment id, the AEAD and the signature
already bind every byte, and the zstd frame checksum (XXH64, nearly free) guards
against decompressor faults.

**Trust tiers.**

- `trusted` namespace: a client accepts a record only if its signer is a key of a
  member whose role is `ci` in the current roster. An `owner` key is not accepted
  there: owners sign rosters, CI signs entries.
- `personal` namespace: a client accepts a record only if its signer is a key of the
  reading member themselves, and only while that member is listed in the current
  roster. A key that is not in the roster accepts nothing.
- The server enforces the same split with scopes: a developer's token cannot write
  `trusted`, and nobody can write another user's `personal`. To poison what CI or
  another developer reads, an attacker must therefore defeat both the server's scope
  check and a signature check, with a `ci` key that developer machines never hold.

**Local copies.** The tiers protect what the client fetches. A copy that a caller
keeps on disk afterwards (oxabl's local artifact cache) is only as trusted as
every process that can write it. For that reason, each `Installed` outcome carries
its `Provenance` (tier, signer, epoch, roster version), and oxabl records it with the
local entry. That recorded origin is informational, not authenticated: a process of
the same user can rewrite it. oxabl uses its local cache only in `developer` mode,
where it trusts every process of the OS user. In `ci` and `read-only` mode it uses no
local cache, except a per-job directory that oxabl empties first.

**Malicious pull requests** run code inside CI. They never hold a `ci` key or
`write:trusted`, so they cannot write. A `reader` identity lets them decrypt cached
artifacts, but the job has already checked out the source those artifacts came from,
so nothing new leaks.

**Replay and rollback.**

- A record opens only for its own cache, epoch, namespace kind and lookup id.
  Replaying an older valid record for the same key yields an older valid output of
  the same inputs, which is correct by definition: a compile key maps to "any valid
  output of these inputs".
- A record signed by a key that the current roster no longer lists is rejected.
- A record that names a roster version above the client's current roster is
  refused (`RosterVersionAhead`), because the client cannot yet judge its signer.
  This fails closed. It is a miss, and the client refetches the roster once and
  retries that lookup (client.md, "Fetch").
- Roster rollback: the client requires `roster.version ≥ max(min_roster_version,
  local high-water mark)`. CI runners without state rely on `min_roster_version`.
- A server that withholds a newer roster can keep clients on an old epoch only until
  the repository bumps `min_roster_version`. From then on the client refuses the old
  roster and disables the cache (fail closed).

**What a malicious server can still do:** refuse service, delay, drop entries
(misses), serve older valid entries for the same key, withhold a roster (bounded as
above), and observe the metadata listed earlier. It cannot read, forge, alter, mix
tiers, or make a client install a wrong or partial file.

**Out of scope:** compile-key correctness. oxabl must hash every input that affects
the output. A wrong key yields a correctly signed but semantically wrong hit, and no
cache design can detect that.

## Transport and authentication

- **TLS 1.3 only** (rustls), HSTS on the hosted service. Self-hosted clients may pin
  a CA bundle in config.
- **Tokens:** `oxc_<id>_<secret>` with a 256-bit secret, shown once and stored as
  `SHA-256(secret)` (enough for a high-entropy secret; no password hash needed),
  compared in constant time. Each token carries scopes, a cache list and an expiry
  (default 90 days, maximum 1 year).
- **OIDC workload identity** (GitHub Actions, GitLab CI, and any issuer that
  publishes discovery and JWKS documents): the job exchanges its ID token at
  `POST /v1/auth/oidc` for an opaque session token valid for 1 hour. The server
  checks:
  - the issuer against the tenant's allowlist, and the signature with JWKS fetched
    over TLS (cached at most 1 h, refetched once on an unknown `kid`);
  - the algorithm: RS256, ES256 or EdDSA (never `none` or HMAC);
  - `aud` is exactly `oxabl-cache:<tenant id>`. This is fixed and not configurable.
    The client derives it from the `tenant` in the committed `.oxabl/cache.toml`,
    never from the server, and refuses to send a token with any other audience. A
    malicious server therefore cannot make a client mint a token for another
    service;
  - `exp`, `nbf` and `iat` within 60 s of skew;
  - the claim conditions of a trust policy, exact matches on claims such as
    `repository_id`, `ref` or `environment`, stored as keyed hashes.
- **Every workload policy pins an immutable id**, whatever the issuer. One issuer can
  serve many repositories (github.com, gitlab.com, GitHub Enterprise, GHES, a
  self-hosted GitLab), and names can be re-registered after a rename. Registering an
  issuer names its immutable id claims, and every policy must include an exact match
  on at least one of them:
  - GitHub-shaped tokens: `repository_id`, or `repository_owner_id` plus
    `repository`;
  - GitLab-shaped tokens: `project_id`, or `namespace_id` plus `project_path`;
  - any other issuer: the claims its admin declares at registration (at least one).

  A typical writer policy is `repository_id = 123456789` with `ref =
  refs/heads/main` and `environment = cache-writer`.
- **Workloads never get administration scopes.** A policy may grant `read`,
  `write:personal` or `write:trusted` only. `cache:admin` and `tenant:admin` belong to
  humans and to tokens a human creates.
- **Issuer fetches are SSRF-guarded**, on every discovery and JWKS fetch:
  registration, each periodic refetch and each refetch on an unknown `kid`.
  - URLs must be `https` on port 443, with no credentials in the URL.
  - The server resolves the name itself, through the system resolver or the
    configured `dns_servers` over UDP (truncated answers are refused). It checks
    every returned address, and one forbidden address refuses the whole name.
    Forbidden ranges:
    - IPv4: `0.0.0.0/8`, `10.0.0.0/8`, `100.64.0.0/10`, `127.0.0.0/8`,
      `169.254.0.0/16` (which includes the cloud metadata address),
      `172.16.0.0/12`, `192.0.0.0/24`, `192.0.2.0/24`, `192.88.99.0/24`,
      `192.168.0.0/16`, `198.18.0.0/15`, `198.51.100.0/24`, `203.0.113.0/24`,
      `224.0.0.0/4` and `240.0.0.0/4`;
    - IPv6: `::`, `::1`, `100::/64`, `2001::/23` (Teredo included),
      `2001:db8::/32`, `fc00::/7`, `fe80::/10`, `fec0::/10`, `ff00::/8`,
      `5f00::/16`. Mapped IPv4 addresses are checked as IPv4, and NAT64 or 6to4
      addresses that embed a forbidden IPv4 address are forbidden.

    `[oidc] allowed_private_ranges` (CIDR blocks or single addresses) overrides
    this, for a self-hosted IdP on an internal network.
  - It then connects to the address it checked, with SNI and `Host` set to the issuer
    name (an IP SNI for IP literals). DNS rebinding between the check and the
    connect, or between fetches, therefore cannot redirect it.
  - At most 3 redirects, each re-parsed and re-checked from scratch.
  - Bodies are capped at 256 KiB. One 5 s budget covers the whole fetch, redirects
    included. No `Accept-Encoding` is sent.
  - Every failure shows the tenant one generic message ("the issuer could not be
    verified"), and the exchange route answers the same 401 for every token
    failure.

  On the hosted service an operator allowlist of issuers is also enforced.
- **Gitea Actions** uses a scoped API token held as a CI secret.
- **Humans:** `oxabl-cache login` runs the OAuth device authorization grant
  (RFC 8628) against the tenant's IdP (or the hosted service's IdP) and exchanges the
  ID token at the same endpoint for a 12-hour session.
- **Sessions** are random, stored hashed, revocable at once, and listed per user.
- **Presigned URLs** are bearer capabilities for one pack for 15 minutes; a leaked
  URL yields ciphertext only.
- **Why authentication is separate from encryption:** the server needs identities to
  enforce scopes, quotas and usage, and it must never hold key material. Splitting
  them means a stolen token yields only ciphertext and denial of service, and a
  stolen identity without a token yields nothing unless the bucket also leaks. The
  operator runs billing and abuse controls without keys.
- **Logging:** tokens, JWTs, OIDC claims, signed URL query strings and lookup ids are
  never logged. Logs carry request ids, tenant and cache ids, counts, sizes, statuses
  and durations.

## Performance cost

Measured single-core on this machine (Intel Xeon Platinum 8462Y+, Rust 1.99, release
build, `target-cpu=native`):

| Operation | Rate |
| --- | --- |
| AES-256-GCM, aws-lc-rs | 5,144 MB/s |
| ChaCha20-Poly1305, aws-lc-rs / RustCrypto | 1,689 / 1,109 MB/s |
| AES-256-GCM, RustCrypto | 1,347 MB/s |
| BLAKE3 | 3,842 MB/s |
| SHA-256 (SHA-NI) | 1,325 MB/s |
| Ed25519 sign / verify (`ed25519-dalek` `verify`) | 17.3 µs / 34.4 µs |
| Ed25519 `verify_strict` (used; rejects non-canonical S and small-order keys) | ~50 µs |
| Record seal (sign + AES), measured in `oxabl-cache-crypto` | 23.1 µs |
| Record open (AES + `verify_strict`), measured in `oxabl-cache-crypto` | 55.0 µs |
| Segment seal (zstd-3 + pad + AES + id), synthetic data | 169 MB/s |
| Segment open (id + AES + zstd decompress), synthetic data | 648 MB/s |
| Lookup-id derivation (keyed BLAKE3 of 32 bytes) | 0.08 µs |
| zstd-3 compress / decompress (synthetic data) | 196 / 989 MB/s |

Per scenario (0.93 GB sealed, 4.3 GB plain, 10,000 sets):

| Scenario | Security CPU | Wall-clock effect at 1 Gbit/s, 30 ms RTT |
| --- | --- | --- |
| Cold fetch | Segment ids 0.24 s, AES 0.18 s, signatures (`verify_strict`, record open at 55 µs) 0.55 s: ~1.0 core-s, ~0.13 s on 8 cores, overlapped with ~8 s of transfer | +1.2% bytes from padding (~0.1 s), ~2% in total |
| CI upload | Input check (BLAKE3 of 4.3 GB against the caller's hashes) 1.12 s, AES 0.18 s, ids 0.24 s, signing 0.17 s: ~1.7 core-s, next to 11 to 22 core-s of zstd | < 2% |
| Lookup of 20,000 keys | ids 1.6 ms; record verification (fetch mode only) 20,000 × 55 µs = 1.1 core-s, ~140 ms on 8 cores | ~0 in presence mode |

At 1 Gbit/s the whole security stack costs under 5% of wall-clock time, because
every flow is bound by bandwidth or round trips. On a same-region 10 Gbit/s link the
cold fetch becomes CPU-bound. Security is then ~19% of client CPU (1.0 of ~5.3
core-s; decompression is the rest), about 0.13 s of wall-clock time on 8 cores.
The strict verifier costs ~0.2 core-s more per cold fetch than plain `verify`, and
it is kept.

The zstd decoder's window limit is 128 MiB. A member who holds the key can
therefore make a decoder reserve up to 128 MiB per segment in flight. The client
bounds how many segments are in flight (client.md), and tightening the limit to the
segment's pledged content size is possible later, because the client knows the file
sizes when it compresses.
