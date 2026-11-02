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

## Development

The devshell provides every tool the tests start as a child process (Garage, versitygw,
Postgres 18, libfaketime, cargo-fuzz) and `rustup`, which installs the Rust toolchain pinned
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
