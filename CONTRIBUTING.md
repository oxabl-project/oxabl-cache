# Contributing

## Code

- **Comments.** Doc comments only, and brief: say what an item is or guarantees. No comments
  that narrate code, and none about history or past behaviour. Write forward-looking code.
  The exception is a genuinely large, complex function, which may carry one explanatory comment.
- **Files stay small and scoped to their names.** Split by responsibility; a module holds what
  its name says.
- **Safety.** `unsafe_code` is forbidden workspace-wide. A crate that truly needs it opts out
  in its own `Cargo.toml` and confines it to one named module.
- **Dependencies.** Versions live in `[workspace.dependencies]`. The crate graph follows the
  diagram in `docs/architecture.md`; `scripts/check-server-deps.sh` enforces the part that
  keeps the server unable to read what it stores.

## Tests

- **No unit tests.** `#[cfg(test)]` modules do not belong in product crates. Tests are
  black-box: they drive the binaries, the HTTP API or the public client API and know nothing
  of the code. Integration tests cover large behaviours end to end, never single functions.
  Fuzzing is welcome.
- **Break it once.** After writing a test, guard or gate, break the thing it protects and
  confirm the test goes red and names the failing assertion. Record the break in the pull
  request.
- **Never weaken a check to make it pass.** No new lint ignores, skipped or deleted tests, or
  loosened assertions.
- **Environment, not mocks.** Tests fake the environment (network, clock, storage servers),
  never the program under test. No Docker: dependencies are child processes started by
  `tests/e2e`, in their own process group and killed with the test.

## Before a pull request

```sh
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets --locked -- -D warnings
nix develop -c cargo test --workspace --locked
nix develop -c scripts/check-server-deps.sh
```
