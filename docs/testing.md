# Testing

There are no unit tests. Every test drives the product as a consumer would: the
`oxabl-cache-server` and `oxabl-cache` binaries, raw HTTP, and the public API of
`oxabl-cache-client`. Tests assert outcomes (bytes on disk, HTTP statuses, usage
figures), never internal state. Fuzzing covers the parsers and the install path.

## Harness

`tests/e2e` is a separate package (`oxabl-cache-e2e`). Its only workspace dependency is
`oxabl-cache-client`, used through its public API. Its frame parser for `OXLR`, `OXCM`
and the other frames is written independently from protocol.md. The hostile proxy
and the storage attacks below use it, so it also checks the spec.

- **Server under test.** The harness builds `oxabl-cache-server` once
  (`cargo build -p oxabl-cache-server --locked`) and starts it per test with a generated
  config: `listen = 127.0.0.1:0`, a GC interval of 200 ms, timers a loaded machine
  cannot outrun (lease 60 s, URL lifetime 30 s, grace 35 s), and a fresh data
  directory under `target/e2e-tmp/<test>-<pid>/`. It reads the bound address from the
  server's `{"ready": …}` stdout line. The timers are plain configuration; the server
  has no test hooks. A test that waits for an expiry shortens that one timer and
  waits on the event (the object gone, the row gone), never on a sleep. Even
  a shortened timer stays above the seconds a request can stall under load (the
  lease-expiry tests use 12 s).
- **Backends and targets.** Each scenario file is written once and included by
  `#[path]` into one test target per backend; a `BACKEND` constant selects the
  backend.

  | Target | Store | Metadata | Notes |
  | --- | --- | --- | --- |
  | `single_node` | fs | SQLite | |
  | `single_node_postgres` | fs | Postgres | |
  | `s3_garage` | Garage | SQLite | |
  | `s3_garage_postgres` | Garage | Postgres | plus the multi-node scenarios |
  | `s3_versitygw` | versitygw | SQLite | |
  | `oidc`, `oidc_postgres` | fs | SQLite, Postgres | the OIDC suite |

  Each Postgres target has a guard test that fails when a module of its SQLite
  target is missing. `single_node_postgres` also reads `schema_migrations` from the
  database in the server's configuration, so a target that quietly ran on SQLite
  goes red. The core `behaviour` suite picks its database from
  `OXABL_CACHE_TEST_BACKEND` (`sqlite` by default, or `postgres`).
  `oxabl-cache-meta-postgres` has its own port-level tests:
  - a scope matrix of which scopes overlap and which serialise, with a
    deterministic rendezvous so "overlap" cannot pass by luck;
  - nodes migrating at once, and a newer schema refused;
  - TLS required;
  - constraint errors, and refusal of a swallowed failed statement;
  - leader election and failover.
- **Multi-node scenarios** (`s3_garage_postgres`):
  - Two nodes commit the same keys at the same instant. Every pack is won whole by
    exactly one node, and both nodes serve the winner.
  - Exactly one node is the GC leader. When it is `SIGKILL`ed, the other takes over
    and sweeps.
  - After the 10 s refresh, each node admits about half of a tenant's burst.
  - The Postgres cluster is killed mid-commit and restarted. The commit gets 503,
    no partial rows remain, and the retry commits everything without a server
    restart.
  - **GC fencing.** Leader A runs behind the fault proxy with slowed replies and is
    caught inside a GC pass, then `SIGSTOP`ped. Its lock session is terminated, and
    rival B takes over. B is configured never to delete anything in the test window.
    The test waits until the pack's lease has run out, then resumes A. A stops
    leading, and the expired pack A would have swept is still in `packs` and in the
    bucket. A in flight with more steps to run is refused by the fence; A at the end
    of its pass gives up at its next election poll. Both leave the pack untouched.
    The session termination is limited to the test's own database, because the
    Postgres cluster is shared with the other tests of the binary.
  - **Readiness.** `/readyz` answers 503 while Postgres is stopped and 200 after it
    restarts, while `/livez` and `/v1/health` stay up. With the local-disk store,
    removing the store root under a running server makes `/readyz` answer 503 naming
    the store, and recreating it brings 200 back.
  - **Postgres timeouts.** With `statement_timeout = "1s"`, a request blocked on a
    table lock answers 503 well before the lock is released. With
    `idle_in_transaction_timeout = "1s"` and a delayed reply, the session is ended and
    the request answers 503. The server recovers in both cases.
