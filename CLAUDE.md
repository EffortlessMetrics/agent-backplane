# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Agent Backplane (ABP) is a **translation layer between agent SDKs**. It provides vendor-agnostic SDK shims that map each vendor's surface area onto a stable internal contract, then routes work orders to any backend (OpenAI, Anthropic, Gemini, Kimi, Copilot, local models) via a projection matrix. The workspace contains **54 crates** — contract types, sidecar protocol, SDK shims, IR translators, bridge crates, and a CLI + HTTP daemon.

## Build & Test Commands

```bash
cargo build                                    # Build all workspace crates
cargo test                                     # Run all tests
cargo test -p abp-core                         # Run tests for a single crate
cargo test -p abp-core receipt_hash            # Run a single test by name
cargo test -p abp-policy                       # Policy crate has unit tests
cargo test -p abp-glob                         # Glob crate has unit tests
cargo run -p xtask -- schema                   # Generate JSON schemas to contracts/schemas/
cargo run -p abp-cli -- run --task "hello" --backend mock   # Run with mock backend
cargo run -p abp-cli -- run --task "hello" --backend sidecar:node    # Node sidecar (requires node)
cargo run -p abp-cli -- run --task "hello" --backend sidecar:claude  # Claude sidecar (requires node)
cargo run -p abp-cli -- run --task "hello" --backend sidecar:copilot # Copilot sidecar (requires node)
cargo run -p abp-cli -- run --task "hello" --backend sidecar:kimi    # Kimi sidecar (requires node)
cargo run -p abp-cli -- run --task "hello" --backend sidecar:gemini  # Gemini sidecar (requires node)
cargo run -p abp-cli -- backends               # List available backends
```

The CLI must be run from the repo root for sidecar backends (they resolve `hosts/` scripts relative to CWD).

Enable debug logging with `--debug` flag on the CLI or `RUST_LOG=abp=debug`.

## Developer Workflow (Enforced)

After one-time setup (`cargo xtask setup` or `just setup`), quality gates run automatically.

### What happens on commit

When you commit Rust or Cargo changes, the pre-commit hook:
1. Runs `cargo xtask lint-fix` (auto-formats + best-effort clippy fixes)
2. Re-stages the originally-staged files with any formatting corrections

### What happens on push

The pre-push hook runs `cargo xtask gate --check` (fmt check + cargo check + clippy strict + test compile). Push is blocked on failure.

### CI parity

CI runs the same `cargo xtask gate --check`, so local hooks give exact CI parity.

### One-time setup

```bash
cargo xtask setup      # sets core.hooksPath=.githooks (idempotent)
```

### How it works

- Generated tests are held to `clippy -D warnings` -- the gate enforces this automatically.
- Emergency bypass: `git commit --no-verify` / `git push --no-verify`.
- **Agents: never use `--no-verify` unless the human operator explicitly instructs you to.**

## Architecture

### Core Principle

**The contract is the product.** `abp-core` defines `WorkOrder`, `Receipt`, `AgentEvent`, capabilities, and policy types. Everything else exists to faithfully map SDK semantics into that contract and back out.

### Crate Dependency Hierarchy (bottom-up)

```
abp-glob ──────────┐
                    ├── abp-policy ──────────┐
abp-core ──────────┤                         │
  │   │            └── abp-workspace ────────┤
  │   │                   │  abp-git         │
  │   ├── abp-ir ─── abp-mapper             │
  │   ├── abp-dialect ─── abp-mapping        │
  │   ├── abp-error ─── abp-error-taxonomy   │
  │   ├── abp-capability ─── abp-projection  │
  │   ├── abp-emulation                      │
  │   ├── abp-receipt                        │
  │   │     ├── abp-telemetry                │
  │   │     └── abp-receipt-store            │
  │   ├── abp-config                         │
  │   ├── abp-sdk-types                      │
  │   │     └── abp-validate                 │
  │   │                                      │
abp-protocol ─── abp-host ─── abp-backend-core ─── abp-backend-mock
  │                  │              │                abp-backend-sidecar
  │              sidecar-kit        │
  │                  │         abp-integrations ─── abp-runtime ─── abp-cli
  │             claude-bridge                           │             │
  │             gemini-bridge                        abp-stream   abp-daemon
  │             openai-bridge
  │             codex-bridge
  │             copilot-bridge                   abp-ratelimit
  │             kimi-bridge                      abp-retry
  ├── abp-sidecar-proto
  ├── abp-sidecar-sdk
  └── abp-sidecar-utils

SDK shims (drop-in client replacements):
  abp-shim-openai, abp-shim-claude, abp-shim-gemini,
  abp-shim-codex,  abp-shim-kimi,   abp-shim-copilot

SDK adapters (vendor API translation):
  abp-claude-sdk, abp-codex-sdk, abp-openai-sdk,
  abp-gemini-sdk, abp-kimi-sdk,  abp-copilot-sdk
```

