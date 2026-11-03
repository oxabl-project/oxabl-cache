# 0003: SQLite for a single node, Postgres for many

Status: accepted

## Context

The product ships as a self-contained single binary and also as stateless API nodes
behind a shared store.

## Decision

Two `MetadataStore` adapters share one schema in a common SQL subset: SQLite (WAL,
`synchronous=FULL`) and Postgres. The database arbitrates first-write-wins with
`INSERT … ON CONFLICT DO NOTHING`; the object store needs no conditional writes.
On Postgres, writers take scoped advisory transaction locks (global shared, then
tenant, then cache) under READ COMMITTED. The port's closure cannot be re-run, so
SERIALIZABLE with retry is not an option. GC runs on the node that holds a
session-level advisory lock.
`oxabl-cache-server migrate` moves a single node to Postgres and S3, verifying every row
count and segment hash.

## Rejected

- Postgres only: the single binary would need a database server.
- SQLite only: API nodes cannot share it.
- redb: no SQL parity, so a second query layer would be needed.

## Consequences

Every scenario test runs on both. Schema types stay portable: bytes, text, and
integer seconds.
