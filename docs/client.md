# Client library

`oxabl-cache-client` (`oxabl_cache_client`) is the only crate oxabl links. It owns
everything between a plaintext `CompileKey` and a verified file in oxabl's stage:
lookup ids, compression, encryption, signing, packing, batching, transfer,
verification and atomic installation. oxabl owns what goes into a compile key; the
cache never defines it.

## The seam with oxabl

- oxabl computes a 32-byte `CompileKey` per compile unit (BLAKE3 over canonical,
  root-relative compile inputs).
- An artifact set is a list of artifact paths. Each is relative to oxabl's logical
  artifact root (`r/…`, `xref/…`, `dbg/…`, `lst/…`). The client treats paths as
  opaque relative strings and only validates them.
- `fetch` streams one outcome per key as it completes and installs into a stage
  directory that oxabl chooses. oxabl promotes from there.
- `store` is an `Uploader` that oxabl feeds during the build, one artifact set at a
  time, as soon as a unit's verification passes. Each file is an `ArtifactFileRef`:
  its artifact path, its absolute location on disk, its size and its BLAKE3. The
  client reads the file lazily from that location and checks size and hash on that
  read, so a file edited between verification and upload is never stored. The
  artifact path and the disk location are independent, so any output layout works
  (for example `output = "rcode"` and `xref = "reports/xref"`).
- `fetch` installs every key of one call into the same tree,
  `stage/<artifact path>`. Artifact paths are unique within a build (oxabl's
  output-collision rule), so sets never overlap, and one call keeps the batched
  lookup of 10,000 ids per request.
- Each `Installed` outcome carries its origin (`Provenance`: tier and the signer's
  public key, plus epoch and roster version), so oxabl's local cache keeps the tier.
- Every cache failure is a miss or "not stored". The cache never fails a build;
  oxabl compiles what it could not fetch. Each failure is also appended to the failure log
  (below), which `status` shows.

## Public API

