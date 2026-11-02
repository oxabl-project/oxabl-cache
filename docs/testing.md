# Testing

There are no unit tests. Every test drives the product as a consumer would: the
`oxcache-server` and `oxabl-cache` binaries, raw HTTP, and the public API of
`oxcache-client`. Tests assert outcomes (bytes on disk, HTTP statuses, usage
figures), never internal state. Fuzzing covers the parsers and the install path.

## Harness

`tests/e2e` is a separate package (`oxcache-e2e`). Its only workspace dependency is
`oxcache-client`, used through its public API. Its frame parser for `OXLR`, `OXCM`
and the other frames is written independently from protocol.md. The hostile proxy
and the storage attacks below use it, so it also checks the spec.

- **Server under test.** The harness builds `oxcache-server` once
  (`cargo build -p oxcache-server --locked`) and starts it per test with a generated
  config: `listen = 127.0.0.1:0`, short timers (grace 2 s, lease 2 s, GC interval
  200 ms, URL lifetime 5 s), and a fresh data directory under
  `target/e2e-tmp/<test>-<pid>/`. It reads the bound address from the server's
  `{"ready": …}` stdout line. The timers are plain configuration; the server has no
  test hooks.
- **Backends.** Every scenario runs twice through a macro: `fs + sqlite` and
  `s3 + postgres`.
- **S3: MinIO**, run as a child process from the pinned nixpkgs binary in the repo's
  devshell (`nix develop`), one per test binary, each test on its own bucket and
  random port. It supports every operation in the portable subset. A nightly job
  repeats the S3 suite against Garage (a second, independent implementation) and
  runs `oxcache-server check-store` against a dedicated R2 test bucket.
- **Postgres:** `initdb` and `postgres -k <dir> -p <port>` from the same devshell,
  one cluster per test binary, one database per test.
- **No Docker.** Every dependency is a child process in its own process group, with
  `PR_SET_PDEATHSIG`, killed on drop. The harness never lists, stops or creates
  containers, so it cannot touch anyone else's.
- **Fixtures.** A generator makes synthetic artifact sets that look like r-code (a
  binary header, a string table, sizes log-uniform from 4 KB to 7 MB). Every file and
  path embeds a unique random canary. No Progress runtime is needed.
- **Driver.** `oxcache-e2e-driver` is a small binary over the public client API. It
  prints one progress line per phase, so the harness can `SIGKILL` it at an exact
  point.
- **Fake IdP.** A test tool that serves JWKS and mints ID tokens shaped like those of
  GitHub, GitLab and Gitea Actions.

## Scenarios

Each test names the behaviour it protects.

| Area | Scenario and assertion |
| --- | --- |
| Round trip | CI stores 500 sets; a developer fetches them. Installed files are byte-identical, `InstalledFile.blake3` matches, and no temp files remain. |
| First write wins | Two CI writers store different bytes for the same 1,000 keys. Per key, one gets `committed` and the other `exists`. Every reader installs exactly one writer's complete set. |
| Tiers | A developer's personal writes are invisible to CI and to other developers. A developer token gets 403 writing `trusted` over raw HTTP. A developer-signed record copied into `trusted` in the database is a miss (`Untrusted`) for CI. |
| Quotas | Writes over the hard quota report `QuotaExceeded` and the build continues. Above the soft quota, eviction brings usage under 80%, personal namespaces first. |
| Retention and GC | An entry idle past retention disappears and its pack is deleted after the grace. A reader holding a URL issued before eviction still downloads within the grace. |
| Compaction | A churn-heavy workload leaves packs under 50% live. After compaction every live entry still installs correctly and stored bytes drop. |
| Rotation | After `member remove`, the removed identity cannot open new-epoch records. Clients pinned to the new `min_roster_version` refuse the old roster. |
| OIDC | A matching policy yields a session with the policy's scopes. A wrong `aud`, an expired token, `alg: none`, an HMAC algorithm, an unknown `kid` or an unmatched claim gets 401. |
| Tokens | A revoked or expired token gets 401 on the next call, mid-session included. |
| Tenancy | Tenant B's token on tenant A's cache gets 404. |
| Usage | `usage export` equals the counts the test drove (bytes, ids, hits, commits). |
| Migration | `fs + sqlite` is migrated to `s3 + postgres`; every entry still installs. A migration with one corrupted pack refuses to finish. |
| Restore | A database snapshot older, then newer, than the bucket gives only misses, and reports clear the stale entries. |

## Adversarial tests

The common assertion: no file at a final path ever differs from what a trusted
producer stored, the outcome is a miss, and no temp file remains.

