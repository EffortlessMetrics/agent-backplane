# Architecture

> Contract version: `abp/v0.1`

This document describes the internal architecture of Agent Backplane (ABP), a
translation layer between agent SDKs. ABP maps each vendor's surface area onto
a stable internal contract, then routes work orders to any backend via a
projection matrix.

**Core principle: the contract is the product.** `abp-core` defines the canonical
types (`WorkOrder`, `Receipt`, `AgentEvent`, capabilities, policy). Everything
else exists to faithfully translate SDK semantics into that contract and back out.

---

## Table of Contents

- [System Overview](#system-overview)
- [Crate Hierarchy](#crate-hierarchy)
- [Message Flow](#message-flow)
- [Sidecar Lifecycle](#sidecar-lifecycle)
- [Workspace Staging](#workspace-staging)
- [Policy Engine](#policy-engine)
- [Receipt Hashing and Verification](#receipt-hashing-and-verification)
- [Projection Matrix and Dialect Translation](#projection-matrix-and-dialect-translation)
- [Execution Modes](#execution-modes)
- [Capability Negotiation](#capability-negotiation)
- [IR Layer](#ir-layer)

---

## System Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Caller / SDK Shim                              │
│   (OpenAI SDK, Anthropic SDK, LangChain, Vercel AI, …)                 │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │  WorkOrder
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           abp-cli / abp-daemon                          │
│                                                                         │
│   • Parse CLI args / HTTP request                                       │
│   • Load config (backplane.toml)                                        │
│   • Register backends (mock, sidecar:node, sidecar:claude, …)          │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                            abp-runtime                                  │
│                                                                         │
│   1. Resolve backend from registry                                      │
│   2. Check capability requirements                                      │
│   3. Prepare workspace (staged copy + git init)                         │
│   4. Compile policy (glob allow/deny rules)                             │
│   5. Dispatch WorkOrder to backend                                      │
│   6. Multiplex AgentEvent stream                                        │
│   7. Collect receipt + attach verification (git diff/status)            │
│   8. Compute receipt hash (SHA-256)                                     │
│                                                                         │
│   Returns: RunHandle { run_id, events, receipt }                        │
└───────────┬───────────────────┬──────────────────┬──────────────────────┘
            │                   │                  │
            ▼                   ▼                  ▼
   ┌────────────────┐  ┌───────────────┐  ┌───────────────────┐
   │  abp-workspace  │  │  abp-policy   │  │  abp-integrations │
   │                 │  │               │  │                   │
   │  Stage files    │  │  Compile      │  │  Backend trait    │
   │  Git init       │  │  allow/deny   │  │  MockBackend      │
   │  Diff/status    │  │  glob rules   │  │  SidecarBackend   │
   └────────────────┘  └───────────────┘  └─────────┬─────────┘
                                                     │
                                  ┌──────────────────┼──────────────────┐
                                  │                  │                  │
                                  ▼                  ▼                  ▼
                           ┌────────────┐    ┌────────────┐    ┌──────────────┐
                           │ MockBackend│    │  abp-host  │    │ claude-bridge│
                           │ (in-proc)  │    │  (sidecar) │    │  (sidecar)   │
                           └────────────┘    └──────┬─────┘    └──────┬───────┘
                                                    │                 │
                                                    ▼                 ▼
                                             ┌────────────┐   ┌──────────────┐
                                             │ sidecar-kit│   │ sidecar-kit  │
                                             │  (JSONL)   │   │  (JSONL)     │
                                             └──────┬─────┘   └──────┬───────┘
                                                    │                │
                                                    ▼                ▼
                                          ┌──────────────────────────────────┐
                                          │     External Sidecar Processes   │
                                          │   hosts/node  hosts/claude       │
                                          │   hosts/python hosts/gemini      │
                                          │   hosts/copilot hosts/kimi       │
                                          └──────────────────────────────────┘
```

---

## Crate Hierarchy

The project uses a micro-crate architecture where each crate has a single clear
purpose and one primary dependency edge. This keeps compile units small and makes
it possible for downstream consumers to depend on only what they need.

```
abp-glob ──────────┐
                    ├── abp-policy ──────────┐
abp-core ──────────┤                         │
  │   │            └── abp-workspace ────────┤
  │   │                      │               │
  │   ├── abp-dialect ─── abp-mapping        │
  │   │                                      │
  │   ├── abp-error                          │
  │   │                                      │
  │   ├── abp-capability ─── abp-projection  │
  │   │                                      │
  │   ├── abp-emulation                      │
  │   │                                      │
  │   ├── abp-receipt                        │
  │   │     │                                │
  │   │     └── abp-telemetry                │
  │   │                                      │
  │   └── abp-config                         │
  │                                          │
abp-protocol ─── abp-host ─── abp-backend-core ─── abp-backend-mock
  │                  │              │                abp-backend-sidecar
  │              sidecar-kit        │
  │                  │         abp-integrations ─── abp-runtime ─── abp-cli
  │             claude-bridge                           │             │
  │                                                 abp-stream   abp-daemon
  └── abp-sidecar-proto

SDK shims (drop-in client replacements):
  abp-shim-openai, abp-shim-claude, abp-shim-gemini

Supporting crates:
  abp-git           Standalone git helpers (init, status, diff)
  abp-sidecar-sdk   Vendor SDK registration helpers

Vendor SDK microcrates (abp-claude-sdk, abp-codex-sdk, abp-openai-sdk,
abp-gemini-sdk, abp-kimi-sdk, abp-copilot-sdk) depend on abp-core +
abp-runtime and register via abp-sidecar-sdk.
```

### abp-core — Contract Types

The foundational crate. If you take one dependency, take this one.

- `WorkOrder`: the unit of work sent to a backend. Contains `id`, `task`
  (human intent), `lane` (execution strategy), `workspace`, `context`,
  `policy`, capability `requirements`, and `config`.
- `Receipt`: structured execution record with timing metadata, backend
  identity, usage data, event trace, artifacts, workspace verification
  (git diff/status), and an integrity hash (`receipt_sha256`).
- `AgentEvent` / `AgentEventKind`: timestamped events emitted during a run
  (`RunStarted`, `AssistantDelta`, `ToolCall`, `ToolResult`, `FileChanged`,
  `CommandExecuted`, `Warning`, `Error`, `RunCompleted`, etc.).
- `BackendIdentity`: stable identifier for the backend that handled a request.
- `CapabilityManifest` / `CapabilityRequirements`: what a backend can do and
  what a work order needs.
- `PolicyProfile`: allow/deny rules for tools, file reads/writes, and network.
- `ExecutionMode`: `Passthrough` (lossless) vs `Mapped` (dialect translation).
- `ExecutionLane`: `PatchFirst` (propose changes) vs `WorkspaceFirst` (mutate directly).
- `CONTRACT_VERSION = "abp/v0.1"`: embedded in all wire messages and receipts.
- **IR module** (`abp_core::ir`): vendor-neutral intermediate representation
  for cross-dialect message normalization. See [IR Layer](#ir-layer).

Uses `BTreeMap` throughout for deterministic serialization (critical for
canonical JSON hashing). All serde enums use `#[serde(rename_all = "snake_case")]`.

### abp-protocol — Wire Format

JSONL (newline-delimited JSON) encoding for sidecar communication.

- `Envelope`: the discriminated union for all wire messages, tagged with
  `#[serde(tag = "t")]` — the field name is `t`, **not** `type`.
- `JsonlCodec`: stateless encoder/decoder with `encode()`, `decode()`, and
  `decode_stream()`.
- Version helpers: `parse_version()` and `is_compatible_version()` for
  major-version compatibility checks.

Designed to be trivially implementable in any language (just read/write JSON lines).

### abp-glob — Glob Compilation

Compiles include/exclude glob patterns using the `globset` crate. Produces
`IncludeExcludeGlobs` that evaluate whether a path or name matches.

Used by both `abp-workspace` (file staging filters) and `abp-policy` (tool/path
allow/deny rules).

### abp-workspace — Staged Workspaces

Creates isolated workspace copies for safe agent execution.

- `WorkspaceManager::prepare()`: given a `WorkspaceSpec`, either passes through
  the directory as-is or creates a staged copy in a temp directory.
- Staging excludes `.git` by default, applies include/exclude glob filters,
  and auto-initializes a fresh git repo with a "baseline" commit.
- `git_status()` / `git_diff()`: capture workspace changes for receipt
  verification.

### abp-policy — Policy Compilation

Compiles a `PolicyProfile` into a `PolicyEngine` with pre-compiled glob matchers.

- `PolicyEngine::can_use_tool(name)` → `Decision`
- `PolicyEngine::can_read_path(path)` → `Decision`
- `PolicyEngine::can_write_path(path)` → `Decision`

Deny rules always override allow rules. An empty profile allows everything.
In v0.1, the policy engine is a utility crate; enforcement happens in adapters
and sidecars.

### sidecar-kit — Low-Level Sidecar I/O

A value-based transport layer with no dependency on `abp-core` types (uses
`serde_json::Value` throughout), making it reusable for any JSONL protocol.

- `SidecarClient`: spawn a process, perform JSONL handshake, parse hello.
- `SidecarProcess`: low-level process I/O (piped stdin/stdout/stderr).
- `Frame`: value-based discriminated union mirroring the protocol Envelope
  with additional transport-level types:
  - `Cancel { ref_id, reason }` — request work cancellation.
  - `Ping { seq }` / `Pong { seq }` — heartbeat for stall detection.
- `RawRun`: event stream + receipt oneshot + cancel token.
- `CancelToken`: async cancellation signal for graceful run termination.

### abp-host — Sidecar Supervision

Higher-level sidecar management built on `sidecar-kit`.

- `SidecarClient::spawn(spec)`: start child process, read `hello`, extract
  identity and capabilities.
- `SidecarClient::run(run_id, work_order)`: send `Run` envelope, start
  background event loop, return `SidecarRun` with event stream and receipt
  future.
- Error types wrap `ProtocolError` and add process-supervision concerns.

### abp-backend-core — Backend Trait

Defines the core `Backend` trait and capability helpers shared across all backend
implementations. Extracted so downstream crates can depend on the trait without
pulling in specific implementations.

### abp-backend-mock — Mock Backend

Simple test backend that emits a few events and returns a receipt. Reports
`Streaming: Native`, `ToolRead: Emulated`. Useful for integration tests and
development without any external API keys.

### abp-backend-sidecar — Sidecar Backend Adapter

Generic wrapper that delegates work to an external sidecar process via
`abp-host`. Translates the `Backend` trait into JSONL protocol I/O.

### abp-integrations — Backend Registry

Re-exports `abp-backend-core`, `abp-backend-mock`, and `abp-backend-sidecar`
under a single crate. Provides the `BackendRegistry` for runtime lookup.

### abp-dialect — Dialect Detection

Detects and validates SDK dialects from request metadata. Defines the `Dialect`
enum (`OpenAi`, `Claude`, `Gemini`, `Codex`, `Kimi`, `Copilot`) and provides:

- `DialectDetector`: inspects JSON values to identify the originating dialect.
- `DialectValidator`: validates a JSON value conforms to a specific dialect.
- `Dialect::label()`, `Dialect::all()` for iteration and display.

### abp-error — Unified Error Taxonomy

Stable, machine-readable error codes for all ABP errors. Every error carries
an `ErrorCode` (SCREAMING_SNAKE_CASE string tag), a human-readable message,
optional cause chain, and structured context. 20 error codes across 10
categories. See [error_codes.md](error_codes.md) for the full reference.

### abp-mapping — Cross-Dialect Mapping Validation

Validates feature translation fidelity between dialect pairs. Provides:

- `MappingRule`: source/target dialect + feature + `Fidelity` level.
- `Fidelity`: `Lossless`, `LossyLabeled { warning }`, `Unsupported { reason }`.
- `MappingRegistry`: stores and looks up mapping rules.
- `MappingMatrix`: boolean compatibility matrix derived from the registry.
- `known_rules()`: pre-populated registry for tool_use, streaming, thinking,
  and image_input across OpenAI, Claude, Gemini, and Codex.

### abp-capability — Capability Negotiation

Compares `CapabilityManifest` against `CapabilityRequirements` to produce
structured negotiation results. See [capability_negotiation.md](capability_negotiation.md).

- `negotiate()` → `NegotiationResult` (native / emulatable / unsupported buckets)
- `generate_report()` → `CompatibilityReport` (human-readable summary)

### abp-emulation — Labeled Emulation

Applies emulation strategies for capabilities not natively supported. Never
silently degrades — every emulation is explicitly recorded.

- `EmulationEngine`: applies strategies to `IrConversation`.
- `EmulationStrategy`: `SystemPromptInjection`, `PostProcessing`, `Disabled`.
- `EmulationConfig`: per-capability strategy overrides.

### abp-receipt — Receipt Building, Chaining, and Diffing

Extended receipt operations beyond core hashing:

- `ReceiptBuilder`: fluent builder for constructing receipts.
- `ReceiptChain`: append-only, ordered chain of receipts with integrity checks.
- `diff_receipts()`: structured diff between two receipts.
- `canonicalize()`, `compute_hash()`, `verify_hash()`: hash utilities.

### abp-telemetry — Metrics Collection

Run-level metrics collection and aggregation:

- `RunMetrics`: per-run timing and usage data.
- `MetricsCollector`: thread-safe collector with `record()` and `summary()`.
- `TelemetrySpan`: structured span with attributes, emitted via tracing.
- `TelemetryExporter` trait + `JsonExporter`: pluggable export.

### abp-git — Git Helpers

Standalone git operations for workspace management:

- `ensure_git_repo()`: initialize a git repo with a baseline commit.
- `git_status()` / `git_diff()`: capture workspace changes.

### abp-sidecar-sdk — Vendor Registration Helpers

Shared registration helpers that vendor SDK microcrates use to register their
sidecar hosts with the runtime. Depends on `abp-host`, `abp-integrations`, and
`abp-runtime`.

### Vendor SDK Microcrates

Each vendor has a dedicated SDK adapter crate:

- `abp-claude-sdk` — Anthropic Claude (Messages API)
- `abp-codex-sdk` — OpenAI Codex (Responses API)
- `abp-openai-sdk` — OpenAI Chat Completions
- `abp-gemini-sdk` — Google Gemini (generateContent)
- `abp-kimi-sdk` — Moonshot Kimi (Chat Completions)
- `abp-copilot-sdk` — GitHub Copilot (scaffold)

All implement the dialect pattern: model name mapping, capability manifest,
`map_work_order()`, `map_response()`, and tool definition translation.

### claude-bridge — Claude Sidecar Bridge

Specialized bridge for the Claude sidecar. Spawns a Node.js host process
(`hosts/claude/`), handles the JSONL protocol, and converts between ABP types
and the Claude-specific wire format.

### abp-projection — Backend Selection

Projection matrix that routes work orders to the best-fit backend based on
capability negotiation. Scores each registered backend against a work order's
requirements and selects the optimal match.

### abp-stream — Event Stream Processing

Filters, transforms, and multiplexes agent event streams. Provides custom
predicates for event filtering and transformation pipelines for stream
processing.

### abp-config — Configuration

Loads, validates, and merges TOML configuration files (`backplane.toml`).
Supports layered configuration with advisory warnings for deprecated or
unrecognized keys.

### abp-sidecar-proto — Sidecar Protocol Handler

Sidecar-side utilities for implementing services that speak ABP's JSONL
protocol. Complements the host-side `abp-host` and `sidecar-kit` crates by
providing helpers for the sidecar process itself.

### SDK Shims

Drop-in SDK client replacements that transparently route through ABP:

- `abp-shim-openai` — OpenAI SDK shim
- `abp-shim-claude` — Anthropic Claude SDK shim
- `abp-shim-gemini` — Gemini SDK shim

These shims allow existing code that uses vendor SDKs to route through ABP's
intermediate representation without code changes.

### abp-runtime — Orchestration

The central orchestrator that ties everything together.

- `Runtime::run_streaming(backend_name, work_order)` → `Result<RunHandle>`
- `RunHandle` contains: `run_id`, `events` (stream), `receipt` (join handle).

See [Message Flow](#message-flow) for the detailed sequence.

### abp-cli — CLI Binary

The `abp` binary with subcommands:

- `run`: execute a work order against a named backend.
- `backends`: list registered backends and their capabilities.
- `validate`: validate a JSON file as a WorkOrder or Receipt.
- `schema`: print a JSON schema to stdout.
- `inspect`: inspect a receipt file and verify its hash.
- `config check`: load and validate a TOML configuration file.
- `receipt verify`: verify a receipt file's hash integrity.
- `receipt diff`: structured diff between two receipt files.

Registers built-in sidecar backends (node, python, claude, copilot, kimi, gemini).
Must be run from the repo root for sidecar backends (they resolve `hosts/`
scripts relative to CWD).

### abp-daemon — HTTP Control Plane

HTTP API for programmatic access. Exposes routes for health, metrics, backends,
capabilities, configuration, validation, schema retrieval, run management
(submit, list, get, cancel, delete), receipt management, event streaming, and
WebSocket connections.

---

## Message Flow

The complete lifecycle of a work order from submission to receipt:

```
                    Caller
                      │
                      │  WorkOrder
                      ▼
              ┌───────────────┐
              │  abp-runtime  │
              │               │
              │  ① Resolve    │     BackendRegistry
              │    backend ───┼───► lookup by name
              │               │
              │  ② Capability │     WorkOrder.requirements
              │    pre-check ─┼───► vs Backend.capabilities()
              │               │     (skipped for sidecars; caps
              │               │      unknown until hello)
              │               │
              │  ③ Prepare    │     WorkspaceManager::prepare()
              │    workspace ─┼───► staged copy + git init
              │               │     OR pass-through
              │               │
              │  ④ Compile    │     PolicyEngine::new()
              │    policy ────┼───► glob rules compiled
              │               │
              │  ⑤ Dispatch   │
              │    to backend │
              └───────┬───────┘
                      │
         ┌────────────┴────────────┐
         │                         │
         ▼                         ▼
   ┌───────────┐           ┌──────────────┐
   │MockBackend│           │SidecarBackend│
   │           │           │              │
   │ emit      │           │ spawn child  │
   │ events    │           │ JSONL I/O    │
   │ return    │           │              │
   │ receipt   │           │ hello → run  │
   └─────┬─────┘           │ → events    │
         │                 │ → final     │
         │                 └──────┬───────┘
         │                        │
         └────────┬───────────────┘
                  │
                  ▼
          ┌───────────────┐
          │  abp-runtime  │
          │               │
          │  ⑥ Multiplex  │   events_tx → caller via ReceiverStream
          │    events     │   (trace collected on-the-fly)
          │               │
          │  ⑦ Fill       │   git_diff(), git_status()
          │    verification│   attached to receipt if missing
          │               │
          │  ⑧ Hash       │   receipt.with_hash()
          │    receipt    │   SHA-256 of canonical JSON
          │               │
          └───────┬───────┘
                  │
                  ▼
              RunHandle {
                run_id: Uuid,
                events: ReceiverStream<AgentEvent>,
                receipt: JoinHandle<Result<Receipt>>,
              }
                  │
                  ▼
                Caller
```

### Step-by-Step

1. **Backend resolution**: the runtime looks up the named backend in its
   `BackendRegistry`. Unknown names produce `RuntimeError::UnknownBackend`.

2. **Capability pre-check**: if the work order declares capability requirements,
   the runtime verifies them against the backend's `CapabilityManifest`. For
   sidecar backends this check is deferred until after the `hello` handshake
   (capabilities are unknown before then).

3. **Workspace preparation**: `WorkspaceManager::prepare()` inspects the
   `WorkspaceSpec`. In `Staged` mode it copies files to a temp directory
   (applying glob include/exclude filters, always skipping `.git`), then
   runs `git init` + initial commit to establish a baseline for diff tracking.
   In `PassThrough` mode it returns the original directory path unchanged.

4. **Policy compilation**: the `PolicyProfile` from the work order is compiled
   into a `PolicyEngine` with pre-built glob matchers. Compilation failure
   (e.g. invalid glob syntax) produces `RuntimeError::PolicyFailed`.

5. **Backend dispatch**: the runtime spawns an async task that calls
   `backend.run(run_id, work_order, events_tx)`. The backend streams
   `AgentEvent`s through the `mpsc::Sender` channel and eventually returns
   a `Receipt`.

6. **Event multiplexing**: events from the backend flow through a `tokio::select!`
   loop that forwards them to the caller's `ReceiverStream` and simultaneously
   records them in the receipt's trace.

7. **Verification**: after the backend completes, the runtime attaches workspace
   verification data (`git diff --no-color` and `git status --porcelain=v1`)
   to the receipt if not already present.

8. **Receipt hashing**: `receipt.with_hash()` computes a SHA-256 hash over the
   canonical JSON of the receipt (with `receipt_sha256` set to `null` first)
   and stores the result. See [Receipt Hashing](#receipt-hashing-and-verification).

---

## Sidecar Lifecycle

Sidecars are external processes (Node.js, Python, etc.) that speak the JSONL
protocol over stdio. The lifecycle has four phases:

```
  Control Plane (Rust)                          Sidecar Process
  ═══════════════════                           ════════════════

  ┌─────────────────┐                          ┌──────────────┐
  │ 1. SPAWN        │  fork/exec               │              │
  │    SidecarClient├─────────────────────────►│  Process      │
  │    ::spawn()    │                           │  starts up    │
  └────────┬────────┘                          └──────┬───────┘
           │                                          │
           │           ◄── stdout: hello ────────────│
           │                                          │
  ┌────────▼────────┐                                 │
  │ 2. HANDSHAKE    │                                 │
  │    Parse hello  │  Verify contract_version,       │
  │    Extract:     │  extract backend identity       │
  │    - identity   │  and capability manifest.       │
  │    - capabilities│                                │
  │    - mode       │                                 │
  └────────┬────────┘                                 │
           │                                          │
           │  stdin: run {id, work_order} ──────────►│
           │                                          │
  ┌────────▼────────┐                          ┌──────▼───────┐
  │ 3. RUN          │                          │ Execute task  │
  │    Event loop   │                          │               │
  │                 │  ◄── stdout: event ──────│ Stream events │
  │    Forward      │  ◄── stdout: event ──────│               │
  │    events to    │  ◄── stdout: event ──────│               │
  │    caller       │                          │               │
  │                 │                          │               │
  │  (ping/pong     │  ── stdin: ping ────────►│ Heartbeat     │
  │   heartbeat)    │  ◄── stdout: pong ───────│               │
  └────────┬────────┘                          └──────┬───────┘
           │                                          │
           │  ◄── stdout: final {receipt} ───────────│
           │       OR                                 │
           │  ◄── stdout: fatal {error} ─────────────│
           │                                          │
  ┌────────▼────────┐                          ┌──────▼───────┐
  │ 4. TEARDOWN     │                          │ Process exits │
  │    Collect      │                          │               │
  │    receipt      │                          └──────────────┘
  │    Kill child   │
  │    on drop      │
  └─────────────────┘
```

### Envelope Wire Format

All messages are newline-delimited JSON (JSONL) with the discriminator
field `t`:

```jsonl
{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"my-sidecar"},"capabilities":{"streaming":"native"},"mode":"mapped"}
{"t":"run","id":"550e8400-...","work_order":{...}}
{"t":"event","ref_id":"550e8400-...","event":{"ts":"2024-01-15T10:30:00Z","type":"assistant_delta","text":"Hello"}}
{"t":"final","ref_id":"550e8400-...","receipt":{...}}
```

### Protocol Rules

- The sidecar **MUST** send `hello` as its first stdout line.
- The `hello` envelope includes `contract_version` for version compatibility,
  `backend` identity, `capabilities` manifest, and optionally `mode`.
- All `event` and `final`/`fatal` envelopes include `ref_id` matching the
  `run.id` for correlation.
- The sidecar-kit transport layer adds `cancel`, `ping`, and `pong` frame
  types for cancellation and heartbeat. These are transport-level concerns
  and do not appear in the `abp-protocol` Envelope type.
- V0.1 assumes **one run at a time** per sidecar process.

### Version Negotiation

The protocol includes version compatibility checking:

- Version format: `"abp/vMAJOR.MINOR"` (e.g. `"abp/v0.1"`).
- `parse_version()`: extracts `(major, minor)` tuple.
- `is_compatible_version()`: versions are compatible if they share the same
  major version (e.g. `v0.1` and `v0.2` are compatible; `v0.x` and `v1.x`
  are not).

### Heartbeat / Stall Detection

The sidecar-kit layer implements a `Ping`/`Pong` heartbeat mechanism:

- The control plane periodically sends `Ping { seq }` frames.
- The sidecar responds with `Pong { seq }` frames.
- Missing pongs indicate a stalled sidecar, triggering timeout handling.

### Cancellation

The `Cancel { ref_id, reason }` frame allows the control plane to request
graceful cancellation of an in-progress run. The `CancelToken` struct provides
async signaling for cooperative cancellation.

### Error Handling

- **`fatal` envelope**: the sidecar explicitly signals an unrecoverable error.
  The control plane extracts the error message and produces a failed receipt.
- **Unexpected exit**: if the sidecar process exits before sending `final` or
  `fatal`, the runtime synthesizes a failed receipt with the exit code.
- **Protocol violations**: invalid JSON, wrong envelope ordering, or missing
  handshake produce `ProtocolError` / `HostError::Violation`.

---

## Workspace Staging

The workspace staging system creates isolated environments for agent execution,
ensuring that agents cannot corrupt the original working directory and enabling
diff-based verification of changes.

### Lifecycle

```
  WorkOrder.workspace
        │
        ▼
  ┌─────────────────┐
  │ WorkspaceManager │
  │   ::prepare()    │
  └────────┬────────┘
           │
     ┌─────┴──────┐
     │             │
     ▼             ▼
  PassThrough    Staged
  (return path   (create copy)
   as-is)            │
                     ▼
              ┌──────────────┐
              │ Walk source  │  Skip .git directory
              │ tree         │  Apply include/exclude globs
              └──────┬───────┘
                     │
                     ▼
              ┌──────────────┐
              │ Copy files   │  Preserve directory structure
              │ to temp dir  │  in system temp directory
              └──────┬───────┘
                     │
                     ▼
              ┌──────────────┐
              │ git init     │  Initialize fresh repo
              │ git add .    │  Stage all files
              │ git commit   │  "baseline" commit
              └──────┬───────┘
                     │
                     ▼
              PreparedWorkspace {
                root: PathBuf,   // temp dir path
                is_staged: true,
              }
                     │
           ┌─────────┴──────────┐
           │                    │
           ▼                    ▼
      Agent runs            After completion:
      in staged dir         git_status() → receipt.verification
                            git_diff()   → receipt.verification
```

### Glob Filtering

Workspace staging uses `IncludeExcludeGlobs` from `abp-glob`:

- **Include patterns**: if specified, only matching files are copied.
- **Exclude patterns**: matching files are always skipped.
- `.git` is unconditionally excluded to avoid copying repository metadata.

### Git Baseline

The auto-initialized git repository with a "baseline" commit enables
meaningful diffs. After the agent completes, the runtime captures:

- `git status --porcelain=v1`: list of changed/added/deleted files.
- `git diff --no-color`: full unified diff of all changes.

These are attached to the receipt's `verification` field.

---

## Policy Engine

The policy engine compiles declarative allow/deny rules into fast glob
matchers for runtime evaluation.

### PolicyProfile Fields

| Field | Type | Purpose |
|-------|------|---------|
| `allowed_tools` | `Vec<String>` | Glob patterns for permitted tool names |
| `disallowed_tools` | `Vec<String>` | Glob patterns for denied tool names |
| `deny_read` | `Vec<String>` | Glob patterns for paths the agent cannot read |
| `deny_write` | `Vec<String>` | Glob patterns for paths the agent cannot write |
| `allow_network` | `Vec<String>` | Glob patterns for permitted network targets |
| `deny_network` | `Vec<String>` | Glob patterns for denied network targets |
| `require_approval_for` | `Vec<String>` | Patterns requiring human approval |

### Evaluation Rules

- **Deny overrides allow**: if a path matches both an allow and a deny rule,
  the deny rule wins.
- **Empty profile = allow all**: if no rules are specified, everything is
  permitted.
- `PolicyEngine::can_use_tool(name)` → `Decision` (Allow / Deny)
- `PolicyEngine::can_read_path(path)` → `Decision` (Allow / Deny)
- `PolicyEngine::can_write_path(path)` → `Decision` (Allow / Deny)

### Enforcement

In v0.1 the policy engine is a utility crate. Enforcement happens at the
adapter / sidecar level — the host process or sidecar inspects policy decisions
before allowing tool invocations or file operations.

---

## Receipt Hashing and Verification

Receipts are the auditable execution record. Every receipt includes a SHA-256
integrity hash that enables tamper detection.

### Hashing Process

```
  Receipt (all fields populated)
       │
       ▼
  ┌────────────────────────┐
  │ Set receipt_sha256      │   Nullify the hash field to
  │    = null               │   prevent self-referential hash.
  └──────────┬─────────────┘
             │
             ▼
  ┌────────────────────────┐
  │ Serialize to canonical  │   Deterministic JSON via BTreeMap
  │ JSON                    │   (sorted keys, no whitespace
  └──────────┬─────────────┘    variations).
             │
             ▼
  ┌────────────────────────┐
  │ SHA-256 hash of the     │
  │ canonical JSON bytes    │
  └──────────┬─────────────┘
             │
             ▼
  ┌────────────────────────┐
  │ Store hash in           │
  │ receipt.receipt_sha256  │
  └────────────────────────┘
```

### API

```rust
// Low-level: compute hash for a receipt
receipt_hash(&receipt) -> Result<String, ContractError>

// High-level: hash and populate in one step (preferred)
receipt.with_hash() -> Result<Receipt, ContractError>
```

**Critical invariant:** `receipt_hash()` sets `receipt_sha256` to `null`
before serializing. This prevents the hash from being included in its own
input. Always call `.with_hash()` as the **last** mutation on a receipt.

### Verification

`validate_receipt(&receipt)` checks:

- All required fields are present and non-empty.
- `contract_version` matches `CONTRACT_VERSION`.
- `started_at` ≤ `finished_at` (no clock inversion).
- `receipt_sha256` matches a freshly recomputed hash.
- `backend.id` is non-empty.

The validator accumulates **all** errors rather than short-circuiting, returning
`Result<(), Vec<ValidationError>>`.

### Deterministic Serialization

The use of `BTreeMap` (not `HashMap`) throughout the contract types ensures
that JSON serialization produces identical output regardless of insertion order.
This is essential for canonical hashing — the same receipt must always produce
the same hash.

---

## Projection Matrix and Dialect Translation

ABP sits between SDK **dialects** (how a caller speaks) and backend **engines**
(what actually executes). The intersection determines the execution mode:

```
                       ┌───────────────────────────────────────────┐
                       │              Backend Engine                │
                       ├──────────┬──────────┬──────────┬──────────┤
                       │  Claude  │  Gemini  │  Codex   │  Kimi    │
    ┌──────────────────┼──────────┼──────────┼──────────┼──────────┤
    │ Claude-style     │PASSTHRU  │ MAPPED   │ MAPPED   │ MAPPED   │
    │ dialect          │ lossless │ lossy    │ lossy    │ lossy    │
  D ├──────────────────┼──────────┼──────────┼──────────┼──────────┤
  i │ Codex-style      │ MAPPED   │ MAPPED   │PASSTHRU  │ MAPPED   │
  a │ dialect          │ lossy    │ lossy    │ lossless │ lossy    │
  l ├──────────────────┼──────────┼──────────┼──────────┼──────────┤
  e │ Gemini-style     │ MAPPED   │PASSTHRU  │ MAPPED   │ MAPPED   │
  c │ dialect          │ lossy    │ lossless │ lossy    │ lossy    │
  t └──────────────────┴──────────┴──────────┴──────────┴──────────┘
```

### Non-Isomorphic Concepts

SDKs differ fundamentally in how they represent:

| Concept | Claude | OpenAI/Codex | Gemini |
|---------|--------|--------------|--------|
| Tool calls | Tool use blocks with `tool_use_id` | Function calling with `function_call` | Function declarations |
| Streaming | Content blocks (SSE) | Chunk deltas (SSE) | Deltas (SSE) |
| Structured output | Tool use constraint | JSON mode | Grounded generation |
| Extended thinking | Native | Not available | Not available |
| Session state | Limited | Threads API | Context caching |

### Design Philosophy

ABP does **not** pretend these are identical. Instead:

1. **Express capabilities precisely**: each backend advertises what it
   supports and at what level (native, emulated, restricted, unsupported).
2. **Emulate when possible**: translate tool call formats, aggregate stream
   deltas, adapt structured output mechanisms.
3. **Fail loudly when impossible**: if a required capability cannot be
   provided, return a typed error **before** execution begins.

### Translation Flow (Mapped Mode)

```
  Dialect Request
       │
       ▼
  ┌────────────────┐
  │ Dialect Parser  │   Parse vendor-specific format
  └───────┬────────┘
          │
          ▼
  ┌────────────────┐
  │ ABP IR          │   Internal representation
  │ (WorkOrder)     │   (vendor-neutral)
  └───────┬────────┘
          │
          ▼
  ┌────────────────┐
  │ Capability      │   Check backend supports all
  │ Validation      │   required features
  └───────┬────────┘
          │
     ┌────┴────┐
     │         │
   FAIL      PASS
   (early     │
    error)    ▼
          ┌────────────────┐
          │ Engine Lowering │   Convert to target format
          └───────┬────────┘
                  │
                  ▼
            Backend Engine
```

### Passthrough Guarantees

When dialect matches engine natively:

- **Request fidelity**: payload sent to backend is byte-identical to input.
- **Response fidelity**: stream events match vendor SDK exactly.
- **No injection**: ABP never injects content into the stream.
- **Out-of-band only**: workspace staging, policy enforcement, receipts,
  and observability are all out-of-band additions.

---

## Execution Modes

### Passthrough

ABP acts as an observer and recorder. The request is wrapped but not
transformed; the response stream is observed but not altered.

```
  Original Request → ABP Envelope → Backend → Raw Response → ABP Receipt → Caller
```

Set via `work_order.config.vendor.abp.mode = "passthrough"`.

Used when:
- Dialect matches engine natively.
- No capability translation required.
- Testing against vendor SDK baseline.

### Mapped (Default)

Full dialect translation between different agent dialects. ABP parses the
request into its internal representation, validates capabilities, and lowers
to the target engine format.

Set via `work_order.config.vendor.abp.mode = "mapped"` (or omitted; mapped
is the default).

Key property: **mapped is explicitly lossy**. Capability mismatches fail
early with typed errors rather than silently degrading.

---

## Capability Negotiation

Every backend advertises a `CapabilityManifest` — a map from `Capability`
to `SupportLevel`. Work orders carry `CapabilityRequirements` that the
runtime checks before dispatch.

### Support Levels

| Level | Meaning |
|-------|---------|
| `Native` | First-class support, no translation needed |
| `Emulated` | Supported via ABP translation with acceptable fidelity |
| `Restricted` | Supported but limited by policy or environment |
| `Unsupported` | Cannot be provided |

### Matching

A `MinSupport` threshold specifies the minimum acceptable level:

- `MinSupport::Native` → satisfied only by `Native`.
- `MinSupport::Emulated` → satisfied by `Native`, `Emulated`, or `Restricted`.

All requirements must be satisfied for dispatch to proceed. For sidecar
backends, capabilities are only known after the `hello` handshake, so the
check is deferred.

### Capabilities

The full list of capability variants is documented in
[capabilities.md](capabilities.md). Key categories include:

- **Streaming**: incremental event delivery.
- **Tools**: file read/write/edit, bash, glob, grep, web search/fetch.
- **Hooks**: pre/post tool use callbacks.
- **Sessions**: resume and fork.
- **Structured output**: JSON schema enforcement.
- **MCP**: Model Context Protocol client/server.

See [capability_negotiation.md](capability_negotiation.md) for the full
negotiation flow, emulation strategies, and decision tree.

---

## IR Layer

The Intermediate Representation (IR) is the vendor-neutral format that sits
between dialect-specific request types and the internal contract. Dialect
adapters **lower** vendor-specific formats into the IR and **raise** the IR
back into the target dialect format.

**Source:** `crates/abp-core/src/ir.rs`

### Core Types

```
  IrConversation
  ├── messages: Vec<IrMessage>
  │   ├── role: IrRole (System | User | Assistant | Tool)
  │   ├── content: Vec<IrContentBlock>
  │   │   ├── Text { text }
  │   │   ├── Image { media_type, data }
  │   │   ├── ToolUse { id, name, input }
  │   │   ├── ToolResult { tool_use_id, content, is_error }
  │   │   └── Thinking { text }
  │   └── metadata: BTreeMap<String, Value>  (vendor-opaque, carried through)
  ├── tools: Vec<IrToolDefinition>
  │   ├── name, description, parameters_schema
  └── usage: Option<IrUsage>
      ├── input_tokens, output_tokens
      ├── cache_creation_tokens, cache_read_tokens
      └── total_tokens
```

### Translation Flow

```
  Vendor Request (e.g. OpenAIRequest)
        │
        │  lowering::to_ir()
        ▼
  IrConversation  ◄── vendor-neutral
        │
        │  (optional) EmulationEngine::apply()
        │  (optional) capability checks
        ▼
  IrConversation  (possibly mutated)
        │
        │  lowering::from_ir()
        ▼
  Target Format (e.g. ClaudeRequest)
```

Each vendor SDK crate provides `lowering::to_ir()` and `lowering::from_ir()`
functions. The IR preserves vendor-opaque metadata in `IrMessage::metadata`
so it survives round-trip translation.

### Design Principles

- **Vendor-neutral**: the IR captures semantic meaning, not wire format.
- **Content-block based**: messages contain typed content blocks, not raw
  strings. This preserves tool calls, images, and thinking blocks.
- **Deterministic**: uses `BTreeMap` for metadata to ensure canonical
  serialization.
- **Lossless where possible**: `Thinking` blocks carry through for backends
  that support extended reasoning; they are dropped (with a warning) for
  backends that don't.

---

## Tracing

ABP uses structured tracing with the following targets:

| Target | Description |
|--------|-------------|
| `abp.runtime` | Runtime orchestration events |
| `abp.sidecar` | Sidecar management events |
| `abp.sidecar.stderr` | Captured stderr from sidecar processes |
| `abp.workspace` | Workspace staging events |

Enable with `RUST_LOG=abp=debug` or the `--debug` CLI flag.
