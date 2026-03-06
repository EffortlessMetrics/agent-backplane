# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Agent Backplane (ABP) is a **translation layer between agent SDKs**. It provides vendor-agnostic SDK shims that map each vendor's surface area onto a stable internal contract, then routes work orders to any backend (OpenAI, Anthropic, Gemini, Kimi, Copilot, local models) via a projection matrix. The workspace contains **55 crates** — contract types, sidecar protocol, SDK shims, IR translators, bridge crates, and a CLI + HTTP daemon.

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
cargo run -p xtask -- gate --check             # Full quality gate (CI-parity)
cargo run -p xtask -- check                    # Gate + run tests + doc-tests
```

The CLI must be run from the repo root for sidecar backends (they resolve `hosts/` scripts relative to CWD).

Enable debug logging with `--debug` flag on the CLI or `RUST_LOG=abp=debug`.

## Developer Workflow (Enforced)

See [`DEVEX.md`](DEVEX.md) for the full enforcement model (hooks, gate, CI parity).

**Quick reference:**
- One-time setup: `cargo xtask setup`
- Pre-commit hook auto-formats and fixes clippy issues
- Pre-push hook runs `cargo xtask gate --check` (blocks on failure)
- CI runs the same gate, so local push success = CI success
- **Agents: never use `--no-verify` unless the human operator explicitly instructs you to.**

## xtask Subcommands

| Subcommand | Purpose | Key Flags |
|------------|---------|-----------|
| `setup` | One-time: sets `core.hooksPath=.githooks`, chmod +x on Unix | — |
| `schema` | Generate JSON schemas to `contracts/schemas/` | — |
| `check` | Gate + run tests + doc-tests | — |
| `coverage` | Run `cargo tarpaulin` with project config | — |
| `lint` | Check formatting + clippy (non-mutating) | — |
| `lint-fix` | Auto-format + best-effort clippy fix | `--check` (non-mutating), `--no-clippy` |
| `gate` | Pre-push quality gate (no test execution) | `--check` (strict/CI-parity mode) |
| `release-check` | Verify release readiness (versions, required fields, README presence, dry-run packaging) | — |
| `docs` | Build rustdoc for all crates | `--open` |
| `list-crates` | Print all workspace crate names | — |
| `audit` | Check required Cargo.toml fields, version consistency, and unused dependencies | — |
| `stats` | Print workspace statistics (crate count, LOC, test count) | — |

## Workspace Modes

- **Staged** (default): ABP copies the project into a temp directory (with glob filtering), auto-initializes git, and captures diffs after the agent run.
- **PassThrough**: No workspace staging — the agent operates directly on the original directory.

Set via `--workspace-mode PassThrough|Staged` CLI flag or `work_order.workspace.mode`.

## Execution Lanes

- **PatchFirst** (default): Agent produces a patch; ABP applies it to the workspace.
- **WorkspaceFirst**: Agent works directly in the staged workspace; ABP captures the diff afterward.

Set via `--lane PatchFirst|WorkspaceFirst` CLI flag or `work_order.lane`.

## CLI Flags

Key runtime flags for `abp run`:

- `--max-budget-usd <N>` — Cap spend per run (wired into runtime budget tracker)
- `--max-turns <N>` — Limit agent turn count
- `--workspace-mode PassThrough|Staged` — Workspace staging strategy
- `--lane PatchFirst|WorkspaceFirst` — Execution lane strategy
- `--model <name>` — Override model selection
- `--param key=value` — Pass vendor-specific parameters (repeatable)
- `--env KEY=VALUE` — Set environment variables for sidecar (repeatable)
- `--policy <path>` — Path to a policy profile JSON file to load
- `--output <path>` — Write the receipt to this file path
- `--out <path>` — Where to write the receipt (defaults to `.agent-backplane/receipts/<run_id>.json`)
- `--events <path>` — Write streamed events as JSONL to this file
- `--stream` — Stream events to stdout as they arrive (implies no buffering)
- `--timeout <secs>` — Timeout in seconds for the entire run
- `--retry <n>` — Number of times to retry on failure (default: 0)
- `--fallback <backend>` — Fallback backend name if the primary fails
- `--json` — Print JSON instead of pretty output
- `--include <glob>` — Include glob(s) relative to root (repeatable)
- `--exclude <glob>` — Exclude glob(s) relative to root (repeatable)
- `--root <path>` — Workspace root (default: `.`)

### `config` Sub-commands

- `abp config check [--config <path>]` — Check (load and validate) the configuration file
- `abp config show [--format toml|json]` — Display the current effective configuration
- `abp config validate [--config <path>]` — Validate a configuration file (alias for check)
- `abp config diff <file1> <file2>` — Diff two configuration files and show changes

### `receipt` Sub-commands

- `abp receipt verify <file>` — Verify a receipt file's hash integrity
- `abp receipt diff <file1> <file2>` — Diff two receipt files and show changes

## CI Workflows

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | Push to `main`, PRs | Format, lint, test, docs, coverage, cargo-deny, schema generation |
| `mutants.yml` | Manual dispatch | Mutation testing via `cargo-mutants` |
| `release.yml` | Tag push (`v*`) | Automated release pipeline |

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
  │             copilot-bridge                   abp-ratelimit ──┐
  │             kimi-bridge                      abp-retry ─────┤
  ├── abp-sidecar-proto              (abp-ratelimit and abp-retry
  ├── abp-sidecar-sdk                feed into abp-runtime layer)
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
