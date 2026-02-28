# Error Catalog

Comprehensive reference for all error types across the Agent Backplane workspace.

---

## Table of Contents

- [abp-core — ContractError](#abp-core--contracterror)
- [abp-core — ValidationError](#abp-core--validationerror)
- [abp-protocol — ProtocolError](#abp-protocol--protocolerror)
- [sidecar-kit — SidecarError](#sidecar-kit--sidecarerror)
- [abp-host — HostError](#abp-host--hosterror)
- [claude-bridge — BridgeError](#claude-bridge--bridgeerror)
- [abp-runtime — RuntimeError](#abp-runtime--runtimeerror)
- [abp-cli — ConfigError](#abp-cli--configerror)
- [Error Decision Tree](#error-decision-tree)

---

## abp-core — `ContractError`

**Source:** `crates/abp-core/src/lib.rs`

Low-level errors from contract operations (serialization, hashing).

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `Json` | `serde_json::Error` (from) | A `Receipt` or contract type cannot be serialized to JSON. Triggered by `canonical_json()`, `receipt_hash()`, or `Receipt::with_hash()`. | Check that all field values are valid JSON-serializable types. This usually indicates a bug in the contract data (e.g. `NaN` or `Infinity` in a float field). |

---

## abp-core — `ValidationError`

**Source:** `crates/abp-core/src/validate.rs`

Validation failures found when checking a `Receipt` for completeness and consistency. The validator accumulates _all_ errors rather than short-circuiting.

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `MissingField` | `field: &'static str` | A required receipt field is missing or empty. | Ensure the backend populates all required fields before returning a receipt. |
| `InvalidHash` | `expected: String`, `actual: String` | The stored `receipt_sha256` does not match the recomputed hash. | The receipt was modified after hashing. Re-hash with `Receipt::with_hash()` after all mutations are complete. |
| `EmptyBackendId` | — | `receipt.backend.id` is an empty string. | Backends must set a non-empty identifier in their `BackendIdentity`. |
| `InvalidOutcome` | `reason: String` | Catch-all for semantic problems: `contract_version` mismatch, `started_at > finished_at`, or hash recomputation failure. | Read the `reason` string for specifics. Common causes: clock skew (timestamps), mismatched contract version, or serialization failure during hash verification. |

**Usage:** Call `validate_receipt(&receipt)` → `Result<(), Vec<ValidationError>>`.

---

## abp-protocol — `ProtocolError`

**Source:** `crates/abp-protocol/src/lib.rs`

Errors from JSONL encoding/decoding and protocol-level violations on the wire.

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `Json` | `serde_json::Error` (from) | A JSONL line cannot be parsed or an `Envelope` cannot be serialized. Raised by `JsonlCodec::encode()` and `JsonlCodec::decode()`. | Inspect the raw line for malformed JSON. Verify the sidecar is emitting valid `Envelope` objects with the `"t"` discriminator tag. |
| `Violation` | `String` | A generic protocol rule was broken (e.g. missing handshake, wrong sequencing). | Review the sidecar's message ordering. The protocol requires: `hello` → `run` → `event*` → `final`/`fatal`. |
| `UnexpectedMessage` | `expected: String`, `got: String` | The control plane expected one envelope type but received another (e.g. expected `hello`, got `event`). | Ensure the sidecar sends `hello` as its very first stdout line before any other messages. |

---

## sidecar-kit — `SidecarError`

**Source:** `crates/sidecar-kit/src/error.rs`

Low-level errors from sidecar process I/O and the raw JSONL transport layer.

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `Spawn` | `std::io::Error` (source) | The sidecar process could not be started (missing binary, permission denied, etc.). | Verify the command exists and is executable. Check `PATH` and file permissions. |
| `Stdout` | `std::io::Error` (source) | Failed to read from the sidecar's stdout pipe. | The sidecar may have crashed or closed its stdout prematurely. Check sidecar stderr logs (`abp.sidecar.stderr` tracing target). |
| `Stdin` | `std::io::Error` (source) | Failed to write to the sidecar's stdin pipe. | The sidecar process likely exited. Check exit code and stderr. |
| `Protocol` | `String` | A protocol-level invariant was violated at the transport layer. | Review the raw JSONL output from the sidecar for correctness. |
| `Serialize` | `serde_json::Error` (source) | An outbound message (e.g. `run` envelope) could not be serialized to JSON. | Indicates a bug in the caller's data. Check for non-serializable values. |
| `Deserialize` | `serde_json::Error` (source) | An inbound JSONL line could not be deserialized into a `Frame`. | The sidecar emitted invalid JSON. Log and inspect the raw line. |
| `Fatal` | `String` | The sidecar explicitly sent a fatal error message. | Read the error string. This is an intentional error from the sidecar indicating it cannot continue. |
| `Exited` | `Option<i32>` | The sidecar process exited unexpectedly. The `Option<i32>` is the exit code (`None` if killed by signal). | Non-zero exit codes suggest a crash. Check sidecar logs. Exit code `None` may indicate the process was killed by a signal (OOM, timeout). |
| `Timeout` | — | A sidecar operation exceeded its time limit. | Increase the configured timeout, or investigate why the sidecar is slow (e.g. network latency, large model responses). |

---

## abp-host — `HostError`

**Source:** `crates/abp-host/src/lib.rs`

Higher-level sidecar management errors. Wraps `ProtocolError` and adds process-supervision concerns. Used by `SidecarClient` during spawn, handshake, and run.

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `Spawn` | `std::io::Error` (source) | `SidecarClient::spawn()` failed to start the child process. | Check the `SidecarSpec` command/args. Verify the binary is installed and accessible. |
| `Stdout` | `std::io::Error` (source) | Reading from the sidecar's stdout failed during handshake or event streaming. | Sidecar likely crashed. Inspect stderr via `abp.sidecar.stderr`. |
| `Stdin` | `std::io::Error` (source) | Writing the `run` envelope to the sidecar's stdin failed. | Sidecar process has exited. Check for early crash. |
| `Protocol` | `ProtocolError` (from) | JSONL decoding failed or the sidecar sent invalid envelope JSON. Automatically converted from `ProtocolError`. | Debug the sidecar's stdout output. Common cause: sidecar printing non-JSON to stdout (e.g. debug logs). |
| `Violation` | `String` | A protocol rule was broken at the host level (e.g. stdin/stdout pipes unavailable, unexpected envelope type during a run). | Ensure the sidecar follows the protocol: no extra stdout before `hello`, correct `ref_id` correlation. |
| `Fatal` | `String` | The sidecar sent a `fatal` envelope, signaling an unrecoverable error. | Read the error message. The sidecar chose to abort — common causes include missing API keys, invalid configuration, or upstream API failures. |
| `Exited` | `code: Option<i32>` | The sidecar process exited before sending a `final` or `fatal` envelope. | A crash or segfault. Check the exit code: `1` is generic failure, `127` is command not found, `137`/`None` is OOM-killed or signal. |

---

## claude-bridge — `BridgeError`

**Source:** `crates/claude-bridge/src/error.rs`

Errors specific to the Claude sidecar bridge, which spawns a Node.js host process.

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `NodeNotFound` | `String` | The `node` binary was not found on `PATH` or at the expected location. | Install Node.js and ensure it is on `PATH`. The error string contains the attempted lookup path. |
| `HostScriptNotFound` | `String` | The sidecar host script (e.g. `hosts/claude/index.js`) does not exist at the expected path. | Run the CLI from the repository root, or verify the `hosts/` directory is intact. The error string contains the expected script path. |
| `Sidecar` | `SidecarError` (from) | A lower-level sidecar-kit error bubbled up (spawn, I/O, protocol, timeout). | See [SidecarError](#sidecar-kit--sidecarerror) for specific variant handling. |
| `Config` | `String` | Bridge-level configuration is invalid (e.g. missing API key, bad environment). | Check the bridge configuration and environment variables (e.g. `ANTHROPIC_API_KEY`). |
| `Run` | `String` | A run-level error occurred that doesn't fit other categories. | Read the error message for specifics. May indicate an upstream API error or sidecar logic failure. |

---

## abp-runtime — `RuntimeError`

**Source:** `crates/abp-runtime/src/lib.rs`

Top-level orchestration errors. These are what callers of `Runtime::run_streaming()` and `Runtime::check_capabilities()` receive.

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `UnknownBackend` | `name: String` | The requested backend name is not registered in the `BackendRegistry`. | List available backends with `Runtime::backend_names()`. Check for typos or missing `register_backend()` calls. |
| `WorkspaceFailed` | `anyhow::Error` (source) | Workspace preparation failed (staging copy, git init, glob filtering). | Check that the workspace root exists and is readable. Inspect the source error for permission or disk-space issues. |
| `PolicyFailed` | `anyhow::Error` (source) | Policy compilation failed (invalid glob patterns in `PolicyProfile`). | Validate glob patterns in `allowed_tools`, `deny_read`, `deny_write`, etc. Use `abp-glob` to test patterns. |
| `BackendFailed` | `anyhow::Error` (source) | The backend returned an error during execution, or the backend task panicked. | Inspect the source chain — this wraps errors from `SidecarBackend`, `MockBackend`, or any `Backend` implementation. |
| `CapabilityCheckFailed` | `String` | The backend's `CapabilityManifest` does not satisfy the work order's `CapabilityRequirements`. | Either relax the work order requirements, choose a more capable backend, or update the backend to advertise the missing capabilities. |

---

## abp-cli — `ConfigError`

**Source:** `crates/abp-cli/src/config.rs`

Validation errors for the TOML configuration file (`backplane.toml`). The validator accumulates all errors.

| Variant | Fields | When It Occurs | Recovery |
|---------|--------|----------------|----------|
| `InvalidBackend` | `name: String`, `reason: String` | A backend definition is semantically invalid (e.g. sidecar with an empty command). | Fix the backend entry in `backplane.toml`. The `reason` field describes the specific problem. |
| `InvalidTimeout` | `value: u64` | A sidecar `timeout_secs` is `0` or exceeds 86,400 (24 hours). | Set `timeout_secs` to a value in the range `1..=86400`. |
| `MissingRequiredField` | `field: String` | A required configuration field is missing (e.g. empty backend name key). | Add the missing field to the configuration file. |

---

## Error Decision Tree

Use this flowchart to diagnose errors starting from the symptom you observe.

```
Your program returned an error
│
├─ Is it a RuntimeError?
│  │
│  ├─ UnknownBackend
│  │  └─ Did you register the backend?
│  │     ├─ No  → call runtime.register_backend("name", backend)
│  │     └─ Yes → check for typos in the backend name
│  │
│  ├─ WorkspaceFailed
│  │  └─ Does the workspace root directory exist?
│  │     ├─ No  → create it or fix the path in WorkOrder.workspace.root
│  │     └─ Yes → check permissions, disk space, and glob patterns
│  │
│  ├─ PolicyFailed
│  │  └─ Are your glob patterns valid?
│  │     ├─ No  → fix patterns in PolicyProfile (deny_read, deny_write, etc.)
│  │     └─ Yes → check for mismatched braces or unsupported syntax
│  │
│  ├─ BackendFailed
│  │  └─ Inspect the source error chain:
│  │     ├─ Is it a HostError/SidecarError? → see sidecar troubleshooting below
│  │     ├─ Did the backend task panic?     → check for bugs in the Backend impl
│  │     └─ Is it a hashing error?          → see ContractError::Json
│  │
│  └─ CapabilityCheckFailed
│     └─ Does the backend support the required capabilities?
│        ├─ No  → choose a different backend or relax requirements
│        └─ Yes → sidecar caps are only known after handshake; check post-hello
│
├─ Is it a HostError or SidecarError?
│  │
│  ├─ Spawn
│  │  └─ Is the sidecar command installed?
│  │     ├─ No  → install it (e.g. `node`, `python`)
│  │     └─ Yes → check PATH, permissions, and SidecarSpec.cwd
│  │
│  ├─ Stdout / Stdin
│  │  └─ Did the sidecar crash?
│  │     ├─ Yes → check stderr logs (RUST_LOG=abp.sidecar.stderr=debug)
│  │     └─ No  → possible pipe buffer issue; check for deadlocks
│  │
│  ├─ Protocol / Violation / UnexpectedMessage
│  │  └─ Is the sidecar printing non-JSON to stdout?
│  │     ├─ Yes → redirect debug output to stderr; stdout is JSONL-only
│  │     └─ No  → check envelope ordering: hello → run → event* → final/fatal
│  │
│  ├─ Fatal
│  │  └─ Read the error message from the sidecar
│  │     ├─ Missing API key?  → set the appropriate env var
│  │     ├─ Rate limited?     → add retry logic or reduce concurrency
│  │     └─ Other?            → check sidecar-specific docs
│  │
│  ├─ Exited
│  │  └─ What is the exit code?
│  │     ├─ None   → killed by signal (OOM? timeout? manual kill?)
│  │     ├─ 1      → generic failure; check stderr
│  │     ├─ 127    → command not found
│  │     └─ Other  → sidecar-specific; check its documentation
│  │
│  └─ Timeout
│     └─ Is the operation expected to be slow?
│        ├─ Yes → increase timeout_secs in config
│        └─ No  → sidecar may be hung; check for blocking I/O or deadlocks
│
├─ Is it a BridgeError?
│  │
│  ├─ NodeNotFound
│  │  └─ Install Node.js and ensure `node` is on PATH
│  │
│  ├─ HostScriptNotFound
│  │  └─ Run the CLI from the repo root (hosts/ is resolved relative to CWD)
│  │
│  ├─ Sidecar
│  │  └─ Follow the SidecarError branch above
│  │
│  ├─ Config
│  │  └─ Check environment variables (ANTHROPIC_API_KEY, etc.)
│  │
│  └─ Run
│     └─ Read the error message; may be an upstream API error
│
├─ Is it a ConfigError?
│  │
│  ├─ InvalidBackend
│  │  └─ Fix the backend entry in backplane.toml (check command field)
│  │
│  ├─ InvalidTimeout
│  │  └─ Set timeout_secs between 1 and 86400
│  │
│  └─ MissingRequiredField
│     └─ Add the missing field to your config file
│
├─ Is it a ValidationError?
│  │
│  ├─ InvalidHash
│  │  └─ Receipt was modified after hashing → call .with_hash() last
│  │
│  ├─ EmptyBackendId
│  │  └─ Backend must set a non-empty id in BackendIdentity
│  │
│  ├─ MissingField
│  │  └─ Populate the indicated field before returning the receipt
│  │
│  └─ InvalidOutcome
│     └─ Read the reason string (version mismatch? timestamp order? hash failure?)
│
└─ Is it a ContractError?
   └─ Json
      └─ A value is not JSON-serializable (NaN? circular ref?)
         → fix the offending field in the receipt or work order
```

---

## Error Propagation Chain

Errors flow upward through the crate hierarchy:

```
SidecarError (sidecar-kit)           ContractError (abp-core)
     │                                      │
     ├──→ BridgeError (claude-bridge)       │
     │                                      │
     ▼                                      │
ProtocolError (abp-protocol)                │
     │                                      │
     ▼                                      │
HostError (abp-host)                        │
     │                                      │
     ▼                                      ▼
RuntimeError (abp-runtime)  ◄────  anyhow wrapping
     │
     ▼
CLI / caller
```

Key observations:

- **`SidecarError`** is the base I/O layer; **`HostError`** adds process supervision.
- **`ProtocolError`** converts into `HostError::Protocol` via `#[from]`.
- **`SidecarError`** converts into `BridgeError::Sidecar` via `#[from]`.
- **`RuntimeError`** wraps most inner errors as `anyhow::Error` sources, preserving the full chain.
- **`ValidationError`** stands alone — it is used for post-hoc receipt auditing, not during execution.
- **`ConfigError`** stands alone — it is used during config loading before any backend is started.
