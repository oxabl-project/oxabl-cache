# 0001: Hexagonal core with a small domain

Status: accepted

## Context

The server runs on SQLite or Postgres and on local disk or any S3-compatible store,
with identical behaviour, and is maintained by a small team.

## Decision

One core crate (`oxabl-cache-core`) holds the domain types, the use cases and the port
traits (`MetadataStore`, `ObjectStore`, `IdentityVerifier`, `Clock`). Every I/O
concern is an adapter crate that depends on the core. The only event-like piece is
an append-only usage ledger. The client-side crypto lives in crates the server never
links.

## Rejected

- Event sourcing: entries are already immutable inserts, so projections and replay
  would protect no invariant.
- A plain layered service: with four storage adapters, SQL and S3 details would leak
  into the business rules.
- Full DDD tactical patterns: the domain has five invariants; plain structs guard
  them.

## Consequences

One black-box suite runs against every adapter combination. A new store or database
is a new crate plus a passing suite.
