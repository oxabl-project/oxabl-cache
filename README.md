# oxabl-cache

A zero-knowledge remote compile cache for [oxabl](https://github.com/oxabl-project/oxabl).
CI populates it; CI jobs and developer machines fetch from it and compile only what
changed. The server and its storage hold only ciphertext; clients verify every entry
against producer signatures.

Runs as a single binary on local disk, or as stateless nodes on Postgres and any
S3-compatible object store.

Design: [architecture](docs/architecture.md), [security](docs/security.md),
[protocol](docs/protocol.md), [client](docs/client.md),
[operations](docs/operations.md), [testing](docs/testing.md), [decisions](docs/adr/), [releasing](RELEASING.md).

## Desktop app

[`desktop/`](desktop/README.md) is a small window onto your organisation's caches: size, speed
and hit rate for the team and for your computer, and joining a cache in three steps without a
command line. Releases attach a Linux build.

## Deploy

One image, one Helm chart, one systemd unit and one OpenTofu module run every shape in
[operations](docs/operations.md).

| Shape | Start here |
| --- | --- |
| A team on one VM: container or systemd, SQLite and local disk | [deploy-container](docs/hosting/deploy-container.md), `deploy/container`, `deploy/systemd` |
| A cluster: Helm chart on Postgres and an S3-compatible bucket | [deploy-kubernetes](docs/hosting/deploy-kubernetes.md), `deploy/helm/oxabl-cache` |
| The hosted service: Cloudflare R2 and Hetzner Cloud | [deploy-hosted](docs/hosting/deploy-hosted.md), `deploy/tofu/hosted` |
| One Fly.io machine: SQLite on a volume and packs in Cloudflare R2 | [deploy-fly](docs/hosting/deploy-fly.md), `deploy/fly` |

The image (static, distroless, non-root, read-only root) will be published, signed, as
`ghcr.io/oxabl-project/oxabl-cache-server`. Publishing is off until the repository variable `PUBLISH_IMAGE` is
`true`, so nothing is on ghcr yet: build it with `docker build -f deploy/container/Containerfile -t oxabl-cache-server .`.
`nix develop -c deploy/check.sh` lints the chart, validates it against the Kubernetes schemas, validates and tests
the OpenTofu module, and verifies and scores the systemd units.

## Development

The devshell provides every tool the tests start as a child process (Garage, versitygw,
Postgres 18, libfaketime, cargo-fuzz), the deploy checks (Helm, kubeconform, kubectl, kind, OpenTofu, tflint)
and `rustup`, which installs the Rust toolchain pinned
in `rust-toolchain.toml`. With [direnv](https://direnv.net) the shell loads on `cd`
(`direnv allow` once); otherwise prefix commands with `nix develop -c`.

```sh
nix develop -c cargo test --workspace --locked      # every test, including the e2e suite
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c scripts/check-server-deps.sh         # the server must not depend on client-side crates
nix develop -c scripts/check-gate-fixture.sh        # the gate must reject its fixture
```

The e2e suite in `tests/e2e` starts its own Garage, versitygw and Postgres processes on random
ports, under `target/e2e-tmp/` (logs included; directories of finished runs are removed on the
next run). It never uses Docker. See [CONTRIBUTING.md](CONTRIBUTING.md) before changing code.

License: MIT.
