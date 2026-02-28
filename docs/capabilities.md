# Capability Matrix

> Contract version: `abp/v0.1`

## Overview

Every ABP backend advertises a **capability manifest** — a map from `Capability` to
`SupportLevel` — so the runtime can verify that a backend can satisfy a work order
before dispatching it. Work orders carry **capability requirements** that declare
the minimum support level needed for each feature.

---

## Capability Variants

| Capability                      | Wire name                          | Description                                                  |
|---------------------------------|------------------------------------|--------------------------------------------------------------|
| `Streaming`                     | `streaming`                        | Backend can stream incremental `AssistantDelta` events.      |
| `ToolRead`                      | `tool_read`                        | File-read tool (e.g. `Read`).                                |
| `ToolWrite`                     | `tool_write`                       | File-write / create tool (e.g. `Write`).                     |
| `ToolEdit`                      | `tool_edit`                        | In-place file editing tool (e.g. `Edit`, `MultiEdit`).       |
| `ToolBash`                      | `tool_bash`                        | Shell / command execution tool.                              |
| `ToolGlob`                      | `tool_glob`                        | Glob-pattern file search.                                    |
| `ToolGrep`                      | `tool_grep`                        | Content search / ripgrep.                                    |
| `ToolWebSearch`                 | `tool_web_search`                  | Web search capability.                                       |
| `ToolWebFetch`                  | `tool_web_fetch`                   | Fetch a URL and return contents.                             |
| `ToolAskUser`                   | `tool_ask_user`                    | Interactive prompt to the human operator.                    |
| `HooksPreToolUse`               | `hooks_pre_tool_use`               | Fire a hook **before** every tool invocation.                |
| `HooksPostToolUse`              | `hooks_post_tool_use`              | Fire a hook **after** every tool invocation.                 |
| `SessionResume`                 | `session_resume`                   | Resume a previously-started session/conversation.            |
| `SessionFork`                   | `session_fork`                     | Fork/branch an existing session into a new one.              |
| `Checkpointing`                 | `checkpointing`                    | Save and restore execution checkpoints.                      |
| `StructuredOutputJsonSchema`    | `structured_output_json_schema`    | Constrain model output to a JSON Schema.                     |
| `McpClient`                     | `mcp_client`                       | Connect to external MCP servers as a client.                 |
| `McpServer`                     | `mcp_server`                       | Expose MCP server endpoints.                                 |

All variant names serialize as **snake_case** (via `#[serde(rename_all = "snake_case")]`).

---

## Support Levels

`SupportLevel` describes **how well** a backend supports a capability:

| Level          | Wire value      | Meaning                                                        |
|----------------|-----------------|----------------------------------------------------------------|
| `Native`       | `"native"`      | First-class support — no translation or emulation needed.      |
| `Emulated`     | `"emulated"`    | Supported via ABP translation layer with acceptable fidelity.  |
| `Restricted`   | `{"restricted": {"reason": "..."}}` | Supported in principle but disabled by policy or environment. |
| `Unsupported`  | `"unsupported"` | Cannot be provided.                                            |

### MinSupport Thresholds

A work order requirement specifies a **minimum** acceptable level:

| `MinSupport` | Satisfied by                                        |
|--------------|-----------------------------------------------------|
| `Native`     | `Native` only.                                      |
| `Emulated`   | `Native`, `Emulated`, or `Restricted`.              |

The `SupportLevel::satisfies(&self, min: &MinSupport) -> bool` method implements
this logic. Note that `Restricted` satisfies `Emulated` but **not** `Native`.

---

## Backend Compatibility Matrix

The table below summarizes the capabilities reported by each sidecar host
shipped in the `hosts/` directory.

