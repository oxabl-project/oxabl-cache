# Client library

`oxcache-client` is the only crate oxabl links. It owns everything between a
plaintext `CompileKey` and a verified file at its final path: lookup ids,
compression, encryption, signing, packing, batching, transfer, verification and
atomic installation. oxabl owns what goes into a compile key; the cache never
defines it.

## The seam with oxabl

- oxabl computes a 32-byte `CompileKey` per compile unit (BLAKE3 over canonical,
  root-relative compile inputs).
- An artifact set is a list of (root-relative output path, bytes): the `.r`, and
  optionally the XREF and debug-list files.
- Any cache failure is a miss or "not stored". The cache never fails a build;
  oxabl compiles what it could not fetch.

## Public API

```rust
pub struct CompileKey(pub [u8; 32]);

/// Root-relative, normalised output path: UTF-8, `/`-separated, no empty, `.` or `..`
/// components, no leading `/`, no `\`, no NUL, no drive prefix, at most 1,024 bytes.
pub struct OutputPath(/* private */);
impl OutputPath {
    pub fn new(path: &str) -> Result<OutputPath, PathError>;
    pub fn as_str(&self) -> &str;
}

pub struct Artifact { pub path: OutputPath, pub bytes: bytes::Bytes }

/// A handle to one cache; cheap to clone and safe to share across tasks.
#[derive(Clone)]
pub struct Cache { /* private */ }

impl Cache {
    /// Reads config, authenticates, fetches and verifies the roster, unwraps the root key.
    pub async fn open(config: Config) -> Result<Cache, OpenError>;

    /// Plan mode: presence only, no verification. A hit predicts; `fetch` decides.
    pub async fn probe(&self, keys: &[CompileKey]) -> Result<Vec<Presence>, CacheError>;

    /// Fetches, verifies and installs; one outcome per request, in order.
    pub async fn fetch(&self, requests: Vec<FetchRequest>) -> Vec<FetchOutcome>;

    /// Starts a batched upload session. Fails if the mode does not write.
    pub fn uploader(&self) -> Result<Uploader, CacheError>;

    pub fn mode(&self) -> Mode;
    pub fn stats(&self) -> Stats;
}

pub enum Presence { Hit, Miss }

pub struct FetchRequest {
    pub key: CompileKey,
    /// Output root; every file is installed at `out_root/path`.
    pub out_root: std::path::PathBuf,
    /// If set, the entry's path set must equal this set exactly, or the result is a miss.
    pub expect: Option<Vec<OutputPath>>,
}

pub enum FetchOutcome {
    Installed { key: CompileKey, files: Vec<InstalledFile> },
    Miss { key: CompileKey, reason: MissReason },
}

pub struct InstalledFile { pub path: OutputPath, pub size: u64, pub blake3: [u8; 32] }

pub enum MissReason {
    NotFound,
    Untrusted,
    Corrupt,
    PathMismatch,
    Unavailable,
    Disabled,
    Install(std::io::ErrorKind),
}

pub struct Uploader { /* private */ }
impl Uploader {
    /// Queues one artifact set; waits only for back-pressure (256 MiB in flight).
    pub async fn put(&self, key: CompileKey, artifacts: Vec<Artifact>) -> Result<(), StoreError>;
    /// Seals and uploads the rest, commits every pack, and reports.
    pub async fn finish(self) -> StoreReport;
}

pub struct StoreReport {
    pub committed: usize,
    pub existed: usize,
    pub failed: Vec<(CompileKey, StoreFailure)>,
    pub bytes_uploaded: u64,
}
pub enum StoreFailure { QuotaExceeded, Unavailable, Rejected, TooLarge }
```

`OpenError` covers invalid config, unreachable server, authentication failure,
roster failures (a bad signature, an unpinned owner, rollback below the high-water
mark or `min_roster_version`), and an identity missing from the roster. oxabl reports
it and builds without the cache. A configuration error is shown as an error, a
transient one as a warning.

## Fetch pipeline

1. Derive lookup ids (0.08 µs each) and send `lookup` in chunks of 10,000 with records
   and URLs.
2. Open each record. Check the commitment and AEAD, the signature transcript, that
   the signer is accepted for the namespace (see "Trust policy"), the roster version,
   the path rules, `expect`, and that `plain_len` is within the configured limit. A
   failure goes on to the personal hit, if any, or else gives a miss.
3. Group segments by pack and coalesce ranges with gaps of 1 MiB or less, splitting
   reads above 8 MiB. Run them with 32 concurrent connections (configurable up to
   128) and at most 256 MiB in flight.
4. Per segment, on a CPU worker pool: check `BLAKE3(bytes)` against the signed
   segment id (on a mismatch, send `reports` in the background and miss); check the
   header; decrypt the chunks in order; strip the padding; decompress with a window
   limit of 128 MiB and an output cap of `plain_len`; split into files by the signed
   sizes; check each file's BLAKE3.
