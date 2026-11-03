# 0004: Client-side encryption with keyed lookup ids

Status: accepted

## Decision

- A per-epoch cache root key, held only by members, is distributed as an age file to
  each member's X25519 recipient inside a roster signed by one or more owner keys.
  Clients accept a roster if a signer is pinned in the repository. The server accepts
  it only if a signer is registered for the cache.
- Lookup ids are `BLAKE3.keyed_hash(lookup_key, compile_key)`, so the server cannot
  confirm guesses.
- Segments and records are sealed with AES-256-GCM (aws-lc-rs) under single-use keys
  derived with BLAKE3 from a random 32-byte salt. Segments use a 64 KiB STREAM
  construction. Every object carries a key commitment. Records bind cache, epoch,
  namespace kind and lookup id as associated data.
- Compression (zstd, optional per-cache dictionary) runs before encryption, with
  Padmé padding (measured overhead 1.2%).
- Rotation is a new epoch; it costs one cold cache.

## Rejected

- XChaCha20-Poly1305: 1.7 GB/s against 5.1 GB/s for AES-256-GCM on this machine, and
  no added safety once keys are single-use.
- Convergent encryption: r-code is not deterministic, so it buys no dedupe, and it
  reveals equality.
- An OIDC-bound unwrap service: whoever runs it can read the data. Customer KMS or
  hardware-token key release (age plugin recipients) needs a later member-descriptor
  version; format version 1 carries X25519 recipients only.
- Power-of-two padding: 47% overhead.

## Consequences

The whole security stack costs under 5% of wall-clock time on a 1 Gbit/s link, and
about 19% of client CPU on a same-region 10 Gbit/s link (with the strict Ed25519
verifier, ~50 µs per record). Records carry no per-file
hashes; the zstd frame checksum guards against decompressor faults.
