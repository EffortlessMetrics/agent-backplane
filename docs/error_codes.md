# Error Codes

> Stable, machine-readable error taxonomy for the Agent Backplane.

All ABP errors carry an `ErrorCode` — a SCREAMING_SNAKE_CASE string tag that
is guaranteed stable across patch releases. Errors also include a human-readable
message, an optional cause chain, and arbitrary structured context via
`BTreeMap<String, serde_json::Value>`.

**Source:** `crates/abp-error/src/lib.rs`

---

## Table of Contents

- [Error Structure](#error-structure)
- [Error Categories](#error-categories)
- [Error Code Reference](#error-code-reference)
  - [Protocol Errors](#protocol-errors)
  - [Backend Errors](#backend-errors)
  - [Capability Errors](#capability-errors)
  - [Policy Errors](#policy-errors)
  - [Workspace Errors](#workspace-errors)
  - [IR Errors](#ir-errors)
  - [Receipt Errors](#receipt-errors)
  - [Dialect Errors](#dialect-errors)
  - [Config Errors](#config-errors)
  - [Internal Errors](#internal-errors)
- [JSON Wire Format](#json-wire-format)
- [Quick Reference Table](#quick-reference-table)

---

## Error Structure

Every ABP error is represented by `AbpError`:

```rust
pub struct AbpError {
    pub code: ErrorCode,                              // stable tag
    pub message: String,                              // human-readable
    pub source: Option<Box<dyn Error + Send + Sync>>, // cause chain
    pub context: BTreeMap<String, serde_json::Value>,  // diagnostics
}
```

Use the builder to construct errors fluently:

```rust
let err = AbpError::new(ErrorCode::BackendTimeout, "timed out after 30 s")
    .with_context("backend", "openai")
    .with_context("timeout_ms", 30_000);
```

For serialization across the wire, `AbpErrorDto` is the JSON-safe snapshot
(replaces the opaque `source` with `source_message: Option<String>`).

---

## Error Categories

Each `ErrorCode` maps to exactly one `ErrorCategory`:

| Category | Wire Value | Scope |
|----------|------------|-------|
| `Protocol` | `"protocol"` | JSONL wire-format / envelope errors |
| `Backend` | `"backend"` | Backend lifecycle (spawn, run, exit) |
| `Capability` | `"capability"` | Capability negotiation failures |
| `Policy` | `"policy"` | Policy evaluation / enforcement |
| `Workspace` | `"workspace"` | Workspace staging / git init |
| `Ir` | `"ir"` | IR lowering / validation |
| `Receipt` | `"receipt"` | Receipt integrity / chain verification |
| `Dialect` | `"dialect"` | Dialect detection / mapping |
| `Config` | `"config"` | Configuration parsing / validation |
| `Internal` | `"internal"` | Unexpected internal failures |

---

## Error Code Reference

### Protocol Errors

#### `PROTOCOL_INVALID_ENVELOPE`

| | |
|---|---|
| **Category** | Protocol |
| **When raised** | An envelope failed to parse as JSON, or has missing/invalid fields. Typically during `JsonlCodec::decode()`. |
| **What to do** | Inspect the raw JSONL line from the sidecar. Verify it emits valid `Envelope` objects with the `"t"` discriminator tag. Common cause: sidecar printing debug output to stdout. |

#### `PROTOCOL_UNEXPECTED_MESSAGE`

| | |
|---|---|
| **Category** | Protocol |
| **When raised** | A message arrived in the wrong order — e.g. an `event` envelope before the `hello` handshake. |
| **What to do** | Ensure the sidecar follows the protocol ordering: `hello` → `run` → `event*` → `final`/`fatal`. The sidecar MUST send `hello` as its very first stdout line. |

#### `PROTOCOL_VERSION_MISMATCH`

| | |
|---|---|
| **Category** | Protocol |
| **When raised** | The `contract_version` in the sidecar's `hello` envelope has a different major version than the host. |
| **What to do** | Update either the sidecar or the host so both use the same major version. Versions are compatible if they share the same major number (e.g. `v0.1` and `v0.2` are compatible). |

---

### Backend Errors

#### `BACKEND_NOT_FOUND`

| | |
|---|---|
| **Category** | Backend |
| **When raised** | The requested backend name is not registered in the `BackendRegistry`. |
| **What to do** | List available backends with `Runtime::backend_names()` or `abp backends`. Check for typos. Ensure the backend was registered before the run. |

#### `BACKEND_TIMEOUT`

| | |
|---|---|
| **Category** | Backend |
| **When raised** | The backend did not respond within the configured timeout. |
| **What to do** | Increase `timeout_secs` in the sidecar configuration. Investigate whether the sidecar is blocked on network I/O or waiting for a slow model response. |

#### `BACKEND_CRASHED`

| | |
|---|---|
| **Category** | Backend |
| **When raised** | The backend process exited unexpectedly before sending `final` or `fatal`. |
| **What to do** | Check the exit code: `1` = generic failure, `127` = command not found, `None` = killed by signal (OOM, manual kill). Inspect sidecar stderr via `RUST_LOG=abp.sidecar.stderr=debug`. |

---

### Capability Errors

#### `CAPABILITY_UNSUPPORTED`

| | |
|---|---|
| **Category** | Capability |
| **When raised** | A required capability is not supported by the selected backend (neither natively nor via emulation). Raised during the pre-dispatch capability check. |
| **What to do** | Choose a more capable backend, relax the work order's `CapabilityRequirements`, or remove the unsupported requirement. |

#### `CAPABILITY_EMULATION_FAILED`

| | |
|---|---|
| **Category** | Capability |
| **When raised** | The emulation layer attempted to provide a capability but failed at runtime — e.g. a system-prompt injection strategy produced an invalid conversation. |
| **What to do** | Check the `EmulationReport` for details. Consider overriding the emulation strategy in `EmulationConfig`, or switch to a backend with native support. |

---

### Policy Errors

#### `POLICY_DENIED`

| | |
|---|---|
| **Category** | Policy |
| **When raised** | A policy rule denied the requested operation — e.g. a tool invocation matched a `disallowed_tools` glob, or a file path matched `deny_write`. |
| **What to do** | Review the `PolicyProfile` in the work order. Adjust `allowed_tools`, `disallowed_tools`, `deny_read`, or `deny_write` patterns as needed. |

#### `POLICY_INVALID`

| | |
|---|---|
| **Category** | Policy |
| **When raised** | The policy definition itself is malformed — e.g. invalid glob syntax in a pattern. |
| **What to do** | Validate glob patterns in the `PolicyProfile`. Check for mismatched braces or unsupported glob syntax. Use `abp-glob` to test patterns. |

---

### Workspace Errors

#### `WORKSPACE_INIT_FAILED`

| | |
|---|---|
| **Category** | Workspace |
| **When raised** | Failed to initialize the staged workspace — e.g. `git init` failed, temp directory creation failed, or the workspace root does not exist. |
| **What to do** | Verify the workspace root path exists and is readable. Check disk space and file permissions. Ensure `git` is available on PATH. |

#### `WORKSPACE_STAGING_FAILED`

| | |
|---|---|
| **Category** | Workspace |
| **When raised** | Failed to copy/stage files into the workspace — e.g. glob filter error, permission denied on source files, or I/O error during copy. |
| **What to do** | Check the source error for specifics. Common causes: invalid include/exclude glob patterns, permission denied, or symlink loops. |

---

### IR Errors

#### `IR_LOWERING_FAILED`

| | |
|---|---|
| **Category** | IR |
| **When raised** | Converting a high-level IR conversation into a vendor-specific wire format failed. This happens during the "engine lowering" phase of mapped-mode execution. |
| **What to do** | Check the source error chain. The IR may contain content blocks that the target dialect cannot represent (e.g. `Thinking` blocks for a backend without extended thinking support). |

#### `IR_INVALID`

| | |
|---|---|
| **Category** | IR |
| **When raised** | The IR structure is invalid or internally inconsistent — e.g. a `ToolResult` references a `tool_use_id` that doesn't exist, or messages are empty. |
| **What to do** | Review the IR construction logic. Ensure tool result blocks reference valid tool-use IDs and that all required content blocks are present. |

---

### Receipt Errors

#### `RECEIPT_HASH_MISMATCH`

| | |
|---|---|
| **Category** | Receipt |
| **When raised** | The stored `receipt_sha256` does not match the recomputed hash. The receipt was modified after hashing. |
| **What to do** | Always call `Receipt::with_hash()` (or `ReceiptBuilder::with_hash()`) as the **last** mutation on a receipt. Never modify fields after hashing. |

#### `RECEIPT_CHAIN_BROKEN`

| | |
|---|---|
| **Category** | Receipt |
| **When raised** | A receipt chain has a gap (non-sequential work order), ordering violation, or integrity failure. |
| **What to do** | Verify that receipts are appended in order. Check `ReceiptChain::push()` error for specifics — possible causes include duplicate entries, out-of-order timestamps, or hash verification failure on a chain member. |

---

### Dialect Errors

#### `DIALECT_UNKNOWN`

| | |
|---|---|
| **Category** | Dialect |
| **When raised** | The dialect identifier in a request or configuration is not recognized. Known dialects: `OpenAi`, `Claude`, `Gemini`, `Codex`, `Kimi`, `Copilot`. |
| **What to do** | Check for typos in the dialect identifier. Ensure the SDK adapter crate is included in the build. |

#### `DIALECT_MAPPING_FAILED`

| | |
|---|---|
| **Category** | Dialect |
| **When raised** | Mapping between two dialects failed — e.g. a feature has no translation rule, or the mapping incurs unacceptable fidelity loss. |
| **What to do** | Check the `MappingRegistry` for the source→target dialect pair. Some features are inherently lossy between specific dialects (see `known_rules()` in `abp-mapping`). Consider using passthrough mode if the dialect matches the backend engine. |

---

### Config Errors

#### `CONFIG_INVALID`

| | |
|---|---|
| **Category** | Config |
| **When raised** | A configuration file or value is invalid — e.g. malformed TOML, missing required fields, invalid timeout value, or empty backend name. |
| **What to do** | Review the configuration file (typically `backplane.toml`). Common issues: `timeout_secs` outside 1–86400, empty backend name keys, sidecar with empty command. |

---

### Internal Errors

#### `INTERNAL`

| | |
|---|---|
| **Category** | Internal |
| **When raised** | Catch-all for unexpected internal errors that don't fit any other category — e.g. a panic in an async task, unexpected `None` value, or logic bug. |
| **What to do** | This usually indicates a bug in ABP itself. File an issue with the full error message, context map, and source chain. |

---

## JSON Wire Format

Errors are serialized as `AbpErrorDto` on the wire:

```json
{
  "code": "BACKEND_TIMEOUT",
  "message": "timed out after 30 s",
  "context": {
    "backend": "openai",
    "timeout_ms": 30000
  },
  "source_message": "connection reset by peer"
}
```

The `source_message` field is omitted when there is no underlying cause:

```json
{
  "code": "POLICY_DENIED",
  "message": "tool 'rm_rf' denied by policy",
  "context": {
    "tool": "rm_rf",
    "rule": "disallowed_tools"
  }
}
```

Error in a `fatal` protocol envelope:

```json
{
  "t": "fatal",
  "ref_id": "550e8400-e29b-41d4-a716-446655440000",
  "error": {
    "code": "BACKEND_CRASHED",
    "message": "sidecar exited with code 1",
    "context": {
      "exit_code": 1
    }
  }
}
```

---

## Quick Reference Table

| Code | Category | One-line Summary |
|------|----------|-----------------|
| `PROTOCOL_INVALID_ENVELOPE` | Protocol | Envelope JSON is malformed or has invalid fields |
| `PROTOCOL_UNEXPECTED_MESSAGE` | Protocol | Wrong message order (e.g. event before hello) |
| `PROTOCOL_VERSION_MISMATCH` | Protocol | Major version mismatch between host and sidecar |
| `BACKEND_NOT_FOUND` | Backend | Requested backend name not in registry |
| `BACKEND_TIMEOUT` | Backend | Backend didn't respond in time |
| `BACKEND_CRASHED` | Backend | Backend process exited unexpectedly |
| `CAPABILITY_UNSUPPORTED` | Capability | Required capability not available |
| `CAPABILITY_EMULATION_FAILED` | Capability | Emulation layer failed at runtime |
| `POLICY_DENIED` | Policy | Operation blocked by policy rule |
| `POLICY_INVALID` | Policy | Policy definition is malformed |
| `WORKSPACE_INIT_FAILED` | Workspace | Staged workspace initialization failed |
| `WORKSPACE_STAGING_FAILED` | Workspace | File copy/staging failed |
| `IR_LOWERING_FAILED` | IR | IR → wire format conversion failed |
| `IR_INVALID` | IR | IR structure is invalid/inconsistent |
| `RECEIPT_HASH_MISMATCH` | Receipt | Hash doesn't match recomputed value |
| `RECEIPT_CHAIN_BROKEN` | Receipt | Receipt chain has gap or ordering violation |
| `DIALECT_UNKNOWN` | Dialect | Dialect identifier not recognized |
| `DIALECT_MAPPING_FAILED` | Dialect | Cross-dialect feature mapping failed |
| `CONFIG_INVALID` | Config | Configuration file/value is invalid |
| `INTERNAL` | Internal | Unexpected internal error (likely a bug) |