```rust
pub struct CompileKey(pub [u8; 32]);

/// Relative artifact path: UTF-8, `/`-separated, no empty, `.` or `..` components,
/// no leading `/`, no `\`, no NUL, no drive prefix, at most 1,024 bytes.
pub struct ArtifactPath(/* private */);
impl ArtifactPath {
    pub fn new(path: &str) -> Result<ArtifactPath, PathError>;
    pub fn as_str(&self) -> &str;
}

/// Configuration; trust fields come only from `.oxabl/cache.toml`.
pub struct Config { /* private */ }
impl Config {
    /// Reads `<root>/.oxabl/cache.toml` and the environment.
    pub fn from_repo(root: &std::path::Path) -> Result<Config, ConfigError>;
    /// The failure log of this cache, readable without opening it; empty when there is none.
    pub fn recent_errors(&self, limit: usize) -> Vec<LoggedError>;
    pub fn with_mode(self, mode: Mode) -> Config;
    pub fn with_credentials(self, credentials: Credentials) -> Config;
    pub fn with_identity(self, identity: IdentitySource) -> Config;
    pub fn with_server(self, url: url::Url) -> Config;
    pub fn with_concurrency(self, connections: usize) -> Config;
    pub fn with_zstd_level(self, level: i32) -> Config;
    pub fn with_max_plain_bytes(self, bytes: u64) -> Config;
    /// Replaces `$XDG_STATE_HOME/oxabl-cache` (per-cache state, the failure log and stored sessions).
    pub fn with_state_dir(self, dir: std::path::PathBuf) -> Config;
    /// A PEM bundle trusted in addition to the system roots (a private CA).
    pub fn with_ca_file(self, path: std::path::PathBuf) -> Config;
    /// Supplies the passphrase of an identity file; `None` from the callback is an error.
    pub fn with_passphrase(self, ask: impl Fn() -> Option<SecretString> + Send + Sync + 'static) -> Config;
}

pub enum Mode { Ci, Developer, ReadOnly }
pub enum Credentials { Auto, Token(SecretString), OidcIdToken(SecretString) }
pub enum IdentitySource { Auto, Value(SecretString), File(std::path::PathBuf) }

/// A handle to one cache; cheap to clone and safe to share across tasks.
#[derive(Clone)]
pub struct Cache { /* private */ }

impl Cache {
    /// Authenticates, fetches and verifies the roster, unwraps the root key.
    pub async fn open(config: Config) -> Result<Cache, OpenError>;

    /// Plan mode: presence only, no verification. A hit predicts; `fetch` decides.
    pub async fn probe(&self, keys: &[CompileKey]) -> Result<Vec<Presence>, CacheError>;

    /// Starts fetching; yields one outcome per request as each completes.
    pub fn fetch(&self, requests: Vec<FetchRequest>, options: FetchOptions) -> Fetch;

    /// Asks the server for up to `count` (1 to `MAX_SAMPLE`) live trusted entries and fetches
    /// them as `fetch` does; an outcome's `key` holds the entry's lookup id.
    pub async fn fetch_sample(&self, count: usize, options: FetchOptions) -> Result<Fetch, CacheError>;

    /// Starts a batched upload session. Fails if the mode does not write.
    pub fn uploader(&self) -> Result<Uploader, CacheError>;

    pub fn mode(&self) -> Mode;
    pub fn stats(&self) -> Stats;
    /// The mode, the member, the verified roster and the server's epoch, dictionary and quota.
    pub async fn status(&self) -> Result<Status, CacheError>;
    /// The last `limit` failures logged for this cache on this machine, newest first.
    pub fn recent_errors(&self, limit: usize) -> Vec<LoggedError>;
    /// Every fetch and store the cache's members shared in the current epoch, each once.
    pub async fn team_activity(&self) -> Result<Vec<Activity>, CacheError>;
    /// Sends this machine's unshared activity now and waits for it.
    pub async fn share_activity(&self);
    /// Seals `dictionary`, commits it and moves the pointer; a `ci` member in `ci` mode only.
    pub async fn publish_dictionary(&self, dictionary: &[u8]) -> Result<DictionaryPublished, DictionaryError>;
}

pub enum Presence { Hit, Miss }

pub struct FetchRequest {
    pub key: CompileKey,
    /// If set, the entry's path set must equal this set exactly, or the outcome is `PathMismatch`.
    pub expect: Option<Vec<ArtifactPath>>,
}

pub struct FetchOptions {
    /// Directory files are installed under (`stage/<artifact path>`).
    pub stage: std::path::PathBuf,
    pub durability: Durability,
    /// Ends the fetch when nothing progresses for this long; default 30 s.
    pub idle_timeout: std::time::Duration,
}

pub enum Durability {
    /// fsync every file and directory before reporting `Installed` (the default).
    PerFile,
    /// No fsync; the caller makes the stage durable (oxabl: one `syncfs` before promotion).
    Caller,
}

/// `futures_core::Stream<Item = FetchOutcome>`. Dropping it cancels what has not completed.
pub struct Fetch { /* private */ }
impl Fetch {
    pub async fn next(&mut self) -> Option<FetchOutcome>;
}

pub enum FetchOutcome {
    Installed { key: CompileKey, files: Vec<InstalledFile>, provenance: Provenance },
    Miss { key: CompileKey, reason: MissReason },
}

pub struct InstalledFile { pub path: ArtifactPath, pub size: u64 }

/// Origin of a hit, for callers that keep a local copy; `signer` is the Ed25519 public key.
pub struct Provenance { pub tier: Tier, pub signer: [u8; 32], pub epoch: u32, pub roster_version: u64 }
pub enum Tier { Trusted, Personal }

pub enum MissReason {
    NotFound,
    Untrusted,
    Corrupt,
    PathMismatch,
    Timeout,
    Unavailable,
    Install(std::io::ErrorKind),
}

/// A verified output as the caller saw it: stored under `path`, read from `disk_path`.
pub struct ArtifactFileRef {
    pub path: ArtifactPath,
    /// Absolute; read once, lazily. Only the final component must not be a symlink.
    pub disk_path: std::path::PathBuf,
    pub size: u64,
    pub blake3: [u8; 32],
}

pub struct Uploader { /* private */ }
impl Uploader {
    /// Queues one set; returns once queued, waiting only for back-pressure.
    pub async fn put(&self, key: CompileKey, files: Vec<ArtifactFileRef>);
    /// Seals and uploads the rest, commits every pack, and reports per key.
    pub async fn finish(self) -> StoreReport;
}

pub struct StoreReport { pub outcomes: Vec<(CompileKey, StoreOutcome)>, pub bytes_uploaded: u64 }
pub enum StoreOutcome {
    Committed,
    Existed,
    /// A file's size or BLAKE3 differed from its `ArtifactFileRef` when read; nothing was uploaded for the key.
    Changed,
    Failed(StoreFailure),
}
pub enum StoreFailure { Rejected, TooLarge, Unreadable, QuotaExceeded, Unavailable }
```

The command line and the desktop app use the rest of the public API, which oxabl does not
need:

- `Config::local_activity()`, the fetches and stores of this machine (below), and
  `ActivitySummary::of(&activities)` for their averages.
- `Invite::new(&config, name, sign_in)`, with `sign_in` an optional `(issuer, client_id)`,
  `Invite::parse`, `to_text`, `sign_in()`, `config()` and `cache_toml()` (below).
- `me(&config)`, the caches the credential holds a grant on (`GET /v1/me`); `round_trip(&config,
  tries)`, the fastest round trip to the server's health check; and `signed_in(&config)`, whether
  a stored login session for the tenant is unexpired.
