# Clippy and Workspace Policy

Agent Backplane treats linting as governed infrastructure rather than a local
style preference. The workspace-level lint block in `Cargo.toml` is the active
code-shape gate, while the TOML files under `policy/` record the machine-readable
policy, staged strict-baseline lints, planned Rust upgrade flips, and any temporary debt.

## Goals

This PR starts the MSRV and policy ratchet without pretending the whole workspace
is already clean under the final strict Clippy block. Lints marked `active` in
`policy/clippy-lints.toml` must be mirrored in `Cargo.toml`; lints marked
`staged` are the target Effortless Metrics baseline for follow-up cleanup PRs.

The target baseline is intentionally shared across production code, tests,
examples, benches, and `xtask`:

- panic-free Rust by default (`panic!`, `unwrap`, `expect`, `todo!`,
  `unimplemented!`, and `unreachable!` are denied);
- no silent failure or swallowed work;
- parser/AST/string/slice safety by default;
- explicit async, synchronization, filesystem, process, numeric, and unsafe
  footgun review surfaces;
- reasoned suppressions instead of broad `allow` carveouts; and
- planned Rust 1.94 and 1.95 lint flips tracked before the MSRV moves.

## Files

- `Cargo.toml` owns the active `[workspace.lints.rust]` and
  `[workspace.lints.clippy]` block.
- `clippy.toml` is reserved for repo-specific `disallowed-*` configuration. Do
  not add test carveouts there.
- `policy/clippy-lints.toml` mirrors active lints, records staged strict-baseline
  lints, and records planned Rust 1.94/1.95 lint flips.
- `policy/clippy-debt.toml` records temporary, scoped lint debt.
- `policy/no-panic-allowlist.toml` records semantic panic-family exceptions when
  the no-panic checker is enabled.
- `policy/non-rust-allowlist.toml` records non-Rust programming/config surfaces
  that must stay in this Rust-first repository.

## Suppression style

Prefer fixing the code. When a suppression is unavoidable, use `#[expect]` with a
reason and keep the scope as narrow as possible:

```rust
#[expect(
    clippy::indexing_slicing,
    reason = "Generated table is bounds-proven by fixture validation."
)]
fn lookup_generated_table(index: usize) -> Entry {
    GENERATED_TABLE[index]
}
```

Do not use broad crate/module `#[allow]` attributes as a shortcut. Do not add
Clippy test carveouts such as `allow-unwrap-in-tests = true`,
`allow-expect-in-tests = true`, `allow-panic-in-tests = true`,
`allow-indexing-slicing-in-tests = true`, or `allow-dbg-in-tests = true`.

## Debt requirements

Temporary lint debt belongs in `policy/clippy-debt.toml`. Every debt entry must
include:

- `lint`
- `path`
- `owner`
- `reason`
- `expires`

Expired debt fails `cargo run -p xtask -- check-lint-policy`.

## Policy checks

Run the policy checks directly:

```bash
cargo run -p xtask -- check-lint-policy
cargo run -p xtask -- check-no-panic-family
cargo run -p xtask -- check-file-policy
cargo run -p xtask -- policy-report
```

The standard local loop is:

```bash
cargo fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo run -p xtask -- check-lint-policy
```

Future PRs should promote `staged` lints to `active` as cleanup lands, moving
from advisory policy reporting toward blocking Clippy cleanup without weakening
the target baseline.