- **S3: Garage** (nixpkgs `garage`, 1.3.1) is the primary store, run as a child
  process from the repo's devshell (`nix develop`). There is one single-node
  instance per test binary, with each test on its own bucket and random ports.
  **versitygw** (nixpkgs `versitygw`, POSIX backend) is the second, independent
  implementation. The full S3 suite runs against both on every pull request, so the
  portable subset never drifts toward one server's behaviour. Optional capabilities
  differ per server, and `s3_suite/capabilities.rs` pins them, so an upgrade that
  changes one fails and says so:

  | Server | `If-None-Match: *` on PUT | `x-amz-checksum-sha256` |
  | --- | --- | --- |
  | Garage 1.3.1 | no (a second PUT overwrites) | yes (a wrong digest is refused) |
  | versitygw (POSIX) | yes (412) | yes |

  A capability's scenarios run where the server has it, and are skipped with a
  logged reason where it does not. On Garage, a stale PUT overwrites a committed
  pack. The commit retry then gets `pack_changed`, a scrub removes the entry, and
  nothing is served. The two servers already caught one drift: versitygw answers
  `DeleteObjects` without the XML namespace, so the adapter parses that answer with
  its own namespace-tolerant parser. That parser still fails closed on anything that
  is not a `DeleteResult`. MinIO is not used: nixpkgs marks it
  insecure. The real R2 bucket is exercised by the prototype, outside this suite.
- **Postgres 18** (nixpkgs `postgresql`): `initdb` and `postgres -k <dir> -p <port>`
  from the same devshell, one cluster per test binary, one database per test.
- **Fault proxy.** A small TCP proxy that the harness puts in front of Postgres or
  the store can delay, cut or drop a chosen reply. It fakes the network, never the
  program under test.
- **Fake clock.** Day-boundary tests run the server under `libfaketime` (from the
  devshell) with an accelerated clock. Rate-limit tests run it under a **frozen**
  clock set to the present instant, so the token bucket cannot refill during the
  test and S3 signatures stay inside the store's skew window. This fakes the
  environment's time; the server has no clock hook.
- **No Docker.** Every dependency is a child process in its own process group, with
  `PR_SET_PDEATHSIG`, killed on drop. The drop returns only once the whole group has
  exited, so a restart on the same data directory cannot meet a dying member. A cluster
  that loses the race for its port to another process starts again on a new one. The
  harness never lists, stops or creates containers, so it cannot touch anyone else's.
- **Fixtures.** A generator makes synthetic artifact sets that look like r-code (a
  binary header, a string table, sizes log-uniform from 4 KB to 7 MB). Every file and
  path embeds a unique random canary. No Progress runtime is needed.
- **Terminals.** `session::without_terminal` starts a child in a session with no
  controlling terminal, so a prompt on `/dev/tty` fails instead of waiting for whoever
  runs the tests. `session::Terminal` gives a child a pseudo-terminal of its own as its
  controlling terminal, and the test types at its prompts and reads what the terminal
  showed, echo included.
- **Driver.** `oxabl-cache-e2e-driver` is a small binary over the public client API. It
  prints one progress line per phase, so the harness can `SIGKILL` it at an exact
  point.