- `save_passphrase`, `stored_passphrase` and `forget_passphrase`: the identity passphrase in the
  operating system's keyring (below). `save_token`, `stored_token` and `forget_token` keep an
  access token there the same way, under the account `token <tenant> <server>`; the library
  never reads it by itself, the caller passes it as `Credentials::Token`.

- `Owner::open(config)` opens the cache as an owner whose signing key the repository pins, and
  `add_member`, `remove_member`, `rotate`, `add_owner` and `remove_owner` publish the next roster
  (signed, checked against the pins, sent as `current + 1`), returning `Published`. `init(request)`
  publishes the first roster.
- `DeviceLogin::new(&config, issuer, client_id)` with `begin` and `finish`, and `logout(&config)`,
  for the device grant and the stored session.
- `create_identity_file`, `open_identity_file`, `write_identity_secret`, `default_identity_path`,
  `signing_key_text` and `signing_key_hex` for identity files.
- `train_from_outputs(dir, options)` for a dictionary and its measured gain, and
  `ArtifactFileRef::from_disk` to measure a file for `put`.

`FetchOptions::new(stage)` gives `Durability::PerFile` and a 30 s idle timeout.

`MissReason`, `StoreOutcome` and `StoreFailure` are exhaustive, not `#[non_exhaustive]`: a new
variant is a compile error in oxabl's adapter, which must decide what it means.

`OpenError` covers:

- an invalid config (`ConfigError`). This includes `TrustOverride` (an attempted
  override of a trust field), `Invalid` (for example a malformed `owner_keys` entry)
  and `Environment` (for example an unknown `OXABL_CACHE_MODE`);
- no usable credential (`NoCredentials`): nothing configured, or a stored session
  that is missing or expired;
- an unreachable server, or failed authentication. An ID-token source that is
  configured but fails (issuer unreachable, wrong audience, exchange refused) is an
  error; it does not fall through to the stored session;
- a roster failure: a bad signature, no pinned owner signature, or a rollback below
  the high-water mark or `min_roster_version`;
- an identity missing from the roster.

oxabl reports it and builds without the cache. A configuration error is shown as an
error, a transient one as a warning.

`uploader()` fails with `CacheError::NotAWriter` when the mode does not write, or when
a `ci`-mode identity is not a `ci` member of the roster. A `developer` writes its own
personal namespace.

### Transport and bounds

- reqwest with rustls, TLS 1.3 minimum. Redirects are never followed. Connect
  timeout 10 s, read timeout 60 s per chunk, no total timeout (the idle timeout is
  what ends a stalled fetch).
- The server URL must be `https`, except `http` on a loopback address.
- Object URLs never carry the bearer token. A 403 on an object read renews that
  pack's URL once, through `urls`.
- A PUT forwards only the headers the server signed (`if-none-match`,
  `content-md5`, `x-amz-*`).

| Bound | Value |
| --- | --- |
| Connections | 32 (configurable 1 to 128) |
| Sealed bytes in flight for fetch | 256 MiB |
| Plaintext held for sealing | 256 MiB |
| Sealed bytes queued or uploading | 256 MiB |
| CPU workers | available parallelism |
| Packs in flight | 8 |
| Sets queued in `put` | 10,000 |
| Lookups | at most 10,000 ids each, two in flight |
| Range coalescing | gaps of at most 1 MiB, at most 8 MiB per request |
| Corruption reports | at most 10 per handle, once per segment id, in the background (the server allows 10 a minute) |
| Roster refetch | at most once per `fetch` call, shared by all its chunks |

Dictionaries are cached per handle. These bounds are the design. testing.md's
`RLIMIT_AS` scenarios are what must prove the client's peak memory actually stays
under them.

## Fetch

1. Derive lookup ids (0.08 µs each) and send `lookup` in chunks of 10,000, with
   records and URLs.
2. Open each record. Check:
   - the commitment and AEAD;
   - the signature transcript, and that the signer is accepted for the namespace
     (see "Modes and trust");
   - the roster version. A record naming a roster version above the client's current
     roster is `RosterVersionAhead`, a miss for now. The client then refetches the
     roster, at most once per `fetch` call (verified as at open), and retries the
     lookup of those keys once with the newer roster. A record that is still ahead
     after that is a miss (`Untrusted`);
   - for `personal` records, that the reading member is listed in the current roster;
   - the path rules: no duplicates within the set, and the total path bytes within
     the limit;
   - `expect`;
   - that `plain_len` is within the configured limit.

   A failure moves on to the personal hit, if any, or else gives a miss.
3. Dictionaries first. For each distinct non-zero `dict_id` among the opened
   records, take its location from the response's `dictionaries` list. Fetch the
   segment once per `Cache` (it is kept in memory, keyed by id). Verify its BLAKE3
   against the `dict_id` signed in the record, then decrypt it. A dictionary that is
   missing from the list, or that fails verification, turns every entry needing it
   into a miss (`Corrupt` on a failed check, `NotFound` when absent). It never
   holds back other entries.