- **Tampered storage.** Flip one byte of a pack in the segment header, a chunk body, a
  tag, the footer and the trailer, on disk and in MinIO. Fetch misses, the report is
  confirmed, the entry is gone, and a fresh `put` then hits.
- **Swapped records.** Swap the `record` columns of two entries in the database. Both
  keys miss.
- **Replayed records.** Copy a record to another lookup id, another cache, an older
  epoch row, and the other namespace. Every case misses.
- **Truncated upload.** A raw `PUT` with a short body is refused. A commit whose
  declared size differs from the stored size gets 409 and creates no entries.
- **Wrong key.** An identity absent from the roster gets `OpenError`. A client holding
  another cache's root key misses everything.
- **Crash between upload and commit.** `SIGKILL` the driver after its PUTs. No entry
  exists, the pending pack is swept after the lease, and the quota reservation is
  released.
- **Crash during commit.** `SIGKILL` the server during a 2,000-entry commit. After
  restart the batch is entirely present or entirely absent.
- **Concurrent writers.** 32 drivers store different bytes for the same 1,000 keys.
  Every key resolves to exactly one writer's complete set for every reader.
- **Hostile server.** A proxy between client and server (and store) flips bytes,
  swaps hits' records, replays old records, truncates ranges, serves another pack's
  bytes with 200, serves a stale or rolled-back roster, and withholds URLs. There is
  never a wrong install. A roster rollback below the high-water mark or
  `min_roster_version` gives `OpenError`.
- **Malicious producer paths.** A `ci` identity stores records with `..`, absolute
  paths, backslashes, NUL bytes and over-long paths, and `out_root` contains a
  symlink pointing outside it. The filesystem outside `out_root` is byte-identical
  before and after, and every case misses.
- **Decompression bombs.** A signed record with `plain_len` above the limit, and a
  stream that expands past its declared `plain_len`. Both miss with bounded memory
  (the driver runs under a 512 MiB `RLIMIT_AS`).
- **Zero-knowledge canary.** After the full suite, scan the SQLite file and a
  `pg_dump`, every object in both stores, and all server logs for every canary, every
  plaintext path and every compile key: raw, hex, base64 and UTF-16. Nothing may
  match.
- **Log canary.** Tokens, session ids, ID tokens and URL signatures never appear in
  server logs.

## Fuzzing

`cargo-fuzz` targets. Each runs for 60 s per pull request and 30 min nightly. The
corpora live in `fuzz/corpus/`, seeded from frames the e2e suite records.

| Target | Property |
| --- | --- |
| `format_*` (each of `OXLQ`, `OXLR`, `OXCM`, `OXCA`, `OXPK` footer, `OXRS`, `OXDP`) | Never panics; `decode(encode(x)) == x`; output of any successful decode re-encodes byte-identically. |
| `open_record`, `open_segment` (fixed key) | Never panics, bounded allocation, `Ok` only for inputs produced by `seal`. |
| `http_request` | Arbitrary method, path, headers and body into the in-process router on in-memory SQLite and local store: no panic, every non-2xx is a valid error envelope, no 500 for malformed input. |
| `install` | Arbitrary path lists and contents into an install under a scratch root: nothing created outside the root, no temp files left, every final file complete. |

## Load tests

`tests/load` holds `oxcache-load`, built on the public client API, against
`s3 + postgres` on dedicated hardware, weekly and before each release. The 30 ms RTT
comes from `tc netem` inside a network namespace on that runner only.

| Scenario | Pass |
| --- | --- |
| 50 concurrent CI full uploads of 10,000 synthetic sets | zero errors; each ≤ 12.5 s at 1 Gbit/s |
| 500 developers, plan-mode lookups of 20,000 ids, 1 per second each | p99 ≤ 300 ms; database CPU < 70% |
| 200 concurrent cold fetches | per-client time within 25% of the budget (architecture.md); zero wrong installs |

## Build gates

Every pull request runs:

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo deny check`
  (advisories, licences, bans), and `#![forbid(unsafe_code)]` in every crate.
- **Server crypto ban:** `scripts/check-server-deps` fails if
  `cargo tree -p oxcache-server -e normal` contains `oxcache-crypto`,
  `oxcache-client` or `age`. A fixture crate that depends on `oxcache-crypto` must
  make the script fail; that case runs in the same job, so the gate cannot silently
  pass.
- **Public-repo hygiene:** a grep for internal identifiers over the tree, with a
  fixture that must trip it.
- The e2e suite on both backends, and 60 s of each fuzz target.