- **Fake IdP.** A test tool that serves discovery and JWKS and mints ID tokens
  shaped like those of GitHub Actions and GitLab CI. It also serves a GitHub-style
  token request endpoint that records the audience each client asks for. It can sign
  RS256, ES256 and EdDSA, rotate keys, redirect discovery, and pad or stall the key
  set. It can also serve the device authorization grant (`idp::device`): a device
  endpoint and a token endpoint that answer `authorization_pending` and `slow_down`
  before approving, deny, or mint tokens for a chosen audience and subject. The OIDC tests (target `tests/e2e/tests/oidc`) run the server, the fake IdP
  and a fake DNS server, whose answers change at run time or alternate per query,
  inside an unprivileged user and network namespace. Each test enters its own
  namespace (`netns::enter`). The launcher configures the namespace as root, then
  runs the test as the invoking uid (`unshare --user --map-user`, with
  `ip_unprivileged_port_start=0` so the IdP can bind 443), because Postgres refuses
  to run as root. Ubuntu 24.04 runners restrict unprivileged user namespaces
  through AppArmor, so CI lifts that restriction before the tests. The IdP
  listens at `10.99.0.10:443`, the only allowed range, with a
  test CA the server trusts. **Decoy listeners** sit on every address that must stay
  unreachable: `127.0.0.1`, `::1`, `10.0.0.5`, `172.16.0.1`, `192.168.1.1`,
  `100.64.0.1` and `169.254.169.254`. A refusal is therefore proven by "no
  connection reached the decoy", not by "connect failed".
- **Break it once.** Every scenario and adversarial test below is first run against a
  deliberately broken server or client build (the protecting check removed) and must
  fail, naming its assertion. The break is recorded in the pull request that adds the
  test.

## Scenarios

Each test names the behaviour it protects.

