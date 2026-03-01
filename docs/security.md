# Security

> Contract version: `abp/v0.1`

This document describes the threat model, security controls, known limitations,
and audit guidance for Agent Backplane (ABP).

---

## Table of Contents

- [Threat Model](#threat-model)
- [Security Controls](#security-controls)
- [Known Limitations (v0.1)](#known-limitations-v01)
- [Audit Checklist](#audit-checklist)

---

## Threat Model

ABP sits between callers (SDK shims, CLI, HTTP daemon) and backend execution
environments (in-process mocks, external sidecar processes). Four trust
boundaries matter:

### Control Plane ↔ Sidecars

The Rust runtime (`abp-host`) spawns sidecar processes and communicates over
**stdin/stdout using JSONL**. The sidecar is an untrusted external process.

| Boundary | Trust assumption |
|----------|-----------------|
| Runtime → Sidecar | Sidecar receives a `WorkOrder` containing the task, policy, and workspace path. The runtime trusts the sidecar to emit valid JSONL envelopes. |
| Sidecar → Runtime | The runtime validates envelope structure (`hello` must be first, `ref_id` must match the active run). Malformed lines are treated as protocol errors. Events from unrecognised `ref_id`s are dropped with a warning. |
| Termination | After a run completes (or on protocol error), the runtime kills the child process and waits for exit (`child.kill()` + `child.wait()`). |

**Attack surface:** A compromised sidecar can emit arbitrary events, fabricate
tool results, or attempt to influence receipt content. The runtime records
whatever the sidecar reports — it does not independently verify tool execution.

### User Workspace ↔ Staged Workspace

`abp-workspace` supports two modes (`WorkspaceMode`):

- **PassThrough** — the agent operates directly on the user's workspace. No
  isolation is provided.
- **Staged** — a filtered copy is created in a `tempfile`-backed directory.
  The `.git` directory is always excluded from the copy. A fresh git repo is
  initialised with a baseline commit so diffs are meaningful.

The staged copy provides **file-level isolation**: the agent cannot modify the
original workspace. However, it does not provide OS-level sandboxing — the
sidecar process still has full filesystem and network access.

### Policy Engine ↔ Tool Execution

`abp-policy` compiles a `PolicyProfile` into glob-based allow/deny rules:

- **Tool rules** — `allowed_tools` / `disallowed_tools` glob lists. Deny
  always takes precedence over allow.
- **Read rules** — `deny_read` glob patterns block path-based reads.
- **Write rules** — `deny_write` glob patterns block path-based writes.
- **Network rules** — `allow_network` / `deny_network` fields are defined in
  the contract but **not yet enforced** at runtime.

Policy decisions are returned as `Decision { allowed, reason }`. The runtime
checks policy before dispatching, but enforcement depends on the backend
respecting these decisions. See [Known Limitations](#known-limitations-v01).

### Receipt Integrity ↔ Hash Verification

Every completed run produces a `Receipt` containing metadata, the event trace,
verification data (git diff/status), and a SHA-256 hash.

- `receipt_hash()` sets `receipt_sha256` to `null` before hashing to prevent
  the hash from being self-referential.
- The canonical JSON is produced via `serde_json::to_value` → `to_string`,
  which yields sorted keys (`BTreeMap` throughout) for deterministic output.
- Callers should use `receipt.with_hash()` rather than computing the hash
  manually.

**Limitation:** The hash covers the receipt's JSON representation, not the
underlying filesystem state. A tampered receipt can be detected by recomputing
the hash, but there is no signature or external attestation.

---

## Security Controls

### `#![deny(unsafe_code)]`

Every crate in the workspace uses `#![deny(unsafe_code)]`:

- `abp-core`, `abp-protocol`, `abp-host`, `abp-glob`, `abp-workspace`,
  `abp-policy`, `abp-integrations`, `abp-runtime`, `abp-cli`, `abp-daemon`,
  `sidecar-kit`, `claude-bridge`, and all SDK adapter crates.

This is enforced at compile time. Any `unsafe` block in first-party code will
fail the build.

### Policy Engine (Glob Allow/Deny)

The `PolicyEngine` (in `abp-policy`) provides three check methods:

| Method | Input | Behaviour |
|--------|-------|-----------|
| `can_use_tool(name)` | Tool name string | Matches against `allowed_tools` / `disallowed_tools` globs. Deny wins over allow. Empty allowlist means "permit all". |
| `can_read_path(path)` | Relative path | Matches against `deny_read` globs. If matched, returns `Decision::deny`. |
| `can_write_path(path)` | Relative path | Matches against `deny_write` globs. If matched, returns `Decision::deny`. |

Glob compilation uses `abp-glob` (backed by the `globset` crate). Invalid
patterns cause an error at policy compilation time, not at evaluation time.

### Workspace Staging with Git Isolation

Staged workspaces (`WorkspaceMode::Staged`) provide:

1. **Temp directory isolation** — `tempfile::tempdir()` creates a unique
   directory that is cleaned up when the `PreparedWorkspace` is dropped.
2. **`.git` exclusion** — `WalkDir` filters out `.git` via `filter_entry`,
   preventing leakage of git history, credentials, or hooks.
3. **Include/exclude glob filtering** — only files matching the workspace
   `include`/`exclude` patterns are copied.
4. **Baseline commit** — a fresh `git init` + `git add -A` + `git commit`
   creates a clean baseline for diff-based verification.

### Receipt SHA-256 Hashing

Receipts are integrity-checked via `receipt_hash()`:

1. Serialize the receipt to a `serde_json::Value`.
2. Set `receipt_sha256` to `null` in the serialized map.
3. Serialize to a canonical JSON string (keys sorted by `BTreeMap`).
4. Compute `SHA-256` over the UTF-8 bytes.
5. Store the hex-encoded digest in `receipt_sha256`.

This allows any party with the receipt JSON to verify integrity by repeating
steps 1–4 and comparing the result.

### Sidecar Process Isolation

`abp-host` constrains communication with sidecars:

- **stdio only** — `stdin`, `stdout`, and `stderr` are captured. No shared
  memory, sockets, or files are used for the protocol.
- **stderr logging** — sidecar stderr is forwarded to tracing at the
  `abp.sidecar.stderr` target as warnings.
- **Protocol enforcement** — the first line must be a `hello` envelope; any
  other message type causes an immediate `ProtocolError`.
- **Run correlation** — events with a `ref_id` that does not match the
  active run are dropped.
- **Process cleanup** — after the run loop exits, the child is killed and
  awaited to avoid zombie processes.

---

## Known Limitations (v0.1)

### No Encryption at Rest for Receipts

Receipts are stored as plain JSON in `.agent-backplane/receipts/`. They may
contain the full event trace, including tool inputs/outputs and assistant
messages. There is no encryption, access control, or redaction applied to
stored receipts.

### Sidecar Environment Inheritance

Sidecar processes inherit the parent process's environment variables unless
explicitly overridden via `SidecarSpec.env`. This means API keys, tokens, and
other secrets present in the parent environment are accessible to all sidecars.

### No Authentication on Daemon HTTP Endpoints

`abp-daemon` binds an HTTP API (default `127.0.0.1:8088`) with no
authentication or authorization. Any process on the same host can submit work
orders, list receipts, or query capabilities. Do not expose the daemon to
untrusted networks.

### Policy Enforcement Is Advisory

The `PolicyEngine` produces `Decision` values, but enforcement depends on the
backend implementation:

- The **runtime** checks policy before dispatching a work order.
- **Sidecars** receive the `PolicyProfile` inside the `WorkOrder` but are not
  required to enforce it. A malicious or buggy sidecar can invoke disallowed
  tools or read/write restricted paths.
- **Network policy** (`allow_network` / `deny_network`) fields exist in the
  contract but are not enforced by any component.

True enforcement requires OS-level sandboxing (seccomp, namespaces, containers),
which is not yet implemented.

### No Cryptographic Signing of Receipts

Receipt integrity relies on SHA-256 hashing but not on digital signatures. An
attacker with write access to the receipt store can modify a receipt and
recompute a valid hash. External attestation or signing is out of scope for
v0.1.

### No Input Sanitisation on Work Order Fields

`WorkOrder.task` and other string fields are passed through to sidecars without
sanitisation. Prompt injection and other input-dependent attacks are the
responsibility of the backend/sidecar, not the control plane.

---

## Audit Checklist

### Dependency Auditing

ABP uses `cargo-deny` (configured in [`deny.toml`](../deny.toml)):

| Check | Setting |
|-------|---------|
| Known vulnerabilities | `vulnerability = "deny"` |
| Unmaintained crates | `unmaintained = "warn"` |
| Yanked crates | `yanked = "warn"` |
| Copyleft licenses | `copyleft = "deny"` |
| Unknown registries | `unknown-registry = "warn"` |
| Unknown git sources | `unknown-git = "warn"` |
| Allowed registry | `crates.io` only |

Run manually:

```bash
cargo deny check advisories
cargo deny check licenses
cargo deny check bans
cargo deny check sources
```

Complement with `cargo audit` for the RustSec advisory database:

```bash
cargo install cargo-audit
cargo audit
```

### Unsafe Code Scanning

All first-party crates use `#![deny(unsafe_code)]`. To verify no `unsafe` has
been introduced:

```bash
# Check that deny(unsafe_code) is present in every crate root
grep -r "deny(unsafe_code)" crates/

# Scan transitive dependencies for unsafe usage
cargo install cargo-geiger
cargo geiger
```

### Input Validation

| Input | Validated | Notes |
|-------|-----------|-------|
| JSONL envelopes | ✅ | Deserialized via serde with typed enums; invalid JSON is a `ProtocolError`. |
| `hello` handshake | ✅ | First line must be `hello`; anything else is rejected. |
| `ref_id` correlation | ✅ | Events with non-matching `ref_id` are dropped. |
| Glob patterns | ✅ | Invalid globs fail at `PolicyEngine::new()` / `IncludeExcludeGlobs::new()`. |
| Work order fields | ❌ | String fields (`task`, `root`, etc.) are not sanitised. |
| Daemon HTTP input | ❌ | JSON body is deserialized but not schema-validated beyond serde types. |
| File paths in workspace staging | Partial | Paths are relativised via `strip_prefix`, but no canonicalization or symlink resolution is performed. |

### Supply Chain Considerations

- **Allowed licenses**: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC,
  Unicode-3.0, Unicode-DFS-2016, Zlib.
- **Registry restriction**: only `crates.io` is allowed; no third-party
  registries or git dependencies.
- **Sidecar hosts** (`hosts/`) are not vendored — they may `npm install`
  third-party packages. Audit sidecar `package.json` and lock files separately.
- **Lock file**: `Cargo.lock` is committed. Verify it matches `Cargo.toml`
  with `cargo update --dry-run`.
