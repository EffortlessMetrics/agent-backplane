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

The workspace contains **51 crates** organized in layers:

```
abp-glob ──────────┐
                    ├── abp-policy ──────────┐
abp-core ──────────┤                         │
  │   │            └── abp-workspace ────────┤
  │   │                      │               │
  │   ├── abp-ir ─── abp-mapper             │
  │   ├── abp-dialect ─── abp-mapping        │
  │   ├── abp-error ─── abp-error-taxonomy   │
  │   ├── abp-capability ─── abp-projection  │
  │   ├── abp-emulation                      │
  │   ├── abp-receipt                        │
  │   │     └── abp-telemetry                │
  │   ├── abp-config                         │
  │   └── abp-sdk-types                      │
  │                                          │
abp-protocol ─── abp-host ─── abp-backend-core ─── abp-backend-mock
  │                  │              │                abp-backend-sidecar
  │              sidecar-kit        │
  │                  │         abp-integrations ─── abp-runtime ─── abp-cli
  │             claude-bridge                           │             │
  │             gemini-bridge                        abp-stream   abp-daemon
  │             openai-bridge
  │                                            abp-ratelimit
  ├── abp-sidecar-proto
  └── abp-sidecar-utils

SDK shims (drop-in client replacements):
  abp-shim-openai, abp-shim-claude, abp-shim-gemini,
  abp-shim-codex,  abp-shim-kimi,   abp-shim-copilot
```

`abp-core` sits at the bottom — if you take one dependency, take that one. The
hierarchy above it adds the wire protocol, sidecar supervision, backend trait,
orchestration, and finally the CLI + HTTP daemon. `sidecar-kit` and
`claude-bridge` are independent transport crates that speak the same JSONL
protocol without pulling in the full runtime. The `abp-shim-*` crates provide
drop-in SDK replacements that transparently route through ABP.

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
| [`abp-projection`](crates/abp-projection) | Projection matrix routing work orders to best-fit backend |
| [`abp-stream`](crates/abp-stream) | Agent event stream processing, filtering, and multiplexing |
| [`abp-capability`](crates/abp-capability) | Capability negotiation between requirements and backend manifests |
| [`abp-error`](crates/abp-error) | Unified error taxonomy with stable machine-readable error codes |
| [`abp-receipt`](crates/abp-receipt) | Receipt canonicalization, hashing, chain verification, and diffing |
| [`abp-mapping`](crates/abp-mapping) | Cross-dialect feature mapping validation with fidelity tracking |
| [`abp-config`](crates/abp-config) | TOML configuration loading, validation, and merging |
| [`abp-sidecar-proto`](crates/abp-sidecar-proto) | Sidecar-side utilities for ABP JSONL protocol services |
| [`abp-emulation`](crates/abp-emulation) | Labeled capability emulation engine |
| [`abp-error-taxonomy`](crates/abp-error-taxonomy) | Error classification, severity, and recovery suggestion helpers |
| [`abp-ir`](crates/abp-ir) | Intermediate representation normalization passes and vendor-specific lowering |
| [`abp-mapper`](crates/abp-mapper) | Dialect mapping engine — JSON-level and IR-level cross-dialect translation |
| [`abp-sdk-types`](crates/abp-sdk-types) | SDK-specific dialect type definitions (pure data model, no networking) |
| [`abp-telemetry`](crates/abp-telemetry) | Structured metrics and telemetry collection |
| [`abp-retry`](crates/abp-retry) | Retry and circuit-breaker middleware for backend calls |
| [`abp-validate`](crates/abp-validate) | Validation utilities for work orders, receipts, events, and envelopes |
| [`abp-receipt-store`](crates/abp-receipt-store) | Receipt persistence and retrieval |
| [`abp-runtime`](crates/abp-runtime) | Orchestration — workspace → backend → event multiplexing → hashed receipt |
| [`abp-cli`](crates/abp-cli) | `abp` binary with `run`, `backends`, `validate`, `config`, `receipt` subcommands |
| [`abp-daemon`](crates/abp-daemon) | HTTP control-plane API with receipt persistence, metrics, validation, and WebSocket |
| [`abp-shim-openai`](crates/abp-shim-openai) | Drop-in OpenAI SDK shim that routes through ABP |
| [`abp-shim-claude`](crates/abp-shim-claude) | Drop-in Anthropic Claude SDK shim that routes through ABP |
| [`abp-shim-gemini`](crates/abp-shim-gemini) | Drop-in Gemini SDK shim that routes through ABP |
| [`abp-shim-codex`](crates/abp-shim-codex) | Drop-in Codex SDK shim that routes through ABP |
| [`abp-shim-kimi`](crates/abp-shim-kimi) | Drop-in Kimi SDK shim that routes through ABP |
| [`abp-shim-copilot`](crates/abp-shim-copilot) | Drop-in Copilot SDK shim that routes through ABP |
| [`abp-claude-sdk`](crates/abp-claude-sdk) | Anthropic Claude SDK adapter |
| [`abp-codex-sdk`](crates/abp-codex-sdk) | OpenAI Codex SDK adapter |
| [`abp-openai-sdk`](crates/abp-openai-sdk) | OpenAI Chat Completions SDK adapter |
| [`abp-gemini-sdk`](crates/abp-gemini-sdk) | Google Gemini SDK adapter |
| [`abp-kimi-sdk`](crates/abp-kimi-sdk) | Kimi (Moonshot) SDK adapter |
| [`abp-copilot-sdk`](crates/abp-copilot-sdk) | GitHub Copilot sidecar SDK integration |
| [`abp-sidecar-sdk`](crates/abp-sidecar-sdk) | Shared sidecar registration helpers for vendor SDK microcrates |
| [`abp-sidecar-utils`](crates/abp-sidecar-utils) | Reusable sidecar protocol utilities (streaming codec, handshake, heartbeat) |
| [`abp-ratelimit`](crates/abp-ratelimit) | Rate limiting primitives (token bucket, sliding window) for backend calls |
| [`sidecar-kit`](crates/sidecar-kit) | Value-based JSONL transport layer for sidecar processes |
| [`claude-bridge`](crates/claude-bridge) | Standalone Claude SDK bridge built on sidecar-kit |
| [`gemini-bridge`](crates/gemini-bridge) | Standalone Gemini SDK bridge built on sidecar-kit |
| [`openai-bridge`](crates/openai-bridge) | Standalone OpenAI Chat Completions bridge built on sidecar-kit |

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

