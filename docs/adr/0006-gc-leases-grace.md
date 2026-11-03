# 0006: Mark and sweep with leases, grace and compaction

Status: accepted

## Decision

- Entries are deleted by retention (30 idle days by default), quota eviction or an
  epoch purge. Bytes are never deleted in the same step.
- Packs are `pending` (1-hour lease), `live` or `deleting`. A commit only names
  the caller's own `pending` pack and flips it to `live` under a row lock. Each pack
  is committed exactly once, and a repeat is answered from the recorded statuses.
  Segment ids are unique per cache, and a dead segment is never revived.
- A segment is live if and only if `dead_since IS NULL`. Dictionary segments are
  credited with no entries and stay live while the dictionary pointer or any entry
  references them.
- A pack is deleted only after it has had no live segment for a 24-hour grace, which
  exceeds the 15-minute URL lifetime by far, or when its lease expired while still
  pending.
- Packs under 50% live are compacted by copying verified segments to a new pack and
  moving locations in one transaction. Records never change.
- A weekly orphan scan removes objects without rows.
- Deleting a cache or tenant tombstones it at once. After the grace, the teardown
  removes its objects and then its rows, in batches (`gc.batch`), resumably.
- One node runs GC: on Postgres, the holder of a session-level advisory lock. Taking
  the lock raises `gc_leader.epoch`. Every GC write transaction first checks that
  epoch (`FOR SHARE` on Postgres), so a takeover waits for older in-flight GC
  transactions, and later transactions of a deposed leader fail as `Fenced` instead
  of acting.
- Quota eviction and admission use live bytes (segments that entries still name),
  so deleting entries always reaches the eviction target. Stored bytes (pending, live
  and in grace) are billed and capped at 3x the hard quota as an abuse guard.

## Rejected

- Reference counting on bytes: any lost decrement leaks and any extra one deletes live
  data.
- Synchronous deletion: an in-flight reader could lose its bytes mid-fetch.

## Consequences

Storage lags deletions by the grace period. GC runs on one elected, fenced node. A
frozen former leader's open transaction can delay a takeover until the database's
idle-in-transaction timeout ends it.