4. Group segments by pack and coalesce ranges with gaps of 1 MiB or less, splitting
   reads above 8 MiB. Run them with 32 concurrent connections (configurable up to
   128) and at most 256 MiB in flight.
5. A range is read as a stream. Each segment goes to a CPU worker as soon as its last
   byte has arrived, so outcomes appear while the rest of the range is still coming.
   Per segment, on that worker:
   - check `BLAKE3(bytes)` against the signed segment id. On a mismatch, send
     `reports` in the background and miss;
   - check the header, decrypt the chunks in order, and strip the padding;
   - decompress (with the verified dictionary when `dict_id` is set) with a window
     limit of 128 MiB and an output cap of `plain_len`, with the zstd frame checksum
     verified;
   - split into files by the signed sizes.
6. Install (below), then yield `Installed`. Expired URLs are refreshed once through
   `urls`.

Outcomes arrive in completion order, not request order. Every request gets exactly one
outcome. The idle timeout restarts on every received byte, every outcome delivered and every
durability flush. Time spent waiting for the reader also counts as progress: while the outcome
buffer is full and a sender waits for room, the fetch is not idle, so a slow reader never
loses an outcome. A link that stops while the reader is fast still times out. When the idle
timeout fires, every request without a delivered outcome yields `Miss { reason: Timeout }`
and the stream ends. A cold fetch on a slow link therefore keeps going as long as bytes flow.
Whatever was installed stays installed. Dropping a `Fetch` stops the work, removes its temp
files, and leaves completed installs alone.

### Install

A final path never holds a partial or unverified file.

- `stage` is opened as a directory handle. Every component is created and walked
  beneath it with `openat` and `O_NOFOLLOW` (`cap-std`). Symlinks, `..` and absolute
  paths cannot escape it, whatever a record says.
- `Durability::PerFile` (the default): every file is durable before it gets its final
  name. Durability is batched, in this order:
  1. Each verified segment is opened into `.<name>.oxc-<random>.tmp` files beside
     its targets. Nothing has a final name yet.
  2. The complete, verified set goes to a committer. A batch is flushed at 128 sets,
     32 MiB or 20 ms after its first set, and always at the end of a lookup chunk.
  3. Per batch, one `syncfs` on the stage's file system. If `syncfs` or the device
     lookup fails, every temp file is fsynced instead. A temp file on another device
     than the stage root is fsynced individually, even when `syncfs` worked.
  4. Every set's temp files are renamed over their final paths (atomic per file).
  5. Every touched directory is fsynced, once per batch.
  6. Only then are the batch's `Installed` outcomes emitted.

  `syncfs` flushes everything dirty on that file system, other processes' writes
  included, so its cost depends on the machine. Batching keeps it to one call per
  batch rather than one fsync per file.
- `Durability::Caller` skips steps 3 and 5 and emits `Installed` straight after the
  rename. Final names in the stage may then hold bytes that are not yet durable. The
  caller's one `syncfs` before promotion covers that (architecture.md, "Performance
  budget"); it is that mode's contract.
- On any failure, temp files are removed and the outcome is a miss
  (`Install(kind)` at the durability or rename steps). Files already at
  final paths are untouched.
- Renames are atomic per file. If the process dies between two renames of one set,
  each final file is either the old or the new verified version. oxabl records a unit
  as fetched only on `Installed`.

## Store

1. `put` validates the paths (rules above, no duplicates, at most 16 files, total
   path bytes ≤ 3,072, total size within the plaintext cap). It queues only the
   references and waits when 10,000 sets are queued or 256 MiB of sealed bytes are in
   flight.
2. Workers take one set at a time. They open each file at its absolute `disk_path`
   with `O_NOFOLLOW`, which refuses a symlink in the final component only.
   Intermediate directories may be symlinks (a workspace under a symlinked
   directory is normal), and that is harmless because the verified size and BLAKE3
   bind the content. A relative path, a final-component symlink or a non-regular file
   gives `Failed(Unreadable)`. Each file is read once into memory, and check its size and BLAKE3
   against the `ArtifactFileRef`.
   - Any mismatch makes the key `Changed`, and nothing of that set is sealed or
     uploaded. Because the bytes compressed are exactly the bytes checked, a file
     edited between oxabl's verification and the upload is never stored.
   - A missing or unreadable file gives `Failed(Unreadable)`.

   Each matching set is compressed (zstd level 3 by default, with the cache dictionary
   if one is published, frame checksum on), padded, sealed, and appended to the open
   pack. A pack is closed at 16 MiB or 2,000 segments, and a
   segment over 16 MiB gets its own pack. The client uses a published dictionary only
   if its `OXDP` pointer verifies: signed by a `ci` or `owner` key of the current
   roster, with an epoch equal to the roster's epoch. The writer finds both through
   `GET …/dictionary` (protocol.md), which returns the pointer and the dictionary
   segment's location. It then checks the fetched segment's BLAKE3 against the
   pointer's `dict_segment_id`. Otherwise it compresses without a dictionary.