### SDK Shim Quick Start

Each `abp-shim-*` crate mirrors the corresponding vendor SDK so you can swap it
in with minimal code changes. Add the shim as a dependency instead of the vendor
SDK, construct a request using the familiar types, and call the conversion
function:

```rust
// OpenAI shim — build a ChatCompletionRequest, convert to ABP WorkOrder
use abp_shim_openai::types::ChatCompletionRequest;
use abp_shim_openai::{Message, Role};

let request = ChatCompletionRequest {
    model: "gpt-4".into(),
    messages: vec![Message { role: Role::User, content: "Hello".into(), ..Default::default() }],
    ..Default::default()
};
let work_order = abp_shim_openai::convert::to_work_order(&request)?;
```

```rust
// Claude shim — mirrors Anthropic Messages API types
use abp_shim_claude::types::MessagesRequest;
use abp_shim_claude::Role;

let request = MessagesRequest {
    model: "claude-sonnet-4-20250514".into(),
    messages: vec![/* Claude-style messages */],
    max_tokens: 1024,
    ..Default::default()
};
let work_order = abp_shim_claude::convert::to_work_order(&request)?;
```

```rust
// Gemini shim — mirrors Google generateContent types
use abp_shim_gemini::types::{GenerateContentRequest, Content, Part};

let request = GenerateContentRequest {
    contents: vec![Content { role: "user".into(), parts: vec![Part::Text { text: "Hello".into() }] }],
    ..Default::default()
};
// Gemini shim converts via IR: ir_to_work_order(&ir_request, "gemini-2.5-flash", &None)
```

All shims follow the same pattern: `convert::to_work_order()` turns a
vendor-specific request into an ABP `WorkOrder`, and
`convert::from_receipt()` turns an ABP `Receipt` back into the vendor's
response type.

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
| `/metrics` | GET | Runtime metrics |
| `/backends` | GET | List registered backends |
| `/capabilities` | GET | Query backend capabilities (optional `?backend=<name>`) |
| `/config` | GET | Current configuration |
| `/validate` | POST | Validate a WorkOrder or Receipt JSON |
| `/schema/{schema_type}` | GET | Retrieve a JSON schema |
| `/run` | POST | Submit a work order |
| `/runs` | GET | List all runs |
| `/runs` | POST | Submit a work order (alias) |
| `/runs/:run_id` | GET | Fetch a specific run |
| `/runs/:run_id` | DELETE | Delete a run |
| `/runs/:run_id/receipt` | GET | Fetch the receipt for a run |
| `/runs/:run_id/cancel` | POST | Cancel an in-progress run |
| `/runs/:run_id/events` | GET | Stream events for a run |
| `/receipts` | GET | List all receipts |
| `/receipts/:run_id` | GET | Fetch a specific receipt |
| `/ws` | GET | WebSocket connection for real-time events |

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
