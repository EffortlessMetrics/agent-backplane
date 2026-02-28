# Contributing to Agent Backplane

Thank you for your interest in contributing to Agent Backplane (ABP)! This guide
will help you get started.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Code Standards](#code-standards)
- [Testing](#testing)
- [Architecture](#architecture)
- [Commit Messages](#commit-messages)
- [Pull Request Process](#pull-request-process)
- [Release Process](#release-process)

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md).
By participating, you are expected to uphold this code.

## Getting Started

### Prerequisites

- **Rust nightly** — the workspace uses edition 2024
  ```bash
  rustup toolchain install nightly
  rustup default nightly
  rustup component add rustfmt clippy
  ```
- **Node.js** — required for sidecar hosts in `hosts/`
- **Python** (optional) — for the Python sidecar host
- **cargo-insta** (optional) — for snapshot test review: `cargo install cargo-insta`
- **cargo-fuzz** (optional) — for fuzz testing: `cargo install cargo-fuzz`

### Clone and Build

```bash
git clone https://github.com/EffortlessMetrics/agent-backplane.git
cd agent-backplane
cargo build
```

### Verify Your Setup

```bash
# Run the full test suite
cargo test --workspace

# Run with the mock backend (no external dependencies)
cargo run -p abp-cli -- run --task "say hello" --backend mock
```

## Development Workflow

### Branch Strategy

- **`main`** is the stable branch. All PRs target `main`.
- Create feature branches from `main` using a descriptive name:
  ```
  feat/add-openai-adapter
  fix/receipt-hash-collision
  docs/update-sidecar-protocol
  ```

### Workflow

1. Fork the repository and clone your fork.
2. Create a feature branch from `main`.
3. Make your changes in small, focused commits.
4. Ensure all checks pass locally (see [Code Standards](#code-standards)).
5. Push your branch and open a pull request against `main`.

## Code Standards

### Formatting

All code must be formatted with `rustfmt`:

```bash
cargo fmt --check
```

### Linting

Clippy must pass with no warnings:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

### Unsafe Code

Unsafe code is **not permitted** in this project. All crates use `#![deny(unsafe_code)]`
or enforce this through CI. If you believe an exception is warranted, open an issue
to discuss before submitting a PR.

### Documentation

- All public APIs must have rustdoc documentation.
- Documentation must build without warnings:
  ```bash
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
  ```

### Dependency Auditing

We use [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) for license
and advisory auditing. Configuration is in `deny.toml`.

### Schema Generation

If you modify contract types in `abp-core`, regenerate the JSON schemas:

```bash
cargo run -p xtask -- schema
```

Commit the updated schema files in `contracts/schemas/`.

## Testing

ABP has a comprehensive, multi-layered test strategy. See [`docs/testing.md`](docs/testing.md)
for the full guide.

### Test Categories

| Category | Command | Description |
|----------|---------|-------------|
| **Unit** | `cargo test -p <crate>` | Per-crate module-level tests |
| **Integration** | `cargo test --workspace` | Cross-crate integration tests |
| **Snapshot** | `cargo insta review` | JSON serialization stability ([insta](https://insta.rs)) |
| **Property** | `cargo test -p abp-core proptest` | Randomized input via [proptest](https://proptest-rs.github.io/proptest/) |
| **Fuzz** | `cd fuzz && cargo +nightly fuzz run <target>` | Envelope/receipt/work-order parsing |
| **Benchmarks** | `cargo bench --workspace` | [Criterion](https://bheisler.github.io/criterion.rs/) micro-benchmarks |
| **Conformance** | `cd tests/conformance && node runner.js` | End-to-end sidecar protocol conformance |
| **Doc tests** | `cargo test --doc --workspace` | In-doc examples |

### Running All Tests

```bash
cargo test --workspace
```

### Writing Tests

- Add unit tests in the same file as the code under test using `#[cfg(test)]`.
- Add integration tests in the crate's `tests/` directory.
- For new contract types, add property-based tests with `proptest`.
- For serialization changes, add or update snapshot tests.

## Architecture

ABP is organized as a Cargo workspace with a strict crate dependency hierarchy:

```
abp-glob ──────────┐
                    ├── abp-policy ──────────┐
abp-core ──────────┤                         │
  │                └── abp-workspace ────────┤
  │                                          │
abp-protocol ─── abp-host ─── abp-integrations ─── abp-runtime ─── abp-cli
```

**The contract is the product.** `abp-core` defines the canonical types and is the
only crate most consumers need to depend on.

For the full architecture documentation, see [`docs/architecture.md`](docs/architecture.md).

### Key Design Principles

- **Deterministic serialization** — `BTreeMap` is used throughout for canonical JSON hashing.
- **Serde conventions** — All enums use `#[serde(rename_all = "snake_case")]`. Protocol
  envelopes use `#[serde(tag = "t")]` (not `type`).
- **Receipt integrity** — `receipt_hash()` sets `receipt_sha256` to `null` before hashing
  to prevent self-referential hashes.

## Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>(<scope>): <short summary>

<optional body>

<optional footer(s)>
```

### Types

| Type | Description |
|------|-------------|
| `feat` | A new feature |
| `fix` | A bug fix |
| `docs` | Documentation only changes |
| `style` | Formatting, missing semicolons, etc. (no code change) |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `perf` | Performance improvement |
| `test` | Adding or correcting tests |
| `build` | Changes to the build system or dependencies |
| `ci` | Changes to CI configuration |
| `chore` | Other changes that don't modify src or test files |

### Scopes

Use the crate name as the scope when the change is crate-specific:

```
feat(abp-core): add ToolResult event variant
fix(abp-host): handle sidecar timeout correctly
docs(abp-protocol): clarify envelope handshake sequence
```

## Pull Request Process

1. **Fill out the PR template** — describe your changes and complete the checklist.
2. **Keep PRs focused** — one logical change per PR. Split large changes into
   stacked PRs if needed.
3. **Ensure CI passes** — formatting, linting, tests, documentation, and schema
   checks must all be green.
4. **Respond to review feedback** — address all comments. Use "Resolve conversation"
   when addressed.
5. **Squash and merge** — PRs are squash-merged to keep a clean `main` history.

### Review Expectations

- At least one maintainer approval is required.
- Contract changes (`abp-core`) require extra scrutiny — the contract is the product.
- Breaking changes must be discussed in an issue first.

## Release Process

ABP follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html):

1. Version bumps are coordinated across the workspace in `Cargo.toml`.
2. Update `CHANGELOG.md` following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format.
3. Regenerate schemas: `cargo run -p xtask -- schema`.
4. Tag the release: `git tag v0.x.y`.
5. Push the tag to trigger the release workflow.

## Questions?

If you're unsure about anything, open an issue or start a discussion. We're happy
to help you contribute!

## License

By contributing, you agree that your contributions will be dual-licensed under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option.
