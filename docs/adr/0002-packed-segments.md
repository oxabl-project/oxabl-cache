# 0002: Packed segments read by range, bytes direct to the store

Status: accepted

## Context

A full build is ~10,000 artifact sets (0.9 GB compressed). The primary host is
Cloudflare R2, which charges per request ($4.50 per million writes, $0.36 per
million reads). Egress is 20 times the stored bytes.

## Decision

- Each artifact set is one independently sealed **segment** named by the BLAKE3 hash
  of its bytes. Writers append segments into immutable **packs** (target 16 MiB, cap
  32 MiB) with server-assigned random ids. Readers use ranged GETs on one presigned
  URL per pack.
- Bytes go directly between client and store through presigned URLs. In local-disk
  mode the server serves URLs of the same shape.
- A per-cache zstd dictionary recovers cross-file compression while segments stay
  independently readable.
- Records name segments by hash, never by location, so the server compacts packs
  without keys.

## Rejected

- One object per artifact set: 80,000 writes and 1.1 million reads per tenant-month,
  against ~490 and ~21,800 packed. On R2 the bucket bill is 10 times higher ($75
  against $7 at 100 tenants), and 10,000 requests per cold fetch hurt over long RTTs.
- Whole-pack compression streams: incremental fetches would download whole packs.
- Proxying bytes through API nodes: they become bandwidth-bound, and hyperscalers bill
  egress twice.

## Consequences

The server must compact packs. Compaction re-verifies every segment it copies.
