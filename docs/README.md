# Documentation

Start with the path for your role. Each one ends with a working cache and a check that proves it.

## Getting started

| Page | What it is for |
| --- | --- |
| [start/operator.md](start/operator.md) | Install and host the server, make the first tenant and its administrator token. |
| [start/owner.md](start/owner.md) | Make and protect the owner identity, create the cache and its first roster, pin it in the repository, add an offline second owner key. |
| [start/ci.md](start/ci.md) | Add a CI system with one command: the writing member versus the read-only one, which secrets go where, a first fetch. Also the steps for an owner and a tenant administrator who are different people. |
| [start/developer.md](start/developer.md) | Join a cache from the desktop app or the command line, sign in, make a first fetch. |

## Using and running a cache

| Page | What it is for |
| --- | --- |
| [data-handling.md](data-handling.md) | What the server and its operator can and cannot see, who holds which key, and what the owner key controls. |
| [operations.md](operations.md) | Configuration, backup and a restore drill, rotating members, epochs and tokens, losing an owner key, observability, tenant administration, quotas, and what to do when something fails. |
| [hosting/deploy-container.md](hosting/deploy-container.md) | One VM: the container image or a systemd unit, with SQLite and local disk, plus TLS and a two-node variant. |
| [hosting/deploy-kubernetes.md](hosting/deploy-kubernetes.md) | The Helm chart, on Postgres and an S3-compatible bucket, or one node on a volume. |
| [hosting/deploy-hosted.md](hosting/deploy-hosted.md) | The hosted service on Cloudflare R2 and Hetzner Cloud, built with OpenTofu. |
| [hosting/deploy-fly.md](hosting/deploy-fly.md) | One Fly.io machine, with SQLite on a volume and packs in Cloudflare R2. |
| [../desktop/README.md](../desktop/README.md) | The desktop app: cache size, speed and hit rate, and joining a cache without a command line. |

## Design

| Page | What it is for |
| --- | --- |
| [architecture.md](architecture.md) | The domain model, storage, metadata, read and write paths, garbage collection and scaling. |
| [security.md](security.md) | The threat model, the key hierarchy, encryption, trust tiers and authentication. |
| [protocol.md](protocol.md) | The HTTP API and the byte formats. |
| [client.md](client.md) | The client library and the `oxabl-cache` command line. |
| [testing.md](testing.md) | How the tests run the product, and what each scenario protects. |
| [hosting/provider-analysis.md](hosting/provider-analysis.md) | The cost model and the numbers behind the choice of hosting providers. |

## Decisions

| Page | What it is for |
| --- | --- |
| [adr/0001-hexagonal-core.md](adr/0001-hexagonal-core.md) | Why the core is small and talks to its adapters through ports. |
| [adr/0002-packed-segments.md](adr/0002-packed-segments.md) | Why entries are packed in segments that clients read by range, directly from the store. |
| [adr/0003-metadata-sqlite-and-postgres.md](adr/0003-metadata-sqlite-and-postgres.md) | Why one node uses SQLite and many nodes use Postgres. |
| [adr/0004-crypto-scheme.md](adr/0004-crypto-scheme.md) | Why clients encrypt and use keyed lookup ids. |
| [adr/0005-trust-tiers.md](adr/0005-trust-tiers.md) | Why trusted and personal namespaces differ by scope and signature. |
| [adr/0006-gc-leases-grace.md](adr/0006-gc-leases-grace.md) | Why garbage collection uses leases, a grace period and compaction. |
| [adr/0007-no-branch-manifests.md](adr/0007-no-branch-manifests.md) | Why the cache keeps no per-branch manifests. |

The repository root also holds [CONTRIBUTING.md](../CONTRIBUTING.md) (how to change the code) and
[RELEASING.md](../RELEASING.md) (how releases are made).
