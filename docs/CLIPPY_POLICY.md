# Clippy policy

Agent Backplane is adopting the Effortless Metrics Rust lint policy as an engineering surface rather than a one-off `Cargo.toml` preference file.

## Goals

- Ratchet the workspace to the shared Rust MSRV tracked in `policy/clippy-lints.toml`.
- Move toward panic-free production and test code.
- Prevent silent failure patterns such as swallowed `Result`s and unreviewed suppressions.
- Keep AST, parser, UTF-8, indexing, async, filesystem, and API footguns visible.
- Track temporary exceptions as explicit debt with owners, reasons, paths, lints, and expiry dates.

## Current rollout posture

This repository is in the first infrastructure PR for the policy rollout. The root manifest enables the initial inherited workspace lint surface, while `policy/clippy-lints.toml` records the target panic-free baseline, debt-tracked lints, and planned Rust 1.94/1.95 flips. Broad entries in `policy/clippy-debt.toml` are temporary bootstrap debt and should be replaced with narrower receipts as follow-up PRs remove call sites.

## Suppression style

Use narrow `#[expect(..., reason = "...")]` suppressions when a local exception is truly required. Do not use broad `#[allow]` suppressions, crate-wide carveouts, or Clippy test carveouts.

Allowed pattern:

```rust
#[expect(clippy::indexing_slicing, reason = "validated table index from generated parser bounds")]
fn generated_lookup(table: &[u8], index: usize) -> u8 {
    table[index]
}
```

Avoid:

```rust
#[allow(clippy::indexing_slicing)]
fn generated_lookup(table: &[u8], index: usize) -> u8 {
    table[index]
}
```

## No test carveouts

Do not add these settings to `clippy.toml`:

- `allow-unwrap-in-tests = true`
- `allow-expect-in-tests = true`
- `allow-panic-in-tests = true`
- `allow-indexing-slicing-in-tests = true`
- `allow-dbg-in-tests = true`

Tests should migrate toward `Result`-returning helpers and explicit assertions that preserve failure context without unchecked panic-family calls.

## Policy checks

Run:

```bash
cargo run -p xtask -- check-lint-policy
```

The gate verifies lint policy schema fields, MSRV alignment intent, inherited workspace lint settings, absence of Clippy test carveouts, planned Rust 1.94/1.95 flips, and non-expired debt entries.
