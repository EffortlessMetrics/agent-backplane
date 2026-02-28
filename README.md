# Agent Backplane

[![CI](https://github.com/EffortlessMetrics/agent-backplane/actions/workflows/ci.yml/badge.svg)](https://github.com/EffortlessMetrics/agent-backplane/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![codecov](https://codecov.io/gh/EffortlessMetrics/agent-backplane/branch/main/graph/badge.svg)](https://codecov.io/gh/EffortlessMetrics/agent-backplane)
<!-- [![crates.io](https://img.shields.io/crates/v/abp-core.svg)](https://crates.io/crates/abp-core) -->

Agent Backplane (ABP) is a **translation layer between agent SDKs**. It provides vendor-agnostic SDK shims that map each vendor's surface area onto a stable internal contract, then routes work orders to any backend (OpenAI, Anthropic, Gemini, local models, etc.) via a projection matrix.

**The contract is the product.** Everything else exists to faithfully map SDK semantics into that contract and back out again.

## Architecture

```
                        +----------------+
                        |    abp-cli     |
                        +-------+--------+
                                |
                        +-------v--------+
                        |  abp-runtime   |
                        +-------+--------+
                                |
               +----------------+----------------+
               |                |                |
        +------v---------+ +---v--------+ +-----v----------+
        |abp-integrations| |abp-policy  | | abp-workspace  |
        +------+---------+ +---+--------+ +-----+----------+
               |               |                |
        +------v-------+ +----v-------+ +------v-----------+
        |   abp-host   | |  abp-glob  | |     abp-glob    |
        +------+-------+ +------------+ +------------------+
               |
        +------v--------+
        | abp-protocol  |
        +------+--------+
               |
        +------v--------+     +--------------+
        |   abp-core    |     |  sidecar-kit |
        +---------------+     +------+-------+
                                     |
                               +-----v--------+
                               | claude-bridge|
                               +--------------+
```

### Crate Overview

| Crate | Description |
|-------|-------------|
| **abp-core** | Stable contract types — `WorkOrder`, `Receipt`, `AgentEvent`, capabilities |
| **abp-protocol** | JSONL wire format (`Envelope` tagged with `#[serde(tag = "t")]`) |
| **abp-host** | Sidecar process supervision, JSONL handshake + event streaming over stdio |
| **abp-glob** | Include/exclude glob compilation using `globset` |
| **abp-workspace** | Staged workspace creation (temp dir copy with glob filtering, auto git init) |
| **abp-policy** | Policy engine — compiles `PolicyProfile` into allow/deny checks for tools/read/write |
| **abp-integrations** | `Backend` trait + `MockBackend` + `SidecarBackend` implementations |
| **abp-runtime** | Orchestration — workspace → backend → event multiplexing → hashed receipt |
| **abp-cli** | `abp` binary with `run` and `backends` subcommands |
| **abp-daemon** | HTTP control-plane API with receipt persistence |
| **abp-claude-sdk** | Anthropic Claude SDK adapter |
| **abp-codex-sdk** | OpenAI Codex SDK adapter |
| **abp-gemini-sdk** | Google Gemini SDK adapter |
| **abp-kimi-sdk** | Kimi (Moonshot) SDK adapter |
| **sidecar-kit** | Value-based JSONL transport layer for sidecar processes |
| **claude-bridge** | Standalone Claude SDK bridge built on sidecar-kit |

### Sidecar Hosts

Example sidecars live in `hosts/`. Each speaks the JSONL protocol over stdio:

| Host | Runtime | Notes |
|------|---------|-------|
| `hosts/node` | Node.js | Minimal JSONL sidecar example |
| `hosts/python` | Python | Minimal JSONL sidecar example, optional `claude_agent_sdk` client mode |
| `hosts/claude` | Node.js | Claude-oriented sidecar with pluggable adapter module |
| `hosts/codex` | Node.js | Codex-oriented sidecar with passthrough/mapped modes |
| `hosts/copilot` | Node.js | GitHub Copilot sidecar scaffold |
| `hosts/kimi` | Node.js | Kimi sidecar with SDK-first adapter and CLI fallback |
| `hosts/gemini` | Node.js | Gemini sidecar with Claude-to-Gemini mapping |

## Getting Started

### Prerequisites

- **Rust** (nightly recommended — workspace uses edition 2024)
- **Node.js** (for sidecar hosts)
- **Python** (optional, for the Python sidecar)

### Build & Run

```bash
# Build all workspace crates
cargo build

# Run all tests
cargo test --workspace

# Generate JSON schemas for the public contract
cargo run -p xtask -- schema

# Run with the mock backend (no external deps)
cargo run -p abp-cli -- run --task "say hello" --backend mock

# List available backends
cargo run -p abp-cli -- backends

# Run with vendor params
cargo run -p abp-cli -- run --task "summarize this codebase" --backend gemini \
  --model gemini-2.5-flash --param stream=true --param vertex=false

# Start the daemon control plane
cargo run -p abp-daemon -- --bind 127.0.0.1:8088
```

Enable debug logging with the `--debug` flag or `RUST_LOG=abp=debug`.

Receipts land in `.agent-backplane/receipts/<run_id>.json`.

### Available Sidecar Backends

```bash
cargo run -p abp-cli -- run --task "hello" --backend sidecar:node      # Node.js
cargo run -p abp-cli -- run --task "hello" --backend sidecar:python    # Python
cargo run -p abp-cli -- run --task "hello" --backend sidecar:claude    # Claude
cargo run -p abp-cli -- run --task "hello" --backend sidecar:codex     # Codex
cargo run -p abp-cli -- run --task "hello" --backend sidecar:copilot   # Copilot
cargo run -p abp-cli -- run --task "hello" --backend sidecar:kimi      # Kimi
cargo run -p abp-cli -- run --task "hello" --backend sidecar:gemini    # Gemini
```

> **Note:** Some sidecar hosts require `npm install` first (e.g. `npm --prefix hosts/copilot install`).

## Configuration

ABP can be configured via a `backplane.toml` file. See [`backplane.example.toml`](backplane.example.toml) for a full example.

```toml
[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["path/to/openai-sidecar.js"]

[backends.anthropic]
type = "sidecar"
command = "python3"
args = ["path/to/anthropic-sidecar.py"]
```

Each backend entry declares either a `"mock"` type for the built-in mock or a `"sidecar"` type with a command and arguments to spawn the sidecar process.

## Daemon API

The HTTP daemon (`abp-daemon`) exposes a REST API for programmatic use:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/backends` | GET | List registered backends |
| `/capabilities` | GET | Query backend capabilities (optional `?backend=<name>`) |
| `/run` | POST | Submit a work order (`{ "backend": "mock", "work_order": {...} }`) |
| `/receipts` | GET | List all receipts |
| `/receipts/:run_id` | GET | Fetch a specific receipt |

## Testing

The project has a comprehensive test suite spanning multiple strategies:

### Test Categories

| Category | Description | Example |
|----------|-------------|---------|
| **Unit tests** | Per-crate module-level tests | `cargo test -p abp-core` |
| **Snapshot tests** | JSON serialization stability via [insta](https://insta.rs) | `crates/*/tests/snapshots/` |
| **Property tests** | Randomized input via [proptest](https://proptest-rs.github.io/proptest/) | `crates/abp-core/tests/proptest_types.rs` |
| **Fuzz tests** | [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for envelope/receipt/work-order parsing | `fuzz/fuzz_targets/` |
| **Benchmarks** | [criterion](https://bheisler.github.io/criterion.rs/) micro-benchmarks for hashing, glob matching, policy eval | `crates/*/benches/` |
| **Conformance tests** | End-to-end sidecar protocol conformance (JS) | `tests/conformance/` |
| **Doc tests** | In-doc examples verified by CI | `cargo test --doc --workspace` |

### Running Tests

```bash
# All tests
cargo test --workspace

# Single crate
cargo test -p abp-core

# Single test by name
cargo test -p abp-core receipt_hash

# Snapshot review (requires cargo-insta)
cargo insta review

# Benchmarks
cargo bench --workspace

# Fuzz (requires cargo-fuzz, nightly)
cd fuzz && cargo +nightly fuzz run fuzz_envelope

# Conformance tests (requires node)
cd tests/conformance && node runner.js
```

### CI Pipeline

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs on every push and PR:

- **Format & lint** — `cargo fmt --check` + `cargo clippy`
- **Tests** — `cargo test --workspace` on Ubuntu and Windows
- **Doc tests** — `cargo test --doc --workspace`
- **Documentation** — `cargo doc --workspace --no-deps` with `-D warnings`
- **Cargo Deny** — license and advisory audit
- **Benchmarks** — compile-check (`--no-run`)
- **Schema generation** — verifies `contracts/schemas/` are up-to-date

## Sidecar Protocol

Sidecars are external processes (Node/Python/etc.) that speak a JSONL protocol over stdio:

1. Sidecar sends `hello` envelope (MUST be first) with backend identity + capabilities
2. Control plane sends `run` with a `WorkOrder`
3. Sidecar streams `event` envelopes containing `AgentEvent`s
4. Sidecar concludes with `final` (containing a `Receipt`) or `fatal`

All envelopes use `ref_id` to correlate with the run. See [`docs/sidecar_protocol.md`](docs/sidecar_protocol.md) for the full specification.

## What's Next

The current scaffold is designed for incremental extension:

- Projection matrix implementation (dialect-to-dialect mapping)
- Policy enforcement inside sidecars
- Sandboxing/container isolation
- Additional vendor SDK adapters

## Contributing

1. Fork the repo and create a feature branch
2. Ensure `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass
3. Add tests for new functionality
4. Run `cargo test --workspace` to verify nothing is broken
5. If contract types changed, regenerate schemas: `cargo run -p xtask -- schema`
6. Open a pull request against `main`

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
