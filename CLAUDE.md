# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Agent Backplane (ABP) is a **translation layer between agent SDKs**. It provides vendor-agnostic SDK shims that map each vendor's surface area onto a stable internal contract, then routes work orders to any backend (OpenAI, Anthropic, Gemini, local models) via a projection matrix. This is a v0.1 scaffold — the contract types and sidecar protocol are stable, but real vendor SDK adapters and the projection matrix are not yet implemented.

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
cargo run -p abp-cli -- backends               # List available backends
```

The CLI must be run from the repo root for sidecar backends (they resolve `hosts/` scripts relative to CWD).

Enable debug logging with `--debug` flag on the CLI or `RUST_LOG=abp=debug`.

## Architecture

### Core Principle

**The contract is the product.** `abp-core` defines `WorkOrder`, `Receipt`, `AgentEvent`, capabilities, and policy types. Everything else exists to faithfully map SDK semantics into that contract and back out.

### Crate Dependency Hierarchy (bottom-up)

```
abp-glob ──────────┐
                    ├── abp-policy ──────────┐
abp-core ──────────┤                         │
  │                └── abp-workspace ────────┤
  │                                          │
abp-protocol ─── abp-host ─── abp-integrations ─── abp-runtime ─── abp-cli
```

- **abp-core**: Stable contract types only. If you take one dep, take this one. Contains `CONTRACT_VERSION = "abp/v0.1"`.
- **abp-protocol**: JSONL wire format (`Envelope` enum tagged with `#[serde(tag = "t")]` — the discriminator field is `t`, not `type`).
- **abp-host**: Spawns sidecar processes, handles JSONL handshake + event streaming over stdio.
- **abp-glob**: Include/exclude glob compilation using `globset`. Used by both workspace staging and policy.
- **abp-workspace**: Staged workspace creation (temp dir copy with glob filtering), auto-initializes git for meaningful diffs.
- **abp-policy**: Compiles `PolicyProfile` into `PolicyEngine` with tool/read/write allow/deny checks via globs.
- **abp-integrations**: `Backend` trait + `MockBackend` + `SidecarBackend`. Backends stream `AgentEvent`s via `mpsc::Sender` and return a `Receipt`.
- **abp-runtime**: Orchestration — prepares workspace, selects backend, multiplexes event streams, produces canonical hashed receipt.
- **abp-cli**: `abp` binary with `run` and `backends` subcommands. Registers built-in sidecars (node, python, claude).
- **abp-daemon**: Stub for future HTTP control-plane API.

### Sidecar Protocol (JSONL over stdio)

Sidecars are external processes (Node/Python/etc.) that speak the JSONL protocol:

1. Sidecar sends `hello` envelope (MUST be first line) with backend identity + capabilities
2. Control plane sends `run` with a `WorkOrder`
3. Sidecar streams `event` envelopes with `AgentEvent`s
4. Sidecar concludes with `final` (containing a `Receipt`) or `fatal`

All envelopes use `ref_id` to correlate with the `run.id`. V0.1 assumes one run at a time.

Example sidecars live in `hosts/` (node, python, claude, gemini).

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
- `hosts/gemini/` — Gemini sidecar with Claude-to-Gemini mapping (`mapper.js`, `capabilities.js`)

## License

Dual-licensed MIT OR Apache-2.0.