| Capability                   | Claude (sidecar) | Copilot         | Gemini          | Kimi            | Codex→Claude    |
|------------------------------|:-----------------:|:---------------:|:---------------:|:---------------:|:---------------:|
| `streaming`                  | Native            | Native          | Native          | Native          | Native          |
| `tool_read`                  | Emulated          | Native          | Native          | Native          | Native          |
| `tool_write`                 | Emulated          | Native          | Native          | Native          | Native          |
| `tool_edit`                  | Emulated          | Native          | Native          | Native          | Native          |
| `tool_bash`                  | Emulated          | Native          | Native          | Native          | Native          |
| `tool_glob`                  | Emulated          | Native          | Native          | Native          | Native          |
| `tool_grep`                  | Emulated          | Native          | Native          | Native          | Native          |
| `tool_web_search`            | Emulated          | Native          | Native          | Native          | Native          |
| `tool_web_fetch`             | Emulated          | Native          | Native          | Native          | Native          |
| `tool_ask_user`              | —                 | Native          | Native          | Native          | —               |
| `hooks_pre_tool_use`         | Native            | Emulated        | Emulated        | Emulated        | Native          |
| `hooks_post_tool_use`        | Native            | Emulated        | Emulated        | Emulated        | Native          |
| `session_resume`             | Emulated          | Native          | Native          | Native          | Emulated        |
| `session_fork`               | —                 | Native          | Native          | Native          | —               |
| `checkpointing`              | Emulated          | Emulated        | Emulated        | Emulated        | —               |
| `structured_output_json_schema` | Emulated       | Native          | Emulated        | Emulated        | Native          |
| `mcp_client`                 | Emulated          | Native          | Native          | Native          | —               |
| `mcp_server`                 | —                 | Emulated        | Emulated        | Emulated        | —               |

**Legend:** Native = first-class, Emulated = ABP translation layer, — = not advertised.

> The Claude sidecar uses `defaultCapabilities()` which reports Emulated for
> most tools because it delegates through a pluggable adapter module. The Codex
> column reflects the Codex→Claude dialect mapping defined in
> `hosts/codex/capabilities.js`.

---

## How Capability Requirements Work

### In a WorkOrder

```json
{
  "requirements": {
    "required": [
      { "capability": "streaming",  "min_support": "native" },
      { "capability": "tool_read",  "min_support": "emulated" }
    ]
  }
}
```

Every entry in `required` is a `CapabilityRequirement` with:
- **`capability`** — the `Capability` variant (snake_case).
- **`min_support`** — the `MinSupport` threshold (`native` or `emulated`).

### Matching Algorithm

For a backend manifest to satisfy a set of requirements **all** requirements
must be met:

```text
for each requirement in work_order.requirements.required:
    level = manifest.get(requirement.capability)
    if level is None  → FAIL (capability not advertised)
    if !level.satisfies(requirement.min_support) → FAIL
→ PASS
```

### Examples

**Example 1 — Satisfied:**

```rust
// Backend manifest
manifest = { Streaming: Native, ToolRead: Emulated }

// Work order requires
requirements = [
    { capability: Streaming,  min_support: Native },   // Native >= Native  ✓
    { capability: ToolRead,   min_support: Emulated },  // Emulated >= Emulated  ✓
]
// Result: SATISFIED
```

**Example 2 — Not satisfied (level too low):**

```rust
manifest = { Streaming: Emulated }

requirements = [
    { capability: Streaming, min_support: Native },  // Emulated < Native  ✗
]
// Result: NOT SATISFIED
```

**Example 3 — Not satisfied (missing capability):**

```rust
manifest = { Streaming: Native }

requirements = [
    { capability: Streaming,   min_support: Native },    // ✓
    { capability: McpClient,   min_support: Emulated },  // not in manifest  ✗
]
// Result: NOT SATISFIED
```

**Example 4 — Restricted satisfies Emulated:**

```rust
manifest = { ToolBash: Restricted { reason: "sandbox only" } }

requirements = [
    { capability: ToolBash, min_support: Emulated },  // Restricted >= Emulated  ✓
]
// Result: SATISFIED
```

---

## Rust API

```rust
use abp_core::*;
use std::collections::BTreeMap;

// Build a manifest
let mut manifest = CapabilityManifest::new();
manifest.insert(Capability::Streaming, SupportLevel::Native);
manifest.insert(Capability::ToolRead, SupportLevel::Emulated);

// Check a single requirement
let req = CapabilityRequirement {
    capability: Capability::Streaming,
    min_support: MinSupport::Native,
};
let level = manifest.get(&req.capability).unwrap();
assert!(level.satisfies(&req.min_support));

// Use the builder
let wo = WorkOrderBuilder::new("task")
    .requirements(CapabilityRequirements {
        required: vec![req],
    })
    .build();
```

---

## Adding New Capabilities

1. Add a variant to the `Capability` enum in `crates/abp-core/src/lib.rs`.
2. Update sidecar manifests in `hosts/*/capabilities.js` (or `defaultCapabilities()`).
3. Regenerate JSON schemas: `cargo run -p xtask -- schema`.
4. Add tests in `crates/abp-core/tests/capability_tests.rs`.
5. Update the matrix table above.
