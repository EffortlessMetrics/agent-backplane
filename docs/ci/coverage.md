# Coverage

Codecov coverage is Rust execution-surface evidence for the `agent-backplane` repository.

## What it answers

> Did tests execute this scoped Rust surface?

The initial Codecov flag is `rust-core-runtime`, scoped to selected **core, protocol, runtime, policy, mapping, receipt, and backend** crates (20 crates total).

## What it does NOT answer

Codecov coverage does not answer:
- whether SDK dialect mappings are **semantically faithful**,
- whether sidecar protocol behavior is **complete**,
- whether backend integrations **behave correctly**,
- whether receipt hashing or **chain verification is complete**,
- whether daemon or **WebSocket behavior is complete**,
- whether **policy safety is proven**,
- whether **BDD coverage is adequate**,
- whether **fuzzing is sufficient**,
- whether **release readiness is proven**.

Those are separate proof lanes beyond execution-surface coverage.

## Workflow

The Coverage workflow runs on:

- **Push to main** — strict upload (fails if Codecov fails)
- **Workflow dispatch** — advisory upload
- **Pull requests labeled `coverage`, `full-ci`, or `ci:full`** — advisory upload

Coverage is skipped on PRs without labels (cheaper feedback for normal PRs).

## Durable receipts

Coverage artifacts are retained for 14 days:

- `target/tarpaulin/lcov.info` — LCOV format (Codecov upload)
- `target/tarpaulin/tarpaulin-report.html` — HTML report
- `target/tarpaulin/coverage-receipt.json` — Scoped receipt metadata
- GitHub Actions artifact: `coverage-report` — full `target/tarpaulin/` directory
- Codecov dashboard — `https://codecov.io/gh/EffortlessMetrics/agent-backplane`

## Configuration

- **codecov.yml** — Codecov status, flag, and ignore paths
- **tarpaulin.toml** — Tarpaulin output format and package scope
- **.github/workflows/coverage.yml** — GitHub Actions workflow definition

## Claim boundary

Codecov is scoped Rust execution-surface evidence only. It does not prove:

- SDK semantic fidelity
- sidecar protocol correctness
- backend behavior
- receipt integrity
- daemon/WebSocket correctness
- policy safety
- BDD adequacy
- fuzz robustness
- release readiness

## Further reading

- [tarpaulin.toml](../../tarpaulin.toml) — Coverage tool configuration
- [codecov.yml](../../codecov.yml) — Codecov policy configuration
- [.github/workflows/coverage.yml](../../.github/workflows/coverage.yml) — GitHub Actions workflow
