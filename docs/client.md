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
  oxabl compiles what it could not fetch.

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
    pub fn with_mode(self, mode: Mode) -> Config;
    pub fn with_credentials(self, credentials: Credentials) -> Config;
    pub fn with_identity(self, identity: IdentitySource) -> Config;
    pub fn with_server(self, url: url::Url) -> Config;
    pub fn with_concurrency(self, connections: usize) -> Config;
    pub fn with_zstd_level(self, level: i32) -> Config;
    pub fn with_max_plain_bytes(self, bytes: u64) -> Config;
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

    /// Starts a batched upload session. Fails if the mode does not write.
    pub fn uploader(&self) -> Result<Uploader, CacheError>;

    pub fn mode(&self) -> Mode;
    pub fn stats(&self) -> Stats;
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
    /// Absolute; opened with `O_NOFOLLOW` and read once, lazily.
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

`OpenError` covers:

- an invalid config (`ConfigError`, which includes an attempted override of a trust
  field);
- an unreachable server, or failed authentication;
- a roster failure: a bad signature, no pinned owner signature, or a rollback below
  the high-water mark or `min_roster_version`;
- an identity missing from the roster.

oxabl reports it and builds without the cache. A configuration error is shown as an
error, a transient one as a warning.

## Fetch

1. Derive lookup ids (0.08 µs each) and send `lookup` in chunks of 10,000, with
   records and URLs.
2. Open each record. Check:
   - the commitment and AEAD;
   - the signature transcript, and that the signer is accepted for the namespace
     (see "Modes and trust");
   - the roster version;
   - the path rules: no duplicates within the set, and the total path bytes within
     the limit;
   - `expect`;
   - that `plain_len` is within the configured limit.

   A failure moves on to the personal hit, if any, or else gives a miss.
3. Group segments by pack and coalesce ranges with gaps of 1 MiB or less, splitting
   reads above 8 MiB. Run them with 32 concurrent connections (configurable up to
   128) and at most 256 MiB in flight.
4. Per segment, on a CPU worker pool:
   - check `BLAKE3(bytes)` against the signed segment id. On a mismatch, send
     `reports` in the background and miss;
   - check the header, decrypt the chunks in order, and strip the padding;
   - decompress with a window limit of 128 MiB and an output cap of `plain_len`,
     with the zstd frame checksum verified;
   - split into files by the signed sizes.
5. Install (below), then yield `Installed`. Expired URLs are refreshed once through
   `urls`.

Outcomes arrive in completion order, not request order. The idle timeout restarts on
every received byte and every outcome. When it fires, every key still pending yields
`Miss { reason: Timeout }` and the stream ends. A cold fetch on a slow link therefore
keeps going as long as bytes flow. Whatever was installed stays installed. Dropping a
`Fetch` stops the work, removes its temp files, and leaves completed installs alone.

### Install

A final path never holds a partial or unverified file.

- `stage` is opened as a directory handle. Every component is created and walked
  beneath it with `openat` and `O_NOFOLLOW` (`cap-std`). Symlinks, `..` and absolute
  paths cannot escape it, whatever a record says.
- Each file is written to `.<name>.oxc-<random>.tmp` in its target directory. Only
  after **every** file of the set has passed every check is each temp file renamed
  over its final path.
- With `Durability::PerFile`, each temp file is fsynced before its rename and each
  parent directory once after. With `Durability::Caller`, nothing is fsynced; the
  caller's `syncfs` covers it (architecture.md, "Performance budget").
- On any failure, temp files are removed and the outcome is a miss. Files already at
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
   (`O_NOFOLLOW`; a relative path, a symlink or a non-regular file gives
   `Failed(Unreadable)`), read it once into memory, and check its size and BLAKE3
   against the `ArtifactFileRef`.
   - Any mismatch makes the key `Changed`, and nothing of that set is sealed or
     uploaded. Because the bytes compressed are exactly the bytes checked, a file
     edited between oxabl's verification and the upload is never stored.
   - A missing or unreadable file gives `Failed(Unreadable)`.

   Each matching set is compressed (zstd level 3 by default, with the cache dictionary
   if one is published, frame checksum on), padded, sealed, and appended to the open
   pack. A pack is closed at 16 MiB or 2,000 segments, and a
   segment over 16 MiB gets its own pack.
3. Each closed pack goes through `uploads` (with a `client_ref`), `PUT`, then
   `commit`, with up to 8 packs in flight. Records are signed with the member's key.
   Uploads therefore overlap compiling.
4. A failed PUT is retried 3 times with backoff, with the same `client_ref`. After
   that, its keys report `Unavailable`. `quota_exceeded` stops further uploads for the
   session. A `put` after the mode or the quota stopped writing is reported as a
   failure, never an error.

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
| `OXABL_CACHE_MODE` | `ci`, `developer` or `read-only` |
| `OXABL_CACHE_TOKEN` | API token. It takes precedence over OIDC and login. |
| `OXABL_CACHE_IDENTITY` | The member identity string (CI) |
| `OXABL_CACHE_IDENTITY_FILE` | Path to an identity file; default `~/.config/oxabl-cache/identity` |
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

Before sending any ID token, the client decodes its payload and refuses to send it
(`OpenError`) unless `aud` is exactly `oxabl-cache:<tenant>`. A token minted for
another audience therefore never reaches the server. Gitea Actions uses a scoped
token (`OXABL_CACHE_TOKEN`). OIDC works with any issuer that publishes discovery and
JWKS documents, once the tenant registers it.

Local state (`$XDG_STATE_HOME/oxabl-cache/<cache>/`) holds the roster high-water
mark and the last verified roster. The root key lives only in memory and is zeroed on
drop.

## Command line (`oxabl-cache`)

The `oxabl-cache` binary (crate `oxabl-cache-cli`) is built on the same public API:

| Command | Purpose |
| --- | --- |
| `identity new [--plugin <name>]` | Create a member identity; print its public descriptor. |
| `init --cache c_… --label <me>` | Create the owner identity, epoch 1, roster v1; print the `cache.toml` block and the owner key for cache creation. |
| `member add <descriptor> --label <l> --role ci\|developer\|reader` | Re-wrap the root key; publish roster `v+1`. |
| `member remove <label>` | Rotate to a new epoch without the member; publish; print the new `min_roster_version`. |
| `rotate [--purge]` | New epoch for all current members; optionally purge older epochs. |
| `owner add <key>`, `owner remove <key>` | Change the roster's signer set (security.md, "Rotation and revocation"). |
| `login`, `logout` | Device-flow OIDC session for humans. |
| `status` | Mode, principal, roster, epoch, quota, last errors, and why the cache is off if it is. |
| `dict train --outputs <dir> [--publish]` | Train a 112 KiB dictionary from sampled outputs; report the ratio gain; publish as `ci`. |
| `verify --sample <n>` | Fetch and fully verify a random sample of trusted entries into a scratch directory. |
| `fetch`, `put` | Manual single-key operations for diagnostics. |
