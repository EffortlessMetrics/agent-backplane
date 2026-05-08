# Clippy Policy

Agent Backplane uses the Effortless Metrics Rust lint policy as an engineering
surface, not as local taste. The policy has three parts:

1. a workspace-level lint baseline in `Cargo.toml`, inherited by every
   workspace crate;
2. machine-readable policy and debt ledgers under `policy/`; and
3. `cargo xtask check-lint-policy` as the CI-adaptable gate that verifies the
   ledgers and workspace shape stay coherent.

## Workspace baseline

The active baseline denies unsafe Rust, denies panic-family collapse in
production and tests, blocks silent failure patterns, and requires suppression
governance. The baseline also turns high-signal reviewability lints on as
`warn` or `deny` so cleanup can be staged without weakening the platform rule.

The source of truth for the policy inventory is `policy/clippy-lints.toml`.
The root manifest must agree with every `status = "active"` lint entry in that
ledger.

## No test carveouts

The workspace standard is panic-free code, not merely panic-free production
code. Do not add Clippy test carveouts such as:

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-indexing-slicing-in-tests = true
allow-dbg-in-tests = true
```

Tests should return `Result` where fallible setup is required and should use
assertion helpers that produce useful errors instead of unchecked `unwrap`,
`expect`, or panic-driven setup.

## Suppression style

New suppressions should use `#[expect(..., reason = "...")]`, not broad
`#[allow(...)]` attributes. Any temporary exception that cannot be remediated in
place belongs in `policy/clippy-debt.toml` with a path, owner, reason, lint, and
expiry.

Existing legacy `#[allow]` usage is tracked as debt for follow-up remediation;
new debt must be narrower than the wave-0 buckets.

## Planned Rust upgrades

`policy/clippy-lints.toml` tracks planned Clippy flips for Rust 1.94 and 1.95
before the MSRV bump. Planned entries must not become active before the
workspace MSRV reaches their `activate_when_msrv` value.

## Repo-local overlays

`clippy.toml` is reserved for repo-specific disallowed methods, types, macros,
fields, and similar domain policy. It must not be used to weaken the shared
panic-free test posture.

## Policy command

Run:

```bash
cargo xtask check-lint-policy
```

The check verifies MSRV alignment, workspace lint inheritance, active/planned
lint consistency, Clippy test-carveout bans, and debt metadata/expiry.
