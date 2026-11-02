# 0005: Trust tiers by namespace, scope and signature

Status: accepted

## Context

r-code is not reproducible, so a client cannot check a hit by recompiling. Trust
must come from who produced the entry.

## Decision

- Two namespace kinds: `trusted`, written only by CI with `write:trusted` and
  signed by a `ci` member key, and `personal(user)`, written only by that user and
  signed with their own key.
- Clients accept trusted records only from `ci` keys in the current roster, and
  personal records only from their own keys.
- The server enforces the same split with scopes, so poisoning needs both a server
  bypass and a stolen `ci` key.
- `ci` identities are exposed only to protected-branch jobs; pull-request jobs read
  only.
- Rollback protection: `min_roster_version` pinned in the repository, plus a local
  high-water mark.

## Rejected

- One shared namespace with signature checks only: a server bug would let developer
  writes shadow CI entries for every reader.
- Developer-to-developer sharing: one compromised laptop would poison its peers.

## Consequences

A developer benefits from CI and from their own earlier builds only.
