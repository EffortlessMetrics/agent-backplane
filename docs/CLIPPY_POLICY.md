# Clippy and policy governance

Agent Backplane uses the Effortless Metrics Rust lint policy as a governed engineering surface. The goal is one workspace-wide baseline for panic-free production and tests, silent-failure prevention, suppression governance, and reviewable Rust style.

## Workspace baseline

The active lint baseline lives in the root `Cargo.toml` under `[workspace.lints.rust]` and `[workspace.lints.clippy]`. Every workspace member inherits it with:

```toml
[lints]
workspace = true
```

The workspace MSRV is Rust 1.93 and must match `policy/clippy-lints.toml`. During the first Agent Backplane rollout, `unsafe_code` is a warning rather than a deny because existing Rust 2024 environment-mutation tests and sidecar extraction code are tracked as explicit rollout debt; the next cleanup PR should remove or narrow those exceptions before promotion.

## No test carveouts

This workspace does not use Clippy test carveouts. Do not add settings such as `allow-unwrap-in-tests`, `allow-expect-in-tests`, `allow-panic-in-tests`, `allow-indexing-slicing-in-tests`, or `allow-dbg-in-tests` to `clippy.toml`.

Tests should return `Result` when setup can fail, use explicit assertion helpers, and avoid unchecked `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, and `unreachable!`.

## Suppression style

New lint suppressions must be narrow and explain themselves:

```rust
#[expect(clippy::some_lint, reason = "specific invariant reviewed in policy/debt ticket")]
```

Existing `#[allow(...)]` attributes are tracked as temporary rollout debt in `policy/clippy-debt.toml`. Do not add new broad or silent suppressions.

## Policy ledgers

- `policy/clippy-lints.toml` is the machine-readable source of truth for active lints and planned Rust 1.94/1.95 flips.
- `policy/clippy-debt.toml` records temporary exceptions with owner, reason, path, lint, and expiry.
- `policy/no-panic-allowlist.toml` reserves the semantic path + family + selector model for reviewed panic-family exceptions.
- `policy/non-rust-allowlist.toml` records reviewed non-Rust surfaces with ownership and CI coverage.

## Local checks

Run the policy gate with:

```bash
cargo run -p xtask -- check-lint-policy
```

The gate verifies MSRV alignment, workspace lint inheritance, active/planned lint consistency, lack of Clippy test carveouts, suppression debt metadata, and expiry dates.
