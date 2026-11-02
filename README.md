# oxabl-cache

A zero-knowledge remote compile cache for [oxabl](https://github.com/oxabl-project/oxabl).
CI populates it; CI jobs and developer machines fetch from it and compile only what
changed. The server and its storage hold only ciphertext; clients verify every entry
against producer signatures.

Runs as a single binary on local disk, or as stateless nodes on Postgres and any
S3-compatible object store.

Design: [architecture](docs/architecture.md), [security](docs/security.md),
[protocol](docs/protocol.md), [client](docs/client.md),
[operations](docs/operations.md), [testing](docs/testing.md), [decisions](docs/adr/).

License: MIT.
