# Data handling

This page says what the server, its operator and its storage provider can see, what they cannot see,
and what the owner key controls. It summarises security.md and architecture.md. Read those for the
design and its reasons.

The short version: the server holds ciphertext and account data. It sees how much, how big and when. It
never sees source, r-code, paths or keys. The people in your cache can read everything in it, and the
operator cannot.

## What the server and its operator can see

| Item | What they see |
| --- | --- |
| Counts | The number of entries per cache, which is about the number of compile units. Requests, lookups, hits and committed entries per tenant and cache, in the usage ledger. |
| Sizes | The size of each packed artifact set, rounded up to a Padmé size class (about 1.2% extra bytes, so exact sizes stay hidden). Pack sizes. Quota and stored bytes per tenant and cache. |
| Timing | When each entry was written and read, how many, and by which principal (token, session or user). The request log shows the same. |
| Access days | One day number per entry, set when a lookup hits it. Retention and eviction use it. |
| Grouping | Which entries one upload wrote together. |
| Labels and names | Tenant and cache ids and display names, user labels and emails, token names, the member labels, roles and public keys in the roster. You choose every label, and a label need not be a name. |
| Epoch and versions | The epoch number, the roster version, and that a compression dictionary exists, with its id and size. |
| Sealed telemetry | That a member sent a report, when it arrived, its epoch and its size. Every report is 96 bytes. The server keeps the newest 4,096 reports of a cache, none older than 30 days. |

The operator's logs hold request ids, the route, tenant and cache ids, the principal, status,
durations, counts and sizes. They never hold tokens, session ids, JWTs, OIDC claims, URL query strings, lookup ids, segment ids or
record bytes (operations.md, "Observability").

An OIDC sign-in shows the server the token's claims (the repository and the ref, for example) for the
moment of the exchange. The server does not store or log them. A trust policy keeps only keyed hashes
of the values it expects.

## What they cannot see

| Item | Why not |
| --- | --- |
| Source, file names, paths, compile inputs, branch names | The client never sends them. Paths exist only inside sealed records. |
| r-code, XREF and debug lists | They sit in AES-256-GCM ciphertext. |
| The compile key | The server sees a lookup id, a keyed hash. Without the cache's key, nobody can compute the id of a guessed file, so nobody can test whether a file is in the cache. |
| The cache keys and member keys | They exist only on member computers and in CI secrets. The roster's keyring is age-encrypted to the members. |
| What a telemetry report says (keys asked for, hits, misses, failures, bytes, duration, round trip, mode) | The report is sealed under a key the server does not hold. |

A full leak of the database and the buckets gives ciphertext, entry counts, size classes, grouping,
access days and account data. It gives no source, no paths and no r-code.

## What a lying or hostile server can do

It can refuse service, answer slowly, drop entries (the client sees a miss), serve an older valid entry
for the same key, and withhold a newer roster. It cannot read an entry, forge one, change one or make
a client install a wrong or partial file. The client checks every entry against the signature of a
`ci` member and the roster pinned in your repository. After you raise `min_roster_version`, a
client refuses an old roster and builds without the cache.

## Who holds what

| Holder | Holds | Can |
| --- | --- | --- |
| Operator | Operator tokens, the database, the buckets, the server's URL-signing and claim-hash keys | Run, suspend and delete tenants. Read the list above. Never decrypt. |
| Tenant administrator | A `tenant:admin` token | Make caches, tokens, users and grants. Register owner keys. Cannot decrypt. Cannot change who a client trusts, because the pins live in your repository. |
| Owner | The owner signing key, and the cache root key | Sign rosters: add and remove members, rotate the epoch. Read every entry. |
| `ci` member | The cache root key and a signing key | Read every entry. Write the trusted namespace. |
| `developer` member | The cache root key and a signing key | Read every entry. Write their own personal namespace. |
| `reader` member | The cache root key | Read every entry. Write nothing. |

Every member holds the cache root key of the current epoch. A `reader` in a pull request job can
therefore decrypt what the cache stores. That job has already checked out the source those artifacts
came from, so it learns nothing new (security.md, "Integrity without trusting the server").

A token without an identity gives ciphertext and the power to flood the server, and nothing else. An
identity without a token gives nothing, unless the bucket leaks too. A token's `read` scope lets its
holder list up to 1,000 lookup ids per call and download the ciphertext behind them. That is the same
ciphertext a bucket leak gives. The tenant's egress quota and rate limit apply.

## What the owner key controls

The owner key signs every roster. A roster lists the members with their roles and the current epoch,
and it carries the keyring. A client opens a cache only when a key pinned in `.oxabl/cache.toml`
signed the roster. So the owner key decides:

- who is a member, and which role each member has;
- when the epoch changes, which makes a new cache root key and cuts a removed member off;
- who else may sign rosters (`owner add`, `owner remove`).

The server checks the same signature against the owner keys the tenant administrator registered, so a
stolen `cache:admin` token cannot publish an unsigned roster or a huge epoch. The owner key does not
sign cache entries: `ci` members do. The repository holds the pins. Whoever can change
`.oxabl/cache.toml` in a branch can already change what that branch compiles, so the pins add no new
trust.

If you lose every owner key, the roster can never change again. Keep two keys, one offline
(owner.md, step 4). If an owner key leaks, remove it (operations.md, "Losing an owner key").

## Retention and deletion

- An entry nobody reads for the cache's retention period (30 days by default) is deleted, and so are
  the oldest entries when the cache goes over its quota.
- Deleting a tenant or a cache stops access at once: the tenant's tokens get 401, and a deleted
  cache answers 404. A garbage collection job removes the bytes and rows after the grace period.
- A removed member keeps the root key of every epoch they held. They can read old ciphertext only if
  they also get the bucket. `rotate --purge` and `member remove --purge` mark every entry of an older
  epoch dead.
- Nobody backs up customer keys on the customer's behalf. The operator's backups hold ciphertext and
  account data only (operations.md, "Backup and restore").