| Area | Scenario and assertion |
| --- | --- |
| Round trip | CI stores 500 sets; a developer fetches them. Installed files are byte-identical, `Provenance` names the `ci` signer and `Tier::Trusted`, and no temp files remain. Repeated with `Durability::PerFile` and `Durability::Caller`. |
| Streaming fetch | Outcomes arrive per key before the whole fetch completes. A store throttled to 1 MB/s keeps the fetch alive past 30 s, because bytes flow. A store that stalls makes every pending key `Timeout` after the idle timeout, and sets already installed stay installed. |
| Output layout | Outputs live in a non-default layout (`rcode/…` for r-code, `reports/xref/…` for XREF), and `ArtifactFileRef.disk_path` points there while `path` stays `r/…` and `xref/…`. CI stores them and a developer fetches 1,000 keys in one `fetch` call: every set installs under `stage/<artifact path>`, byte-identical. A relative `disk_path`, or one whose final component is a symlink, reports `Failed(Unreadable)`. A `disk_path` under a symlinked intermediate directory stores normally. |
| Lazy store | 2,000 sets of 5 MB each are fed through `put` while the driver runs under a 512 MiB `RLIMIT_AS`, with no failure. A file rewritten after `put`, with the same size and different bytes and with a different size, reports `Changed`, and a fetch of that key afterwards is `NotFound` (nothing was uploaded). A file whose `ArtifactFileRef` hash is wrong from the start also reports `Changed`. A missing file reports `Failed(Unreadable)`. Duplicate or invalid paths report `Rejected`. |
| Config trust | `from_repo` fails with `TrustOverride` when any one of `OXABL_CACHE_TENANT`, `OXABL_CACHE_CACHE`, `OXABL_CACHE_OWNER_KEYS` or `OXABL_CACHE_MIN_ROSTER_VERSION` is set (each in its own case), and fails when `cache.toml` has an unknown key. `OXABL_CACHE_SERVER` is honoured. |
| OIDC audience | The client requests a GitHub-style token, and the fake IdP records audience `oxabl-cache:<tenant>`. A hostile proxy then adds an `audience` field to every server response, and answers 401 with a different audience in the message: the next request still asks for `oxabl-cache:<tenant>`. A GitLab-style `OXABL_CACHE_ID_TOKEN` minted for `sts.amazonaws.com` gives `OpenError`, and the proxy sees no request carrying it. The server gets 401 for a valid token with any other audience. |
| First write wins | Two CI writers store different bytes for the same 1,000 keys. Per key, one gets `committed` and the other `exists`. Every reader installs exactly one writer's complete set. |
| Tiers | A developer's personal writes are invisible to CI and to other developers. A developer token gets 403 writing `trusted` over raw HTTP. A developer-signed record copied into `trusted` in the database is a miss (`Untrusted`) for CI. |
| Personal crowding | Hard quota of 4 full sets, GC interval longer than the test. 8 developer identities each try to store a full set. Each one's personal live bytes stop at or below 10% of hard (0.4 full sets), and the rest of each set reports `QuotaExceeded`; personal live bytes total about 3.2 full sets. CI's full upload (1 full set, so 4.2 > 4) then commits completely: its admission evicts personal entries synchronously, oldest first, and every CI entry installs. In a second case, a developer holds a large personal upload `pending` (not yet committed) while CI's upload arrives. CI is admitted over the hard quota by at most those pending bytes, every CI entry installs, and stored bytes stay under the 3x ceiling. |
| Quotas | Writes over the hard quota report `QuotaExceeded` and the build continues. With the grace set longer than the test, a full-rebuild workload pushes live bytes past 90%. Eviction stops at or just under 80% live, trusted entries written last still install, personal namespaces go first, and `reclaimable_bytes` reports the lag. |
| Dictionary lifecycle | CI publishes a dictionary (a pack with one `dictionary` segment and no entries), then moves the pointer. After waiting past the grace, the pack and segment are still live. CI stores 200 sets with the dictionary, and a developer fetches them. CI publishes a second dictionary. The first stays live while any entry names it. With the pointer already on the second dictionary, a developer (`read` only, a different namespace from CI) fetches the 200 entries compressed with the first: each lookup response lists the first dictionary's location, and every set installs byte-identical. After those entries are evicted and the grace passes, the first dictionary's pack is swept, and `quota.live_bytes` dropped by its length when it died. A dictionary committed without a pointer update is marked dead after the grace, and its pack is swept. |
| Live-byte accounting | A personal commit with two entries naming one artifact segment (the same `segment_idx`) gets `invalid` for both, and nothing is credited. Throughout every quota, GC, compaction and dictionary scenario, the harness reads `quota.live_bytes` after each step and compares it with the sum of the lengths of live segments, computed from the database by the attacker-side SQL reader. They are always equal, and never negative. |
| Losing packs | Two CI writers store different bytes for the same 500 keys, one after the other. Every entry of the second pack is `exists`. After the grace, the second pack's object is gone from the store and `stored_bytes` is back to one pack's worth. |
| Retention and GC | An entry idle past retention disappears and its pack is deleted after the grace. An entry that is never read retires on time too (it has an access row from commit). A reader holding a URL issued before eviction still downloads within the grace. |
| Compaction | A churn-heavy workload leaves packs under 50% live. After compaction every live entry still installs correctly and stored bytes drop. |
| Rotation | After `member remove`, the removed identity cannot open new-epoch records. Clients pinned to the new `min_roster_version` refuse the old roster. |
| Member command line, lifecycle | `crates/oxabl-cache-cli/tests/member`. An owner runs `identity new`, the tenant administrator (through `oxabl-cache-admin` and the tenant API) creates the cache with the printed owner key, and `init` publishes roster v1. A `ci` identity (`identity new --secret-out`) and a developer join with `member add`; the client library, as oxabl uses it, stores and fetches with each, and the developer's personal entry reads back as `personal`. `member remove` starts epoch 2: the removed identity gets `IdentityNotInRoster`, its old handle misses every epoch-2 entry, and the command prints the new `min_roster_version`. `rotate` leaves the old epoch readable, `rotate --purge` leaves nothing readable under it (every fetch and probe from a handle opened before the rotation misses), and a fresh handle stores and fetches again. |
| Member command line, owners | `owner add` publishes a roster signed by both owners: a checkout pinned only to the old key and one pinned only to the new key both open it. `owner remove` by key starts a new epoch signed by the remaining owner: a checkout pinned only to the removed key gets a roster error, one pinned to the remaining key opens, and the removed owner can no longer change the roster. A descriptor that does not match the cosigning identity, a missing cosigner, removing the signing owner, an unknown owner and `member remove` of an owner each fail and leave roster version and epoch unchanged. |
| Member command line, dictionary | `dict train --publish` as a `ci` member reports a gain of at least 1.3x on held-out files and moves the pointer to version 1; the next 40 sets upload at under 80% of the bytes the same sets took before, and a developer fetches all of them byte-identical. Noise outputs (gain under 1.3x), a developer's `--publish` and too few files publish nothing (`dictionary_version` stays 0) and exit 1. |
| Member command line, entries | `put` as `ci` commits one key and a repeat reports `existed`; `fetch` installs exactly its files as `trusted`, honours `--expect` (a wrong set is `pathmismatch`), reports `notfound`, and refuses a malformed key, a missing file or a path that escapes (exit 2). A developer's `put` lands in the personal namespace, which `ci` cannot fetch. `verify --sample n --keys` verifies exactly the sample, never more than the keys, and exits 1 with an unknown key or a personal-tier entry in it; its scratch directory is gone afterwards. |
| Member command line, login | Inside a network namespace, with the fake identity provider serving the device grant over TLS at an address the server's SSRF guard allows: `login` prints the page and code, polls through `slow_down` and `authorization_pending`, asks for `scope=openid` and `audience=oxabl-cache:<tenant>`, and stores a mode 0600 session of about 12 hours that `init` and `status` then use. `logout` deletes it and the server answers 401 for its token. An ID token with another audience never reaches the server (the server log shows no exchange) and stores nothing, a denied login and an unknown person store nothing, and each exits 1. |
| Member command line, secrets | No command prints a passphrase, token or identity string, in JSON or text. `OXABL_CACHE_TOKEN`, `OXABL_CACHE_IDENTITY` and `OXABL_CACHE_IDENTITY_PASSPHRASE` each make a command exit 2, naming the flag and not the value. A passphrase, token or identity file with group or other permission bits, and a missing passphrase source without a terminal, exit 2; a wrong passphrase exits 1. On a pseudo-terminal the passphrase is not echoed, a new identity needs it typed twice the same, and different entries make no file. Identity files are age files with mode 0600 and are never overwritten. A repository without `cache.toml` exits 2; an unreachable server exits 1 and `status` says the cache is off. |
| OIDC | A matching policy yields a session with the policy's scopes. A wrong `aud`, an expired token, `alg: none`, an HMAC algorithm, an unknown `kid` or an unmatched claim gets 401. A policy with only `environment` (no immutable id claim) is refused with 400 on a GitHub-shaped issuer, on a GitLab-shaped one, and on a generic issuer registered with `sub` as its id claim. A token from another repository with the same environment name gets 401. A workload policy granting `cache:admin` or `tenant:admin` is refused. |
| Issuer SSRF | Registering issuers at `https://127.0.0.1`, at a private address, at a name that resolves to a private address, and at a public URL that redirects to a private one: every case is refused, and no decoy listener sees a connection. With an issuer allowlist configured, an unlisted public issuer is refused. |
| DNS rebinding | The issuer's name resolves to its allowed address at registration. The fake DNS then answers with a private address for the periodic JWKS refetch, and again for a refetch on an unknown `kid`, and then alternates answers between the check and the connect. No decoy listener ever sees a connection, the refetch fails, and the IdP's own listener sees only connections whose SNI is the issuer name. |
| Roster checks | A `cache:admin` token's PUT of an unsigned roster, a roster signed by an unregistered key, or one with epoch `current + 2` gets 400, and clients keep working. A roster signed by both the old and the new owner key opens for clients pinned to either. |
| Tokens | A revoked or expired token gets 401 on the next call, mid-session included. |
| Tenancy | Tenant B's token on tenant A's cache gets 404. |
| Usage | `usage export` equals the counts the test drove (bytes, ids, hits, commits). |
| Ledger across roll-up | Two server nodes share one Postgres through the fault proxy, under an accelerated fake clock. The proxy drops the reply to one of node A's flush commits just before midnight, so node A retries after the 01:00 roll-up. Node B is `SIGSTOP`ped across the roll-up with counters in memory, then resumed. The monthly export equals the driven counts minus exactly node B's late counters (also reported by `oxabl_cache_ledger_late_dropped_total`), and nothing is counted twice. A third node restarts mid-test: its new process draws a new `node_id`, and its first flush after the restart is counted, not ignored. The fault proxy fails one of node A's flushes outright. Node A sends nothing newer until that flush succeeds, and `oxabl_cache_ledger_ignored_flushes_total` counts only replies that were dropped after the write. |
| Migration | A populated SQLite database (two tenants, three caches, tokens, usage ledger, committed packs) is migrated to Postgres with `--from a.toml --to b.toml`. The printed per-table counts and digests match the source, the source file is unchanged, and every live pack is in the target store with the source's bytes: in a second directory, in a Garage bucket, or left in place when both files name the same store. A second run is refused as "not empty", writes no pack and leaves the target as it was. The server switched to the target configuration serves every lookup (old tokens, the operator token, quotas, object bytes) and takes new writes. A pack with a flipped segment byte, a flipped footer byte, a truncated body or no object at all refuses the migration, names the pack and leaves every metadata table of the target empty, whether the packs are copied or verified in place. A source with an unknown table, a missing configuration, a SQLite target or a Postgres source is refused before the target is touched. |
| Backup | A writer commits packs continuously while `backup` runs. The copy is restored over `meta.db` and the server restarted. Every pack committed before the backup began is fully present and none is half present, the counters equal the restored packs, scrub finds nothing, and new writes work. The copy has mode 0600, no staging file is left, and a second backup to the same path is refused. |
| Deletion and teardown | Deleting a cache gives 404 at once on lookup, info, reservation, a repeated delete, and an old object URL, while its bytes and rows are still present. After the grace, its objects and rows are gone and its name is free, while a sibling cache and another tenant are unchanged and the usage ledger keeps its rows. Deleting a tenant gives 401 for its tokens at once, and empties every table for it after the grace. A server `SIGKILL`ed twice during a teardown with `gc.batch = 2` completes it after restarting. Runs on every backend. |
| Secret modes | The URL key file at 0600 and at 0440 with the server's own group starts without a warning; 0444, 0404, 0460 and 0644 warn `readable by other users`. **Not covered yet:** 0440 with a group the process is not in, which must warn. The harness cannot create a file owned by a foreign group without privileges. |
| Health | `/livez` and `/readyz` answer on the metrics listener only. `/readyz` answers 503 naming the store while the local store root is removed, and 200 again once it is back. `/v1/health` answers 204 with no body, with or without garbage credentials, and stays 204 after the tenant's burst is exhausted. |
| Operator tokens | A second token is minted, the first is revoked with it, and the first is refused at once (CLI and raw HTTP). The list shows `revoked_at`. Revoking the last valid token gets 409 `last_operator_token`. A tenant token is not an operator token. Runs on SQLite and Postgres. |
| Client durability | 300 keys fetched with `Durability::PerFile` under throttling: every `Installed` file exists complete and byte-identical when its outcome arrives. A driver `fetch` `SIGKILL`ed at a random point (4 rounds, fresh stage each) leaves only temp names, or files byte-identical to what was stored. That `syncfs` ran before the renames is not observable black-box; neither is a power cut. |
| Restore | A database snapshot older, then newer, than the bucket gives only misses, and reports clear the stale entries. |