3. Each closed pack goes through `uploads` (with a `client_ref`, kept across retries,
   and a fresh one after a 409), `PUT`, then
   `commit`, with up to 8 packs in flight. Records are signed with the member's key.
   Uploads therefore overlap compiling.
4. A failed PUT is retried 3 times with backoff, with the same `client_ref`. After
   that, its keys report `Unavailable`.
5. Commit is exactly-once on the server. The client builds each pack's `OXCM` once
   and keeps those exact bytes until a reply arrives. A commit whose reply is lost
   (timeout, connection error, 5xx) is retried 3 times with backoff, sending the
   byte-identical body; it is never re-sealed, re-signed or reordered. The recorded
   `OXCA` that comes back is mapped per key as usual. On a retry, `409 pack_closed`
   or `409 pack_changed` maps every key of the pack to `Failed(Unavailable)` and
   is logged with the pack id and the `reason`. On a first attempt, either one is a
   client bug, and is logged and reported the same way. A 409 on commit is never
   retried.
6. `quota_exceeded` stops further uploads for the session. A `put` after the mode or
   the quota stopped writing is reported as a failure, never an error.

`finish` waits for every queued set and returns one outcome per key.

## Modes and trust

| Mode | Reads | Accepts signers | Writes | Typical credentials |
| --- | --- | --- | --- | --- |
| `ci` | trusted | `ci` members | trusted, signed with its `ci` key | OIDC, or a token with `write:trusted` |
| `developer` | trusted, then own personal | `ci` members; own keys for personal | own personal | `oxabl-cache login` session or a token |
| `read-only` | trusted, then own personal (if the identity is a developer) | as `developer` | none | OIDC, a token or a session with `read` |

If no mode is given and the `CI` environment variable is set, the mode is
`read-only`; otherwise it is `developer`. Writing to trusted always needs an explicit
`ci`. CI systems that do not set `CI` (Jenkins, TeamCity, Bamboo agents, for
example) must set `OXABL_CACHE_MODE`.

## Configuration

`.oxabl/cache.toml` is committed in the customer's repository. It is the only source
of the cache identity and the root of trust, and it holds no secrets:

```toml
server = "https://cache.example.com"
tenant = "t_2h8k4m6p9q1r3s5t7v0w2x4y6z"
cache = "c_6v2k9q3m8x1r4t7w0y5z2b8n4d"
owner_keys = ["ed25519:Jx1…"]
min_roster_version = 12
```

Each `owner_keys` entry has exactly one spelling: `ed25519:` followed by the 32-byte
key in standard base64 without padding. Padding, the URL-safe alphabet, whitespace,
a missing prefix, or non-zero trailing bits are `ConfigError::Invalid`.

- `Config::from_repo(root)` reads exactly these five keys. Unknown keys are an
  error, and so is a file that is not a regular file inside `root`.
- `tenant`, `cache`, `owner_keys` and `min_roster_version` can never be overridden.
  `Config` has no setter for them. `from_repo` fails with
  `ConfigError::TrustOverride` if the environment sets `OXABL_CACHE_TENANT`,
  `OXABL_CACHE_CACHE`, `OXABL_CACHE_OWNER_KEYS` or `OXABL_CACHE_MIN_ROSTER_VERSION`.
  That way an attempt is loud, never silently ignored.
- The OIDC audience is always `oxabl-cache:<tenant>`, derived from the committed
  `tenant`. The client never takes an audience from the server, the environment or
  a flag.
- `server` may be overridden by `OXABL_CACHE_SERVER` or `with_server`. The server is
  untrusted anyway.
- Tuning (concurrency, zstd level, plaintext cap) is set by the caller through
  `Config` methods. oxabl takes those values from its own `oxabl.toml`.

Mode, credentials and identity come from the environment or from the caller's flags
(through the `with_*` methods):

| Variable | Meaning |
| --- | --- |
| `OXABL_CACHE_MODE` | `ci`, `developer` or `read-only`; any other value is `ConfigError::Environment` |
| `OXABL_CACHE_TOKEN` | API token. It takes precedence over OIDC and login. |
| `OXABL_CACHE_IDENTITY` | The member identity string (CI) |
| `OXABL_CACHE_IDENTITY_FILE` | Path to an identity file; default `$XDG_CONFIG_HOME/oxabl-cache/identity` (or `~/.config/…`). The file holds the identity string age-encrypted to a passphrase (scrypt); plaintext identity files are not read |
| `OXABL_CACHE_IDENTITY_PASSPHRASE` | The identity file's passphrase, used only when no `with_passphrase` callback is set |
| `OXABL_CACHE_SERVER` | Server URL override |
| `OXABL_CACHE_ID_TOKEN` | A GitLab CI ID token from the job's `id_tokens` |

