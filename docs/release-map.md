# Release Map

Which crates matter to you depends on what you're building.

## Start here

| Crate | What it is |
|-------|------------|
| `abp-core` | The contract — `WorkOrder`, `Receipt`, `AgentEvent`, capabilities |
| `abp-protocol` | JSONL wire format for sidecar communication |
| `sidecar-kit` | Transport layer for building sidecars |
| `abp-cli` | The `abp` command-line binary |
| `abp-daemon` | HTTP + WebSocket control-plane daemon |
| `abp-glob` | Include/exclude glob compilation (standalone, no ABP deps) |

## By persona

### Library consumer

You're embedding ABP in a Rust application or building tooling on top of the contract types.

| Crate | Purpose |
|-------|---------|
| `abp-core` | Contract types — the only mandatory dependency |
| `abp-error` | Unified error codes |
| `abp-protocol` | Wire format if you need to speak JSONL |
| `abp-config` | Load and validate TOML configuration |
| `abp-receipt` | Receipt hashing, chaining, diffing |
| `abp-shim-openai` | Drop-in OpenAI client replacement |
| `abp-shim-claude` | Drop-in Claude client replacement |
| `abp-shim-gemini` | Drop-in Gemini client replacement |
| `abp-shim-codex` | Drop-in Codex client replacement |
| `abp-shim-kimi` | Drop-in Kimi client replacement |
| `abp-shim-copilot` | Drop-in Copilot client replacement |

### Sidecar / host author

You're building a new sidecar process that speaks the ABP protocol.

| Crate | Purpose |
|-------|---------|
| `abp-core` | Contract types for work orders and receipts |
| `abp-protocol` | Envelope encoding/decoding |
| `sidecar-kit` | JSONL transport layer (handles handshake, streaming, framing) |
| `abp-sidecar-proto` | Sidecar-side protocol utilities |
| `abp-sidecar-utils` | Streaming helpers, heartbeat |

See also the bridge crates (`claude-bridge`, `openai-bridge`, etc.) as reference implementations.

### Operator / deployer

You're running ABP to orchestrate agent work.

| Crate | Purpose |
|-------|---------|
| `abp-cli` | The `abp` binary — `run`, `backends`, `config`, `receipt` subcommands |
| `abp-daemon` | HTTP daemon with REST + WebSocket |
| `abp-config` | Configuration loading and validation |

### Internal plumbing

These crates are published but exist to serve the runtime and CLI. You generally don't depend on them directly.

| Layer | Crates |
|-------|--------|
| Mapping & translation | `abp-ir`, `abp-mapper`, `abp-mapping`, `abp-dialect`, `abp-sdk-types` |
| Capability & routing | `abp-capability`, `abp-projection`, `abp-emulation` |
| Backend infrastructure | `abp-backend-core`, `abp-backend-mock`, `abp-backend-sidecar`, `abp-integrations` |
| Workspace & git | `abp-workspace`, `abp-git`, `abp-glob` |
| Resilience | `abp-ratelimit`, `abp-retry` |
| Observability | `abp-receipt-store`, `abp-telemetry`, `abp-stream` |
| Validation & errors | `abp-validate`, `abp-error-taxonomy` |
| Vendor SDK adapters | `abp-claude-sdk`, `abp-openai-sdk`, `abp-codex-sdk`, `abp-gemini-sdk`, `abp-kimi-sdk`, `abp-copilot-sdk` |
| Vendor bridges | `claude-bridge`, `openai-bridge`, `codex-bridge`, `gemini-bridge`, `kimi-bridge`, `copilot-bridge` |
| Sidecar registration | `abp-sidecar-sdk` |

## Not published

| Crate | Reason |
|-------|--------|
| `agent-backplane` | Root workspace metapackage |
| `xtask` | Build automation (schema gen, lint, gate, audit) |