## Adversarial tests

The common assertion: no file at a final path ever differs from what a trusted
producer stored, the outcome is a miss, and no temp file remains.

- **Tampered storage.** Flip one byte of a pack in the segment header, a chunk body, a
  tag, the footer and the trailer, on disk, in Garage and in versitygw. Fetch misses, the report is
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
  paths, backslashes, NUL bytes, duplicate paths and over-long paths, and the stage
  contains a symlink pointing outside it. The filesystem outside the stage is
  byte-identical before and after, and every case misses.
- **Claimed segment ids.** A developer opens a trusted record (they hold the root
  key), uploads a personal pack of garbage, and commits an `OXCM` naming the trusted
  segment id at an offset in their own pack. The entry gets `invalid`, the trusted
  segment's location is unchanged, and every trusted entry still installs for CI and
  for other developers.
- **PUT after commit.** After a pack's first commit, a `client_ref` retry returns
  `committed` with no URL. A replay of the original PUT URL is refused where the store
  supports `If-None-Match`, by the store's checksum where the URL carries the pack's
  SHA-256 (Garage), and by the local route. Where a store has neither, the overwrite
  is caught instead: the next commit retry gets `pack_changed`, and scrub and
  compaction report the ETag change (covered by the tamper scenarios).
- **Commit retries.** The fault proxy drops the reply to a commit, and the client
  retries. The retry returns byte-identical `OXCA` statuses in each of three states:
  - after the commit's entries were purged and their segments died;
  - after a losing commit whose entries were all `exists`, so its pack is already
    dead;
  - after compaction moved the pack's segments.

  No retry credits `live_bytes` or revives a segment, and the client reports
  `Committed` or `Existed` exactly as the first answer said.
