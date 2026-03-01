# Agent Backplane

[![CI](https://github.com/EffortlessMetrics/agent-backplane/actions/workflows/ci.yml/badge.svg)](https://github.com/EffortlessMetrics/agent-backplane/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/abp-core.svg)](https://crates.io/crates/abp-core)
[![docs.rs](https://docs.rs/abp-core/badge.svg)](https://docs.rs/abp-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![codecov](https://codecov.io/gh/EffortlessMetrics/agent-backplane/branch/main/graph/badge.svg)](https://codecov.io/gh/EffortlessMetrics/agent-backplane)

## What is ABP?

Agent Backplane (ABP) is a **translation layer between AI agent SDKs**. Author a
work order once, route it to *any* supported backend — Claude, Codex, Gemini,
Kimi, Copilot, or a local model — without rewriting client code.

ABP solves three problems that arise when you work with multiple AI agent APIs:

1. **Format divergence** — every vendor uses different JSON shapes for requests,
   responses, tool calls, and streaming events.
2. **Semantic drift** — tool names, event lifecycle labels, and capability
   surfaces differ across SDKs.
3. **Capability heterogeneity** — not every vendor supports every feature; ABP
   tracks what is native, emulated, or unsupported per backend.

**The contract is the product.** `abp-core` defines `WorkOrder`, `Receipt`,
`AgentEvent`, capabilities, and policy types. Everything else exists to
faithfully map SDK semantics into that contract and back out.

## Architecture

```
abp-glob ──────────┐
                    ├── abp-policy ──────────┐
abp-core ──────────┤                         │
  │                └── abp-workspace ────────┤
  │                                          │
abp-protocol ─── abp-host ─── abp-integrations ─── abp-runtime ─── abp-cli
                     │               │                                  │
                 sidecar-kit    abp-dialect                        abp-daemon
                     │
                claude-bridge
```

`abp-core` sits at the bottom — if you take one dependency, take that one. The
hierarchy above it adds the wire protocol, sidecar supervision, backend trait,
orchestration, and finally the CLI + HTTP daemon. `sidecar-kit` and
`claude-bridge` are independent transport crates that speak the same JSONL
protocol without pulling in the full runtime.

See [`docs/architecture.md`](docs/architecture.md) for the detailed
crate-by-crate walkthrough.

## Quick Start

```bash
# Build everything
cargo build

# Run a work order against the built-in mock backend (no API keys needed)
cargo run -p abp-cli -- run --task "say hello" --backend mock

# The receipt is written to .agent-backplane/receipts/<run_id>.json
```

A minimal work order → receipt flow looks like this (conceptually):

```text
WorkOrder { task: "say hello", backend: "mock" }
   │
   ▼
abp-runtime: resolve backend → prepare workspace → dispatch
   │
   ▼
MockBackend: stream AgentEvents (RunStarted, AssistantMessage, RunCompleted)
   │
   ▼
Receipt { status: success, events: [...], receipt_sha256: "ab3f…" }
```

## Workspace Crates

| Crate | Description |
|-------|-------------|
| [`abp-core`](crates/abp-core) | Stable contract types — `WorkOrder`, `Receipt`, `AgentEvent`, capabilities |
| [`abp-protocol`](crates/abp-protocol) | JSONL wire format (`Envelope` tagged with `#[serde(tag = "t")]`) |
| [`abp-host`](crates/abp-host) | Sidecar process supervision, JSONL handshake + event streaming over stdio |
| [`abp-glob`](crates/abp-glob) | Include/exclude glob compilation using `globset` |
| [`abp-git`](crates/abp-git) | Git repository helpers for workspace staging and diff verification |
| [`abp-workspace`](crates/abp-workspace) | Staged workspace creation (temp dir copy with glob filtering, auto git init) |
| [`abp-policy`](crates/abp-policy) | Policy engine — compiles `PolicyProfile` into allow/deny checks for tools/read/write |
| [`abp-backend-core`](crates/abp-backend-core) | Shared `Backend` trait and capability helpers |
| [`abp-backend-mock`](crates/abp-backend-mock) | Mock backend for local testing without external dependencies |
| [`abp-backend-sidecar`](crates/abp-backend-sidecar) | Sidecar backend adapter bridging JSONL protocol agents |
| [`abp-integrations`](crates/abp-integrations) | Backend registry re-exporting mock + sidecar backends |
| [`abp-dialect`](crates/abp-dialect) | Dialect detection, validation, and metadata |
| [`abp-runtime`](crates/abp-runtime) | Orchestration — workspace → backend → event multiplexing → hashed receipt |
| [`abp-cli`](crates/abp-cli) | `abp` binary with `run` and `backends` subcommands |
| [`abp-daemon`](crates/abp-daemon) | HTTP control-plane API with receipt persistence |
| [`abp-claude-sdk`](crates/abp-claude-sdk) | Anthropic Claude SDK adapter |
| [`abp-codex-sdk`](crates/abp-codex-sdk) | OpenAI Codex SDK adapter |
| [`abp-openai-sdk`](crates/abp-openai-sdk) | OpenAI Chat Completions SDK adapter |
| [`abp-gemini-sdk`](crates/abp-gemini-sdk) | Google Gemini SDK adapter |
| [`abp-kimi-sdk`](crates/abp-kimi-sdk) | Kimi (Moonshot) SDK adapter |
| [`abp-copilot-sdk`](crates/abp-copilot-sdk) | GitHub Copilot sidecar SDK integration |
| [`abp-sidecar-sdk`](crates/abp-sidecar-sdk) | Shared sidecar registration helpers for vendor SDK microcrates |
| [`sidecar-kit`](crates/sidecar-kit) | Value-based JSONL transport layer for sidecar processes |
| [`claude-bridge`](crates/claude-bridge) | Standalone Claude SDK bridge built on sidecar-kit |

## SDK Support Matrix

| Vendor | SDK Crate | Sidecar Host | Work Order Mapping | Response Mapping | Tool Translation | Streaming |
|--------|-----------|-------------|-------------------|-----------------|-----------------|-----------|
| **Anthropic Claude** | `abp-claude-sdk` | `hosts/claude` | ✅ | ✅ | ✅ | ✅ |
| **OpenAI Codex** | `abp-codex-sdk` | `hosts/codex` | ✅ | ✅ | ✅ | ✅ |
| **OpenAI Chat** | `abp-openai-sdk` | — | ✅ | ✅ | ✅ | ✅ |
| **Google Gemini** | `abp-gemini-sdk` | `hosts/gemini` | ✅ | ✅ | ✅ | ✅ |
| **Moonshot Kimi** | `abp-kimi-sdk` | `hosts/kimi` | ✅ | ✅ | ✅ | ✅ |
| **GitHub Copilot** | `abp-copilot-sdk` | `hosts/copilot` | 🚧 scaffold | 🚧 scaffold | 🚧 scaffold | 🚧 scaffold |

✅ = implemented · 🚧 = scaffold / in progress

See [`docs/sdk_mapping.md`](docs/sdk_mapping.md) for the full dialect × engine
mapping matrix, tool name tables, and capability comparison.

## Sidecar Hosts

Example sidecars live in `hosts/`. Each speaks the JSONL protocol over stdio:

| Host | Runtime | Notes |
|------|---------|-------|
| `hosts/node` | Node.js | Minimal JSONL sidecar example |
| `hosts/python` | Python | Minimal example, optional `claude_agent_sdk` client mode |
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

### Build & Test

```bash
cargo build                    # Build all workspace crates
cargo test --workspace         # Run all tests
cargo run -p xtask -- schema   # Generate JSON schemas to contracts/schemas/
```

### Run with Backends

```bash
# Mock backend — no API keys required
cargo run -p abp-cli -- run --task "say hello" --backend mock

# List registered backends
cargo run -p abp-cli -- backends

# Sidecar backends (requires node; some need npm install first)
cargo run -p abp-cli -- run --task "hello" --backend sidecar:node
cargo run -p abp-cli -- run --task "hello" --backend sidecar:claude
cargo run -p abp-cli -- run --task "hello" --backend sidecar:codex
cargo run -p abp-cli -- run --task "hello" --backend sidecar:copilot
cargo run -p abp-cli -- run --task "hello" --backend sidecar:kimi
cargo run -p abp-cli -- run --task "hello" --backend sidecar:gemini

# Vendor-specific params
cargo run -p abp-cli -- run --task "summarize this codebase" --backend gemini \
  --model gemini-2.5-flash --param stream=true --param vertex=false

# Start the HTTP daemon
cargo run -p abp-daemon -- --bind 127.0.0.1:8088
```

Enable debug logging with `--debug` or `RUST_LOG=abp=debug`.

Receipts are persisted to `.agent-backplane/receipts/<run_id>.json`.

> **Note:** Some sidecar hosts require `npm install` first
> (e.g. `npm --prefix hosts/copilot install`).

## Sidecar Protocol

Sidecars are external processes that speak **JSONL over stdio**:

1. Sidecar sends `hello` (MUST be first line) — identity + capabilities
2. Control plane sends `run` — contains the `WorkOrder`
3. Sidecar streams `event` envelopes — `AgentEvent`s
4. Sidecar concludes with `final` (receipt) or `fatal` (error)

Transport extensions (`sidecar-kit`): `cancel`, `ping`/`pong` for heartbeat.

All envelopes use `ref_id` to correlate with the run. See
[`docs/sidecar_protocol.md`](docs/sidecar_protocol.md) for the full
specification.

## Configuration

ABP can be configured via `backplane.toml`. See
[`backplane.example.toml`](backplane.example.toml) for the full example.

```toml
[backends.mock]
type = "mock"

[backends.openai]
type = "sidecar"
command = "node"
args = ["path/to/openai-sidecar.js"]
```

## Daemon API

The HTTP daemon (`abp-daemon`) exposes a REST API:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/backends` | GET | List registered backends |
| `/capabilities` | GET | Query backend capabilities (optional `?backend=<name>`) |
| `/run` | POST | Submit a work order |
| `/receipts` | GET | List all receipts |
| `/receipts/:run_id` | GET | Fetch a specific receipt |

## Testing

| Category | Description | Example |
|----------|-------------|---------|
| **Unit tests** | Per-crate module-level tests | `cargo test -p abp-core` |
| **Snapshot tests** | JSON serialization stability via [insta](https://insta.rs) | `crates/*/tests/snapshots/` |
| **Property tests** | Randomized input via [proptest](https://proptest-rs.github.io/proptest/) | `crates/abp-core/tests/proptest_types.rs` |
| **Fuzz tests** | [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets | `fuzz/fuzz_targets/` |
| **Benchmarks** | [criterion](https://bheisler.github.io/criterion.rs/) micro-benchmarks | `benches/` |
| **Conformance tests** | End-to-end sidecar protocol conformance (JS) | `tests/conformance/` |
| **Doc tests** | In-doc examples verified by CI | `cargo test --doc --workspace` |

```bash
cargo test --workspace                    # all tests
cargo test -p abp-core receipt_hash       # single test by name
cargo insta review                        # snapshot review
cargo bench --workspace                   # benchmarks
cd fuzz && cargo +nightly fuzz run fuzz_envelope  # fuzz
```

### CI Pipeline

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs on every push
and PR:

- `cargo fmt --check` + `cargo clippy`
- `cargo test --workspace` on Ubuntu and Windows
- `cargo doc --workspace --no-deps` with `-D warnings`
- Cargo Deny — license and advisory audit
- Schema generation — verifies `contracts/schemas/` are up-to-date

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — Detailed crate hierarchy and message flow
- [`docs/sidecar_protocol.md`](docs/sidecar_protocol.md) — JSONL wire format specification
- [`docs/sdk_mapping.md`](docs/sdk_mapping.md) — Dialect × engine mapping matrix and fidelity rules
- [`docs/dialect_engine_matrix.md`](docs/dialect_engine_matrix.md) — Passthrough vs mapped routing design
- [`docs/capabilities.md`](docs/capabilities.md) — Capability model reference
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — Contribution guidelines
- [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) — Contributor Covenant Code of Conduct

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full guide. The short version:

1. Fork the repo and create a feature branch
2. `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
3. Add tests for new functionality
4. `cargo test --workspace`
5. If contract types changed: `cargo run -p xtask -- schema`
6. Open a pull request against `main`

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.