`Credentials::Auto` tries, in order:

1. `OXABL_CACHE_TOKEN`.
2. GitHub Actions OIDC, when `ACTIONS_ID_TOKEN_REQUEST_URL` and
   `ACTIONS_ID_TOKEN_REQUEST_TOKEN` are set. It requests a token with audience
   `oxabl-cache:<tenant>`.
3. GitLab, through `OXABL_CACHE_ID_TOKEN`. The job declares it under `id_tokens`
   with `aud: oxabl-cache:<tenant>`.
4. The stored `oxabl-cache login` session.

`IdentitySource::Auto` tries `OXABL_CACHE_IDENTITY`, then the file named by
`OXABL_CACHE_IDENTITY_FILE`, then the default identity file. The passphrase comes
from the `with_passphrase` callback, or else from `OXABL_CACHE_IDENTITY_PASSPHRASE`, or else
from the operating system's keyring (the Secret Service on Linux, with the default
`os-keyring` feature), where `save_passphrase` or `oxabl-cache identity remember` put it under
the service `oxabl-cache` and the identity file's absolute path. No keyring, or none running,
is the same as no passphrase there.
Unlocking (scrypt) runs on a blocking thread.

The stored login session is `$XDG_STATE_HOME/oxabl-cache/session/<tenant id>`
(default state directory `~/.local/state`). It is a JSON file
`{"token": "oxs_…", "expires_at": <unix seconds>}`, a regular file of at most 16 KiB,
and must have mode 0600: any group or other permission bit is an error, not ignored.
`oxabl-cache login` writes it.

Before sending any ID token, the client decodes its payload and refuses to send it
(`OpenError`) unless `aud` is exactly `oxabl-cache:<tenant>`. A token minted for
another audience therefore never reaches the server. Gitea Actions uses a scoped
token (`OXABL_CACHE_TOKEN`). OIDC works with any issuer that publishes discovery and
JWKS documents, once the tenant registers it.

Local state (`$XDG_STATE_HOME/oxabl-cache/<cache>/`) holds the roster high-water
mark (`roster-high-water`, raised under `flock`) and the last verified roster. An
unreadable or malformed high-water file is an error, never treated as zero. A failed
state write is only logged. The root key lives only in memory and is zeroed on
drop.

### Activity

Every fetch (not a sample) and every store of a handle is recorded twice, and neither ever
fails or delays the operation:

- In the journal of this machine, `$XDG_STATE_HOME/oxabl-cache/journal/<tenant>-<cache>.jsonl`
  (the newest 1,000 entries, at most 320 KiB, written as the failure log is). An entry holds
  when it ended, pull or push, the mode, keys asked for (or sets put), hits (installed, or
  committed or already there), misses (not in the cache, or refused for quota), other failures,
  bytes (plaintext installed, or sealed uploaded), the duration and the round trip to the
  server. `Config::local_activity` reads it without opening the cache.
- Shared with the cache's members: in the background, the handle seals each unsent entry as an
  `OXTL` report under the epoch's telemetry key (protocol.md) and sends it, at most 64 at a
  time and only entries of the last 7 days; the journal marks what was sent. `Cache::open`
  sends what an earlier process left unsent, so a CI job that exits early shares at its next
  run. The round trip is measured once per handle against `GET /v1/health`.

`Cache::team_activity` reads the cache's log back, opens the reports of the current epoch and
drops repeats by report id. The server cannot read a report (security.md, "Telemetry").

### Invites

An invite is one line, `oxc-invite:` and the unpadded base64url of a JSON object with a
4-byte BLAKE3 checksum appended: the server, tenant, cache, `owner_keys` and
`min_roster_version` that `.oxabl/cache.toml` pins, a display name, and the OIDC issuer and
public client id people sign in with. The issuer and client id come together or not at all:
without them, people join with an access token a tenant administrator gives them
(`POST /v1/tenant/tokens`), and an invite with only one of the two is refused. It holds no
secret. `Invite::config()` is a `developer`
configuration whose pins come from the invite, so a person who trusts the sender of the invite
trusts the cache as one who trusts the repository. A changed character fails the checksum, and
an issuer that is not `https` is refused.

### Failure log

Every run of the library, so `oxabl build` as well as the command line, appends what failed to
`$XDG_STATE_HOME/oxabl-cache/errors/<tenant id>-<cache id>.jsonl` (under `--state-dir` or
`with_state_dir` when set): a failed `open`, `probe`, `fetch`, `store`, `dictionary` publication
or `status`. A fetch or a store over many entries writes one line per kind of failure, with the
count ("12 of 340 entries failed verification"), not one per entry. An entry that is simply not
in the cache is no failure.