- **Client retry discipline.** The fault proxy records every commit body. For each
  dropped reply, the retried body is byte-identical to the first. When the proxy
  instead forges `409 pack_closed` on the retry, every key of that pack reports
  `Failed(Unavailable)`, never `Committed`, and the log names the pack id.
- **Commit by a reader.** A principal with only `read` posts commits (random bodies
  and a copy of a real `OXCM`) to pack ids taken from its lookups. Each gets 403. The
  fault proxy in front of the store sees no `HEAD` or `GET` caused by them, and no
  scrub is queued. A `write:personal` principal committing to a trusted pack, or to another
  user's pack, also gets 403.
- **Second commit to a pack.** A commit to an already-committed pack with a
  different body gets 409 `pack_closed`, and so does a client commit to a pack
  written by compaction. After the sweep, every live entry still installs.
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
`fuzz/run.sh [seconds]` runs every target in turn. **Known gap:** it currently fails
with `Argument list too long` on 3 of the 22 targets, so those three do not run until
the script is fixed; the script exits non-zero, so the failure is visible.

| Target | Property |
| --- | --- |
| `format_*` (each of `OXLQ`, `OXLR`, `OXCM`, `OXCA`, `OXPK` footer, `OXRS`, `OXDP`) | Never panics; `decode(encode(x)) == x`; output of any successful decode re-encodes byte-identically. |
| `open_record`, `open_segment` (fixed key) | Never panics, bounded allocation, `Ok` only for inputs produced by `seal`. |
| `http_request` | Arbitrary method, path, headers and body into the in-process router on in-memory SQLite and local store: no panic, every non-2xx is a valid error envelope, no 500 for malformed input. |
| `install` | Arbitrary path lists and contents into an install under a scratch root: nothing created outside the root, no temp files left, every final file complete. |

