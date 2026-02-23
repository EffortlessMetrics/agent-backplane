# Mapping matrix

This document is a planning aid for building real SDK shims.

## Normalized primitives

The contract needs to represent these cleanly:

- **Messages**: system/developer/user/assistant
- **Streaming**: deltas vs full messages
- **Tools**:
  - declaration
  - tool call
  - tool result
  - tool errors
- **Files**:
  - read/write/edit with policy gates
- **Web**: search/fetch
- **Structured output**:
  - JSON schema enforcement where supported
- **Sessions**:
  - run state, resume/fork
- **Artifacts**:
  - patches, files, screenshots, logs

## Common mismatch patterns

### 1) Tool calling

SDKs differ on:

- schema language (JSON schema subset)
- how tool calls are represented (function calls vs tool blocks)
- how errors are encoded

Backplane stance:

- Normalize to a tool-call event with a stable `tool_use_id`.
- Preserve vendor raw payload in `usage_raw` or vendor fields.
- Mark `tool_*` capabilities as `native` or `emulated`.

### 2) Streaming semantics

Some SDKs stream:

- text deltas
- structured events
- tool calls

Backplane stance:

- Use `AssistantDelta` for text chunks.
- Use `ToolCall/ToolResult` for tools.
- Everything else becomes `Warning/Error` or vendor-specific artifacts.

### 3) “Agents” vs “Chat Completions”

Some SDKs expose an explicit agent runtime; others are closer to raw chat.

Backplane stance:

- The shim can expose the SDK’s public agent API.
- Internally you still produce `WorkOrder` + events.

### 4) Memory and state

Session state varies widely.

Backplane stance:

- Session behaviors must be explicit capabilities (`session_resume`, `session_fork`).
- If unsupported, require the shim to surface that error.

## What we still need certainty on

Before implementing real mappings, you need hard answers per SDK:

- streaming event shapes and ordering guarantees
- tool calling schema subset and limits
- file/tool APIs exposed by the SDK (if any)
- cost/usage reporting formats
- retry semantics and idempotency hooks
- how “runs” are represented (run id, steps)

