# Releasing

Three library crates are published to crates.io so that oxabl can depend on them as normal semver dependencies:

| Crate | Purpose |
| --- | --- |
| `oxabl-cache-format` | Byte formats, no keys |
| `oxabl-cache-crypto` | Client-side cryptography |
| `oxabl-cache-client` | Client library used by oxabl |

Every other crate (server, admin, member CLI, stores, metadata, http, oidc, core and the test crates) is `publish = false`. The server image and the binaries are not part of this flow.

release-please's `cargo-workspace` plugin reads every workspace member, so:

- each crate states a literal `version` (the plugin cannot read `version.workspace = true`);
- the unpublished crates are in the config with `skip-github-release` and `skip-changelog`: their versions move with their dependencies, but they get no tag, release or changelog;
- the dependency graph has no cycle, dev-dependencies included. A suite that needs a crate and the crates built on it lives in its own test crate, as `tests/core-behaviour` does.

`scripts/check-release-graph.sh` checks all three on every PR (CI job `dependency-gate`).

## Versions

The three crates version independently, starting at `0.1.0`. [release-please](https://github.com/googleapis/release-please) reads conventional commits on `main` and keeps a release PR open with the version bumps and changelogs (`feat` bumps the minor, `fix` the patch, while the major is 0). Dependents of a bumped crate are bumped with it. Path dependencies between the published crates carry `version` as well as `path`; release-please keeps both in step.

Merging the release PR tags `<crate>-v<version>` and starts the build and publish jobs of `.github/workflows/release.yml`:

1. build and test the workspace with `--locked --release`;
2. `scripts/publish-workspace.sh` publishes every workspace crate that is not `publish = false`, in dependency order, skipping versions crates.io already has. A crates.io rate limit is waited out for up to 30 minutes; re-running the workflow is always safe.

The first release, `0.1.0`, is already in the manifest: run the `Release` workflow manually (`workflow_dispatch`) to publish it.

## Secrets

| Secret | Used for |
| --- | --- |
| `RELEASE_TOKEN` | release-please opens the release PR with it, so the PR triggers CI (the default `GITHUB_TOKEN` does not) |
| `CARGO_REGISTRY_TOKEN` | crates.io token that may publish the three crates |

## Checking a release locally

```sh
cargo package -p oxabl-cache-format -p oxabl-cache-crypto -p oxabl-cache-client --locked
cargo publish --dry-run -p oxabl-cache-format -p oxabl-cache-crypto -p oxabl-cache-client --locked
cargo package -p oxabl-cache-client --list
```

A package holds `src/`, the manifest and a README only; tests and examples are excluded.