## Load tests

`tests/load` holds `oxabl-cache-load`, built on the public client API, against
`s3 + postgres` on dedicated hardware, weekly and before each release. The 30 ms RTT
comes from `tc netem` inside a network namespace on that runner only.

| Scenario | Pass |
| --- | --- |
| 50 concurrent CI full uploads of 10,000 synthetic sets | zero errors; each ≤ 12.5 s at 1 Gbit/s |
| Modelled peak: 500 tenants' worth, 4 full-graph plan lookups (20,000 ids, so 8 probes of 10,000) per second for 10 min | p99 ≤ 300 ms; database CPU ≤ 25% of a 4-vCPU primary (8 probes × 40 ms = 0.32 cores at the pessimistic estimate, 8%) |
| Capacity ramp: 10,000-id probes, raised until p99 exceeds 1 s | Reports the measured probe cost and the probes per second at p99 = 300 ms. Must reach ≥ 80 probes/s (10x the modelled peak) on a 4-vCPU primary. If it does not, the headroom in architecture.md "Scaling" is corrected in the same change. The gate never tightens a CPU threshold beyond what the measured cost allows. |
| Cold-fetch install: one client, 10,000 sets with XREF and debug lists (~30,000 files) | reports install time for `PerFile` and for `Caller` plus one `syncfs`; within 25% of the budget |
| 200 concurrent cold fetches | per-client time within 25% of the budget (architecture.md); zero wrong installs |

## Build gates

Every pull request runs:

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo deny check`
  (advisories, licences, bans), and `#![forbid(unsafe_code)]` in every crate.
- **Server crypto ban:** `scripts/check-server-deps` fails if
  `cargo tree -p oxabl-cache-server -e normal` contains `oxabl-cache-crypto`,
  `oxabl-cache-client` or `age`. A fixture crate that depends on `oxabl-cache-crypto` must
  make the script fail; that case runs in the same job, so the gate cannot silently
  pass.
- **Public-repo hygiene:** a grep for internal identifiers over the tree, with a
  fixture that must trip it.
- The e2e suite on both backends, and 60 s of each fuzz target.