- **abp-core**: Stable contract types only. If you take one dep, take this one. Contains `CONTRACT_VERSION = "abp/v0.1"`.
- **abp-protocol**: JSONL wire format (`Envelope` enum tagged with `#[serde(tag = "t")]` — the discriminator field is `t`, not `type`).
- **abp-host**: Spawns sidecar processes, handles JSONL handshake + event streaming over stdio.
- **abp-glob**: Include/exclude glob compilation using `globset`. Used by both workspace staging and policy.
- **abp-workspace**: Staged workspace creation (temp dir copy with glob filtering), auto-initializes git for meaningful diffs.
- **abp-policy**: Compiles `PolicyProfile` into `PolicyEngine` with tool/read/write allow/deny checks via globs.
- **abp-backend-core**: Shared `Backend` trait and capability helpers.
- **abp-backend-mock**: Mock backend for local testing without external API keys.
- **abp-backend-sidecar**: Sidecar backend adapter bridging JSONL protocol agents.
- **abp-integrations**: Backend registry re-exporting mock + sidecar backends.
- **abp-runtime**: Orchestration — prepares workspace, selects backend, multiplexes event streams, produces canonical hashed receipt.
- **abp-cli**: `abp` binary with `run`, `backends`, `validate`, `schema`, `inspect`, `translate`, `health`, `config`, `receipt`, `status` subcommands.
- **abp-daemon**: HTTP control-plane API with REST endpoints and WebSocket support.
- **abp-ir**: Intermediate representation for vendor-neutral cross-dialect message normalization.
- **abp-mapper**: Dialect mapping engine — JSON-level and IR-level cross-dialect translation.
- **abp-dialect**: Dialect detection, validation, and metadata for all supported vendors.
- **abp-projection**: Projection matrix routing work orders to best-fit backends.
- **abp-capability**: Capability negotiation between requirements and backend manifests.
- **abp-emulation**: Labeled capability emulation engine (never silently degrades).
- **abp-receipt**: Receipt canonicalization, chaining, diffing, and hash verification.
- **abp-telemetry**: Structured metrics and telemetry collection.
- **abp-config**: TOML configuration loading, validation, and merging.
- **abp-error** / **abp-error-taxonomy**: Unified error codes with classification and recovery suggestions.
- **abp-git**: Git repository helpers for workspace staging and diff verification.
- **abp-validate**: Validation utilities for work orders, receipts, events, and envelopes.
- **abp-receipt-store**: Receipt persistence and retrieval.
- **abp-stream**: Agent event stream processing, filtering, transformation, and multiplexing.
- **abp-ratelimit**: Rate limiting primitives (token bucket, sliding window) for backend calls.
- **abp-retry**: Retry and circuit-breaker middleware for backend calls.
- **abp-sidecar-sdk**: Shared sidecar registration helpers for vendor SDK microcrates.
- **sidecar-kit**: Value-based JSONL transport layer for sidecar processes.
- **claude-bridge** / **gemini-bridge** / **openai-bridge** / **codex-bridge** / **copilot-bridge** / **kimi-bridge**: Standalone SDK bridges built on sidecar-kit.
- **abp-shim-***: Drop-in SDK client replacements (openai, claude, gemini, codex, kimi, copilot).
- **abp-*-sdk**: Vendor SDK adapter microcrates (claude, codex, openai, gemini, kimi, copilot).

### Sidecar Protocol (JSONL over stdio)

Sidecars are external processes (Node/Python/etc.) that speak the JSONL protocol:

1. Sidecar sends `hello` envelope (MUST be first line) with backend identity + capabilities
2. Control plane sends `run` with a `WorkOrder`
3. Sidecar streams `event` envelopes with `AgentEvent`s
4. Sidecar concludes with `final` (containing a `Receipt`) or `fatal`

All envelopes use `ref_id` to correlate with the `run.id`. V0.1 assumes one run at a time.

Example sidecars live in `hosts/` (node, python, claude, codex, copilot, kimi, gemini).

### Receipt Hashing Gotcha

`receipt_hash()` in `abp-core` sets `receipt_sha256` to `null` before hashing to prevent self-referential hash. Always use `receipt.with_hash()` rather than computing manually.

### Execution Modes

- **Passthrough**: ABP acts as observer/recorder only — no request rewriting, stream is bitwise-equivalent after removing ABP framing.
- **Mapped** (default): Full dialect translation between different agent dialects.

Set via `work_order.config.vendor.abp.mode`.

## Key Patterns

- All serde enums use `#[serde(rename_all = "snake_case")]`. Contract types use `#[serde(tag = "type")]` for `AgentEventKind`, but the protocol envelope uses `#[serde(tag = "t")]`.
- `BTreeMap` is used throughout for deterministic serialization (important for canonical JSON hashing).
- Workspace staging always excludes `.git` directory and auto-initializes a fresh git repo with a "baseline" commit.
- Tracing targets: `abp.sidecar.stderr`, `abp.runtime`, `abp.workspace`, `abp.sidecar`.

## Hosts Directory (Sidecar Examples)

- `hosts/node/` — Minimal Node.js sidecar example
- `hosts/python/` — Minimal Python sidecar example
- `hosts/claude/` — Claude-oriented sidecar with pluggable adapter module (has its own test suite)
- `hosts/codex/` — Codex-oriented sidecar with passthrough/mapped modes
- `hosts/gemini/` — Gemini sidecar with Claude-to-Gemini mapping (`mapper.js`, `capabilities.js`)
- `hosts/copilot/` — GitHub Copilot sidecar with agent protocol adapter
- `hosts/kimi/` — Kimi sidecar with SDK-first adapter and CLI fallback

## License

Dual-licensed MIT OR Apache-2.0.
