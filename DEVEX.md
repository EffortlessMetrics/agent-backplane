# DEVEX.md — Developer Experience Contract

Single source of truth for the edit-commit-push enforcement model.
Docs describe the machine; the machine enforces the rules.

---

## 1. Goal

The edit→commit→push loop requires **zero manual rituals**. You write code, commit, and push. Hooks and CI handle formatting, linting, and gating automatically.

## 2. Non-negotiable Invariants

| Principle | Detail |
|-----------|--------|
| One truth command | `cargo xtask gate --check` — if it passes, CI passes |
| One mutating entrypoint | `cargo xtask lint-fix` — auto-formats and best-effort clippy fixes |
| Fix is best-effort | `lint-fix` may not resolve every clippy issue; that's fine |
| Gate is strict | `gate --check` is non-mutating and fails on any violation |

## 3. xtask Contract

### Subcommands

| Subcommand | Purpose | Flags |
|------------|---------|-------|
| `setup` | One-time: sets `core.hooksPath=.githooks`, chmod +x on Unix | — |
| `schema` | Generate JSON schemas to `contracts/schemas/` | — |
| `check` | Gate + run tests + doc-tests | — |
| `coverage` | Run `cargo tarpaulin` with project config | — |
| `lint` | Check formatting + clippy (non-mutating) | — |
| `lint-fix` | Auto-format + best-effort clippy fix | `--check` (non-mutating), `--no-clippy` (skip clippy) |
| `gate` | Pre-push quality gate (no test execution) | `--check` (strict/CI-parity mode) |
| `release-check` | Verify release readiness (versions, required fields, README presence, dry-run packaging) | — |
| `docs` | Build rustdoc for all crates | `--open` |
| `list-crates` | Print all workspace crate names | — |
| `audit` | Check required Cargo.toml fields, version consistency, and unused dependencies | — |
| `stats` | Print workspace statistics (crate count, LOC, test count) | — |

### `gate --check` Steps (the truth table)

| Step | Exact command |
|------|---------------|
| Format | `cargo fmt --all -- --check` (Windows: per-package fallback on OS error 206) |
| Compile | `cargo check --workspace --all-targets --all-features` |
| Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Test compile | `cargo test --workspace --no-run` |

### `check` Steps

Runs `run_fmt(true)` (with Windows fallback), then:

| Step | Exact command |
|------|---------------|
| Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Test | `cargo test --workspace` |
| Doc-test | `cargo test --doc --workspace` |

### Windows `fmt` Fallback

`cargo fmt --all` can fail on Windows with OS error 206 (path too long). When this happens, xtask automatically falls back to per-package `cargo fmt -p <name>`, then to direct `rustfmt` invocation on individual `.rs` files. This is transparent — no user action needed.

### Justfile Aliases

| Recipe | Expands to |
|--------|------------|
| `just setup` | `cargo xtask setup` |
| `just lint-fix` | `cargo xtask lint-fix` |
| `just lint-check` | `cargo xtask lint-fix --check` |
| `just gate` | `cargo xtask gate` |
| `just gate-check` | `cargo xtask gate --check` |
| `just check` | `cargo xtask check` |
| `just test` | `cargo test --workspace` |
| `just test-compile` | `cargo test --workspace --no-run` |
| `just schema` | `cargo run -p xtask -- schema` |
| `just audit` | `cargo xtask audit` |
| `just stats` | `cargo xtask stats` |
| `just docs` | `cargo xtask docs` |
| `just docs-open` | `cargo xtask docs --open` |

## 4. Git Hooks

Installed by `cargo xtask setup` (sets `core.hooksPath=.githooks`).

### Pre-commit

1. **Skip non-Rust** — exits early if no `.rs`, `Cargo.toml`, or `Cargo.lock` files are staged.
2. **`cargo xtask lint-fix`** — auto-formats and applies best-effort clippy fixes.
3. **Re-stage** — re-adds the originally-staged files with any formatting corrections.
4. **Typos (optional)** — runs `typos --diff` if the `typos` CLI is installed.

The pre-commit hook does **not** run the gate. This is intentional: the gate is expensive and runs at push time only.

### Pre-push

Runs `cargo xtask gate --check`. Push is blocked on failure.

### Emergency Bypass

```bash
git commit --no-verify   # skip pre-commit
git push --no-verify     # skip pre-push
```

Use sparingly. CI will still catch failures.

## 5. CI Policy

### Truth Lane

The `check` job runs `cargo xtask gate --check` — the same command as the pre-push hook. This is the single required check. If your push succeeds locally, CI will pass.

### Additional Lanes

| Job | What it does |
|-----|-------------|
| `test` | `cargo test --workspace --all-features` on ubuntu-latest + windows-latest |
| `doc` | `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS="-D warnings"` |
| `coverage` | `cargo tarpaulin` → Codecov upload |
| `deny` | `cargo deny check` — license and advisory audit |
| `schema` | `cargo run -p xtask -- schema` + verify non-empty output |
| `mutants.yml` | Manual-dispatch mutation testing via `cargo-mutants` |
| `release.yml` | Tag-triggered (`v*`) automated release pipeline |

### Local/CI Parity

CI does **not** set global `RUSTFLAGS`. The gate enforces `-D warnings` through its clippy step. This means `cargo xtask gate --check` produces identical results locally and in CI.

## 6. Documentation Policy

Docs describe what the machine does — never tell developers to manually run checks.

| Do | Don't |
|----|-------|
| "The pre-commit hook auto-formats code" | "Before committing, run `cargo fmt`" |
| "Push is blocked if the gate fails" | "Ensure all checks pass before pushing" |

If a doc says "run X before Y", that's a bug in the automation, not missing documentation.

## 7. Agent Contract

Rules for AI agents (Claude Code, Copilot, Codex, etc.) operating in this repo:

1. **Never `--no-verify`** unless the human operator explicitly instructs you to.
2. **Include hook changes** — if you modify `.githooks/` or xtask, test that hooks still work.
3. **CI parity** — the command to verify everything is `cargo xtask gate --check`. Run it.

### Generator Guardrails

When generating Rust code, avoid these patterns that trigger clippy warnings or produce churn:

1. **No identity maps** — `map_err(|e| e)` is a no-op. Remove it.
2. **Const assertions** — compile-time invariants: `const { assert!(...) }`, not runtime `assert!`.
3. **Prefer `if let`** — don't `match` with one meaningful arm and a wildcard.

## 8. Windows Notes

- `chmod` is a no-op on Windows — `cargo xtask setup` handles this with `#[cfg(unix)]`.
- `cargo fmt --all` may hit OS error 206 (path too long). The `run_fmt()` function automatically falls back to per-package formatting. See [Section 3](#3-xtask-contract).
- CI runs the test matrix on both `ubuntu-latest` and `windows-latest`.

## 9. Porting to Other Repos

Template checklist for rolling out this enforcement model to another repository:

- [ ] Copy `xtask/` crate (adjust workspace-specific schema generation)
- [ ] Copy `.githooks/pre-commit` and `.githooks/pre-push`
- [ ] Copy `Justfile` (adjust recipes as needed)
- [ ] Run `cargo xtask setup` in CI setup step
- [ ] CI truth lane: `cargo xtask gate --check`
- [ ] Verify Windows `fmt` fallback works if repo has deep paths
- [ ] Add `DEVEX.md` and update repo docs to point at it