One JSON object per line: `t` (unix seconds), `op`, `kind` and `msg`. The message is fixed text
built from the kind of error and counts. It never holds a secret, a token, a path, a value of
the configuration or the content of a file, and is cut to 160 characters. The log keeps the
newest 30 entries and at most 4 KiB. Writers take an exclusive lock on a lock file beside the
log, add their entries, trim and replace the log with a rename, so concurrent processes lose
and tear nothing and a reader sees the log before or after a write. The log is a regular file of
mode 0600 in a directory of mode 0700; a log that is not a regular file is not read, lines that
do not parse are dropped at the next write, and a file that is group readable is private again
after it. A failure to write the log (no state directory, a full disk, a lock held for more than
250 ms) is ignored: it never fails the operation it records.

## Command line (`oxabl-cache`)

The `oxabl-cache` binary (crate `oxabl-cache-cli`) is built on the public API above, as
oxabl is. Exit codes: 0 done, 1 refused or failed, 2 usage or configuration error. Every
command takes `--json`, which prints one JSON document on stdout; a failure prints its
message on stderr (as `{"error": …, "exit": …}` with `--json`). A command that cannot
establish what it needs fails; it never reports success for nothing.

| Command | Purpose |
| --- | --- |
| `identity new [--file <path>] [--secret-out <path>]` | Create a member identity and print its public descriptor and signing key (`ed25519:` spelling and hex). `--file` (default: the identity file path) writes the passphrase-encrypted file. `--secret-out` writes the identity string to a mode 0600 file, for a CI secret store; given alone it writes no passphrase file. An existing file is never overwritten. |
| `init --server <url> --tenant t_… --cache c_… --label <me>` | Publish roster v1 at epoch 1 with this identity as the only owner, for a cache that exists with this identity's key registered as an owner key. Print the `cache.toml` block. |
| `member add <descriptor> --label <l> --role ci\|developer\|reader` | Re-wrap the root key to the new member; publish roster `v+1` in the same epoch. |
| `member remove <label> [--purge]` | Rotate to a new epoch without the member; publish; print the new `min_roster_version`. With `--purge`, entries of older epochs are marked dead. |
| `rotate [--purge]` | New epoch for all current members; optionally purge older epochs. |
| `owner add <descriptor> --label <l> --cosign-identity-file <f>` | Add an owner (below). |
| `owner remove <label\|ed25519:key> [--purge]` | Remove an owner and start a new epoch (below). |
| `login --issuer <url> --client-id <id>`, `logout` | Device-flow OIDC session for humans (below). |
| `invite --name <n> [--issuer <url> --client-id <id>]` | Print a one-line invite to the repository's cache for the desktop app (above). Without `--issuer` and `--client-id`, people join with an access token. |
| `identity remember`, `identity forget` | Put the identity file's passphrase in the operating system's keyring, after checking that it unlocks the file, or remove it. |
| `status` | Mode, principal, roster, epoch, quota, or why the cache is off, and the last five failures from the failure log (`recent_errors` in `--json`, newest first, each `{at, operation, kind, message}`), whether the cache is on or off. |
| `dict train --outputs <dir> [--publish]` | Train a 112 KiB dictionary from sampled outputs, report the ratio gain, and publish it as `ci` when the gain is at least 1.3x. |
| `verify --sample <n> [--keys <file>]` | Fetch and fully verify a random sample of trusted entries into a scratch directory. Without `--keys` the server names the sample (`n` is 1 to 1,000). |
| `fetch --key <hex> --stage <dir> [--expect <path>]…`, `put --key <hex> --file <artifact>=<path>…` | Manual single-key operations for diagnostics. |

Global flags, valid on every command: `--repo <dir>` (default: the nearest `.oxabl/cache.toml`
above the current directory), `--identity-file`, `--passphrase-file`, `--token-file`,
`--state-dir`, `--ca-file` (extra trusted CAs, for a private CA of the server or the identity
provider; also `Config::with_ca_file`) and `--mode ci|developer|read-only`.

### Setting up a cache

The server picks a cache's id and checks roster signatures against the owner keys registered
at creation, so the owner's key exists before the cache does:

1. The owner runs `identity new`, which prints the signing key.
2. The tenant administrator creates the cache with that key (`POST /v1/tenant/caches`,
   `owner_keys` as hex) and hands out a token with `cache:admin` for it.
3. The owner runs `init`, which publishes roster v1 and prints the `.oxabl/cache.toml` block
   that pins the cache. The repository commits it.
4. The owner runs `member add` for each descriptor a member sends. A `ci` member's identity
   string goes to the CI secret store (`identity new --secret-out`).

`init` refuses a cache that has a roster, and the server refuses a roster whose signature is
not by a registered owner key.

### Secrets

A secret comes from a file named by a flag, from the environment variable the library reads for
it, or for a passphrase from the controlling terminal (echo off). The first that is present wins
and a named file is never replaced by a variable:

| Secret | Flag, then | Then | Last |
| --- | --- | --- | --- |
| Server credential | `--token-file` | `OXABL_CACHE_TOKEN` | a CI system's ID token and the stored login session, as in "Configuration" |
| Identity | `--identity-file` | the identity string in `OXABL_CACHE_IDENTITY` | the default identity file (`OXABL_CACHE_IDENTITY_FILE`, else `$XDG_CONFIG_HOME/oxabl-cache/identity`) |
| Identity file passphrase | `--passphrase-file` | `OXABL_CACHE_IDENTITY_PASSPHRASE`, then the keyring | the terminal; without one the command fails instead of waiting |

So a CI job that sets `OXABL_CACHE_TOKEN` and `OXABL_CACHE_IDENTITY` for `oxabl build` runs
`oxabl-cache dict train --publish` the same way. An identity string needs no passphrase. `init`
and `owner add` take an identity file only: an owner's key is not a job secret.

A secret file is a regular file of at most 4 KiB with no group or other permission bit; anything
else is a usage error. No flag takes a secret as its value: the flags that name secrets end in
`-file`, and a secret in the arguments is an unknown argument (exit 2). A command prints no
passphrase, token, session or identity string, in text or JSON, nor in an error, and a secret
from the environment is never logged; `identity new --secret-out` is the only way an identity
string leaves the process, into a mode 0600 file.

### Owners

An owner is a roster member whose role is `owner`. It holds the root key and its signing key
is one of the cache's owner keys (`owner_keys` in `.oxabl/cache.toml` and the server's).

- `owner add` publishes roster `v+1` in the same epoch with the new owner as a member. The
  roster is signed by the current owner and by the new owner's own key (`--cosign-identity-file`,
  and `--cosign-passphrase-file` or a prompt), so a checkout that pins either key opens it
  (security.md, "Planned owner-key change"). The descriptor must name the cosigning identity.
  The command prints the `owner_keys` line and the `min_roster_version` for
  `.oxabl/cache.toml`, and the call that registers the keys on the server
  (`PUT /v1/tenant/caches/<c>/owner-keys`, by the tenant administrator).
- `owner remove` names an owner by label or `ed25519:` key. Another owner signs the new roster,
  which omits the member and starts a new epoch, as for any removal. The command prints the
  `owner_keys` line without the key and the new `min_roster_version`. A checkout that pins only
  the removed key refuses the new roster and runs without the cache (security.md, "Owner key
  compromise"). The signing owner cannot remove itself, and a roster never loses its last owner.
- `member remove` refuses an owner, and `member add` refuses the role `owner`.

Only an owner whose signing key the repository pins changes the roster, and a roster without
an owner is never signed. A change goes out as `current + 1` over the roster it was made on: if
another roster was published first, the command fails and the change is repeated.

### Login

`login` runs the OAuth device authorization grant (RFC 8628) against the issuer in `--issuer`
with the client `--client-id` (also `OXABL_CACHE_OIDC_ISSUER`, `OXABL_CACHE_OIDC_CLIENT_ID`;
neither is secret). It reads the issuer's discovery document, which must name that issuer, asks
for a device code with `scope=openid` and `audience=oxabl-cache:<tenant>`, and prints the
verification page and the code on stderr. The issuer must be registered for the tenant and
must put exactly that audience in the ID token. The client refuses any other audience before
anything is sent to the cache server (as for every ID token), exchanges the token at
`/v1/auth/oidc` and writes `{"token", "expires_at"}` to
`$XDG_STATE_HOME/oxabl-cache/session/<tenant>` with mode 0600, replacing it atomically. Endpoints
must be `https` (or loopback `http`). `logout` revokes the session with `DELETE /v1/auth/session`
and deletes the file; it exits 1 when the server could not confirm the revocation, and the file
is deleted anyway.

### Verification and dictionaries

`verify --sample n` asks the server for `n` live trusted entries (`GET .../sample`, 1 to 1,000;
`Cache::fetch_sample` in the library, whose outcomes carry the lookup id in `key`). With
`--keys <file>` (one 64-digit hex key per line; `#` starts a comment) it draws the sample from
compile keys the caller knows instead, for example from oxabl's plan, and `n` is then bounded
by the file. Either way it fetches the sample with every check `fetch` makes into a scratch
directory under the state directory, removes it afterwards, and exits 1 unless at least one
entry was sampled and every sampled entry installs from the trusted namespace. An empty cache
prints `sampled 0` and exits 1: nothing was verified is not a pass. `--sample` outside 1 to
1,000 without `--keys` is a usage error (exit 2).

`dict train` samples up to 2,000 regular files under the directory, trains on the first 128 KiB of
80% of them, and measures zstd with and without the dictionary on up to 1 MiB of each of the
other 20%. `--publish` needs `ci` mode and a `ci` identity, and publishes only when the
measured gain is at least 1.3x.