5. Install (below). Expired URLs are refreshed once through `urls`.

### Install

The final path never holds a partial or unverified file.

- `out_root` is opened as a directory handle. Every component is created and walked
  beneath it with `openat` and `O_NOFOLLOW` (`cap-std`). Symlinks, `..` and
  absolute paths cannot escape the root, whatever a record says.
- Each file is written to `.<name>.oxc-<random>.tmp` in its target directory, then
  fsynced. Only after **every** file of the set has passed every check is each temp
  file renamed over its final path. Each parent directory is fsynced once.
- On any failure, temp files are removed and the outcome is a miss. Files already
  at final paths are untouched.
- Renames are atomic per file. If the process dies between two renames of one set,
  each final file is either the old or the new verified version. oxabl records a unit
  as fetched only on `Installed`, so a torn set is recompiled or refetched on the next
  run.

## Upload pipeline

1. `put` validates the paths and the plaintext size, derives the lookup id, and
   queues the set.
2. Workers compress (zstd level 3 by default, with the cache dictionary if one is
   published), pad, seal, and append the segment to the open pack. A pack is closed
   at 16 MiB or 2,000 segments, and a segment over 16 MiB gets its own pack.
3. Each closed pack goes through `uploads` (with a `client_ref`), `PUT`, then
   `commit`, with up to 8 packs in flight. Records are signed with the member's key
   and name the pack's segments.
4. A failed PUT is retried 3 times with backoff, with the same `client_ref`. After
   that, its keys go to `failed` as `Unavailable`. `quota_exceeded` stops further
   uploads for this session.

## Trust policy

| Mode | Reads | Accepts signers | Writes | Credentials |
| --- | --- | --- | --- | --- |
| `ci` | trusted | `ci` members | trusted, signed with its `ci` key | OIDC or a token with `write:trusted` |
| `ci-readonly` | trusted | `ci` members | none | OIDC or a token with `read` |
| `dev` (default) | trusted, then own personal | `ci` members; own keys for personal | own personal | `oxabl-cache login` session or a token |
| `dev-readonly` | trusted, then own personal | as `dev` | none | as `dev` |
| `off` | none | none | none | none |

If the environment variable `CI` is set and no mode is configured, the mode is
`ci-readonly`. Writing to trusted always needs an explicit `ci`.

## Configuration

`.oxabl/cache.toml` is committed in the customer's repository and holds no secrets:

```toml
server = "https://cache.example.com"
cache = "c_6v2k9q3m8x1r4t7w0y5z2b8n4d"
owner_keys = ["ed25519:Jx1…"]
min_roster_version = 12

[client]
concurrency = 32
zstd_level = 3
max_plain_bytes = 67108864
```

Secrets and per-machine settings come from the environment or the user's config
directory, never from the repository:

| Variable | Meaning |
| --- | --- |
| `OXABL_CACHE_MODE` | `ci`, `ci-readonly`, `dev`, `dev-readonly` or `off` |
| `OXABL_CACHE_TOKEN` | API token (alternative to OIDC or login) |
| `OXABL_CACHE_OIDC` | `github`, `gitlab` or `gitea`. GitHub and Gitea Actions use the job's ID-token request endpoint; GitLab reads `OXABL_CACHE_ID_TOKEN` from the job's `id_tokens` |
| `OXABL_CACHE_IDENTITY` | The member identity string (CI) |
| `OXABL_CACHE_IDENTITY_FILE` | Path to an identity file; default `~/.config/oxabl-cache/identity` |
| `OXABL_CACHE_SERVER`, `OXABL_CACHE_CACHE` | Overrides for the repository file |

Local state (`$XDG_STATE_HOME/oxabl-cache/<cache>/`) holds the roster high-water
mark and the last verified roster. The root key lives only in memory and is zeroed on
drop.

## Command line (`oxabl-cache`)

Built on the same public API:

| Command | Purpose |
| --- | --- |
| `identity new [--plugin <name>]` | Create a member identity; print its public descriptor. |
| `init --cache c_… --label <me>` | Create the owner identity, epoch 1, roster v1; print the `cache.toml` block. |
| `member add <descriptor> --label <l> --role ci\|developer\|reader` | Re-wrap the root key; publish roster `v+1`. |
| `member remove <label>` | Rotate to a new epoch without the member; publish; print the new `min_roster_version`. |
| `rotate [--purge]` | New epoch for all current members; optionally purge older epochs. |
| `login`, `logout` | Device-flow OIDC session for humans. |
| `status` | Mode, principal, roster, epoch, quota, last errors. |
| `dict train --outputs <dir> [--publish]` | Train a 112 KiB dictionary from sampled outputs; report the ratio gain; publish as `ci`. |
| `verify --sample <n>` | Fetch and fully verify a random sample of trusted entries into a scratch directory. |
| `fetch`, `put` | Manual single-key operations for diagnostics. |
