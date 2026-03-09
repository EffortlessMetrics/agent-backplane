# Claude Agent SDK Integration Guide for Agent Backplane

This guide defines the implemented `sidecar:claude` integration in this repository,
covering the Claude Agent SDK surfaces (Python V1, TypeScript V1, TypeScript V2 preview).

> **Note:** This is distinct from the Anthropic Messages API shim (`abp-shim-claude`),
> which provides a drop-in Claude SDK client replacement. The Agent SDK integration
> uses external sidecar processes that speak the ABP JSONL protocol.

## 1) Why this is implemented as a micro-sidecar

- The Claude Agent SDK owns agent loops, tool execution, sessions, permissions, hooks, MCP, sandboxing, and checkpointing.
- ABP keeps `abp-core` stable by confining all vendor-specific behavior to `hosts/claude/*` and `hosts/python/*`.
- The sidecar provides a deterministic boundary for receipts, event streams, and canonical hashing.
- Policy can be enforced once at the boundary before tool invocation reaches the SDK.

## 2) Runtime topology

### TypeScript V1 (default)

```text
WorkOrder -> abp-runtime -> host runtime -> ABP host process (hosts/claude/host.js)
   -> adapter V1 (hosts/claude/adapter.js)
      -> query() transport (one-shot, default)
      -> client transport (session-based, when abp.client_mode=true)
      -> Claude Agent SDK (@anthropic-ai/claude-agent-sdk)
```

### TypeScript V2 Preview

```text
WorkOrder -> abp-runtime -> host runtime -> ABP host process (hosts/claude/host.js)
   -> adapter V2 (hosts/claude/adapter-v2.js)
      -> unstable_v2_prompt() transport (one-shot)
      -> unstable_v2_createSession() / send() transport (session-based)
      -> Claude Agent SDK V2 preview surface
```

### Python V1

```text
WorkOrder -> abp-runtime -> host runtime -> ABP host process (hosts/python/host.py)
   -> Claude Agent SDK (claude_agent_sdk Python package)
      -> query() transport (one-shot, default)
      -> ClaudeSDKClient transport (session-based)
```

This split is intentional:

- `host.js` / `host.py` owns protocol, policy preflight, artifacts, receipts.
- `adapter.js` / `adapter-v2.js` owns Claude transport details (query/client/session).
- External SDK logic can be swapped without changing ABP internals.

## 3) Protocol behavior in this stack

The sidecar uses the ABP JSONL envelope:

- `hello` must be first line, with backend identity + capabilities.
- Each request is a `run` envelope with a full `work_order`.
- Progress/events are emitted with `event` envelopes.
- Final output is a `final` envelope containing an ABP `Receipt`.

`abp-host` enforces envelope order and hash injection at runtime. Any `receipt_sha256`
from the sidecar is replaced with the runtime canonical hash.

## 4) WorkOrder mapping

### Common fields consumed (all surfaces)

- `work_order.task` -> prompt/query text
- `work_order.workspace.root` -> cwd
- `work_order.context` -> context files/snippets (injected into prompt)
- `work_order.policy` -> `allowed_tools`, `disallowed_tools`, `require_approval_for`
- `work_order.config.model` -> model override
- `work_order.config.max_turns` -> maxTurns
- `work_order.config.vendor.claude` -> vendor-specific options
- `work_order.config.vendor.abp.mode` -> passthrough/mapped

### Claude vendor fields consumed

| Field | Type | Description |
|-------|------|-------------|
| `model` | string | Model identifier override |
| `systemPrompt` | string | System prompt text |
| `permissionMode` | string | Permission mode (e.g., `"acceptEdits"`) |
| `sessionId` | string | Session ID for resume |
| `resume` | bool | Whether to resume an existing session |
| `settingSources` | string[] | Settings sources (default: `["project"]`) |
| `allowedTools` | string[] | Explicit tool allowlist |
| `disallowedTools` | string[] | Explicit tool denylist |
| `maxTurns` | number | Maximum agent turns |
| `mcpServers` | object | MCP server configuration |
| `tools` | array | Custom tool definitions |
| `maxBudgetUsd` | number | Budget cap in USD |
| `outputFormat` | string | Structured output format |
| `thinking` | object | Extended thinking configuration |
| `effort` | string | Reasoning effort level |
| `sandbox` | object | Sandbox configuration |
| `enableFileCheckpointing` | bool | Enable file checkpointing |
| `hooks` | object | Hook configuration |
| `agents` | array | Sub-agent definitions |
| `plugins` | array | Plugin configuration |
| `sdk_surface` | string | SDK surface selector (`"ts_v1"`, `"ts_v2_preview"`, `"python_v1"`) |
| `v2_preview` | bool | Enable V2 preview adapter (TS only) |

### V2-only fields

| Field | Type | Description |
|-------|------|-------------|
| `transport` | string | V2 transport mode (`"prompt"` or `"session"`) |

### V1-only fields (rejected in V2)

`forkSession`, `cliPath`, `extraArgs`, `stderr`, `maxBufferSize` — these emit
a warning and are skipped when the V2 adapter is active.

## 5) Capability manifest

### Per-surface capabilities

| Capability | TS V1 | TS V2 Preview | Python V1 |
|------------|:-----:|:-------------:|:---------:|
| `streaming` | Native | Native | Native |
| `tool_read` | Emulated | Emulated | Emulated |
| `tool_write` | Emulated | Emulated | Emulated |
| `tool_edit` | Emulated | Emulated | Emulated |
| `tool_bash` | Emulated | Emulated | Emulated |
| `tool_glob` | Emulated | Emulated | Emulated |
| `tool_grep` | Emulated | Emulated | Emulated |
| `tool_web_search` | Emulated | Emulated | Emulated |
| `tool_web_fetch` | Emulated | Emulated | Emulated |
| `tool_ask_user` | Native | Native | Native |
| `hooks_pre_tool_use` | Native | Native | Native |
| `hooks_post_tool_use` | Native | Native | Native |
| `session_resume` | Native | Native | Native |
| `session_fork` | Emulated | — | Emulated |
| `checkpointing` | Native | Native | Native |
| `structured_output_json_schema` | Emulated | Native | Emulated |
| `mcp_client` | Native | Native | Native |
| `mcp_server` | — | — | — |
| `interrupt` | Native | Native | Native |
| `permission_callback` | Native | Native | Native |
| `subagents` | Native | Native | — |
| `custom_tools` | Native | Native | Native |
| `extended_thinking` | Native | Native | Native |

## 6) Security and governance at the boundary

`host.js` / `host.py` enforces these pre-flight checks:

- Hard tool allow/deny patterns from `work_order.policy`
- `require_approval_for` emits `permission_requested` events and auto-resolves based on policy
- `canUseTool` callback synthesized from ABP policy (allowed_tools / disallowed_tools)
- Path checks against `work_order.workspace.root`
- Budget limits via `maxBudgetUsd`

### Permission flow

1. SDK requests tool use -> ABP emits `permission_requested` event
2. Policy engine evaluates against allowed/disallowed lists
3. ABP emits `permission_resolved` event with `granted: true/false`
4. If denied, tool is skipped with a descriptive reason

## 7) Adapter contract

### V1 adapter (`adapter.js`)

Expects normalized request shape:
- `request_id`, `prompt`, `workspace_root`, `model`
- `systemMessage`, `context`, `policy`
- `streaming`, `options` (all vendor fields)
- optional `env`

Two transport modes:
- **query** (default): one-shot `sdk.query(prompt, options)` call
- **client**: session-based `sdk.client(options)` with multi-turn capability

### V2 adapter (`adapter-v2.js`)

Two transport modes:
- **prompt**: one-shot `sdk.unstable_v2_prompt(prompt, options)` call
- **session**: creates/resumes sessions via `unstable_v2_createSession()` / `unstable_v2_resumeSession()`

V2 receipts include preview labeling:
```json
{ "sdk_surface": "ts_v2_preview", "api_version": "v2_preview", "preview": true }
```

### Python adapter (`host.py`)

Two transport modes:
- **query** (default): one-shot `claude_agent_sdk.query()` call
- **client**: session-based `ClaudeSDKClient()` with multi-turn capability

## 8) Event mapping

### Hook events

| SDK Event | ABP Event | ext metadata |
|-----------|-----------|-------------|
| PreToolUse | `tool_call` | `{"hook": "pre_tool_use"}` |
| PostToolUse | `tool_result` | `{"hook": "post_tool_use"}` |
| PostToolUseFailure | `tool_result` | `{"hook": "post_tool_use_failure"}` |
| Notification | `warning` | `{"notification": true, "level": "..."}` |

### Session lifecycle events

| Event | Fields |
|-------|--------|
| `session_started` | `session_id` |
| `session_resumed` | `session_id`, `resumed_from` |

### Permission events

| Event | Fields |
|-------|--------|
| `permission_requested` | `tool_name`, `input` |
| `permission_resolved` | `tool_name`, `granted`, `reason` (optional) |

### Sub-agent events

| Event | Fields |
|-------|--------|
| `subagent_spawned` | `agent_id`, `task` |
| `subagent_completed` | `agent_id`, `success` |

## 9) Artifacts and receipts

The host:

- Writes `run_started` / `run_completed` markers
- Streams all recognized events to ABP
- Records tool call/results in trace
- Stores session metadata in `usage_raw`:
  - `session_id`, `transport`, `sdk_surface`, `num_turns`, `stop_reason`
  - V2 preview adds `api_version` and `preview: true`
- Computes receipt hash with deterministic nulling of `receipt_sha256`

## 10) End-to-end activation

```bash
# TypeScript V1 (default)
cargo run -p abp-cli -- run --backend sidecar:claude --task "refactor auth module"

# TypeScript V2 preview
cargo run -p abp-cli -- run --backend sidecar:claude \
  --param claude.v2_preview=true --task "refactor auth module"

# Python
cargo run -p abp-cli -- run --backend sidecar:python --task "refactor auth module"
```

Optional runtime overrides:

- `ABP_CLAUDE_SDK_MODULE=./path/to/sdk` — override SDK module path
- `ABP_CLAUDE_ADAPTER_MODULE=./path/to/adapter.js` — override adapter module
- `--env ANTHROPIC_API_KEY=sk-...` — pass API key to sidecar

### Vendor config examples

```json
{
  "config": {
    "vendor": {
      "claude": {
        "model": "claude-sonnet-4-20250514",
        "systemPrompt": "You are a helpful assistant.",
        "maxTurns": 10,
        "maxBudgetUsd": 5.0,
        "thinking": { "type": "enabled", "budget_tokens": 10000 },
        "mcpServers": { "filesystem": { "command": "npx", "args": ["-y", "@anthropic-ai/mcp-server-filesystem"] } }
      }
    }
  }
}
```

## 11) Production path

1. Install the Claude Agent SDK (`npm install @anthropic-ai/claude-agent-sdk` or `pip install claude-agent-sdk`).
2. Set `ANTHROPIC_API_KEY` in environment.
3. Keep `host.js` / `host.py` unchanged unless event/receipt schema changes are required.
4. Use policy assertions in integration tests:
   - hello-first ordering
   - permission flow (requested + resolved events)
   - session lifecycle events
   - receipt hash determinism
   - hook event normalization

## 12) ext field conventions

The `ext` field on `AgentEvent` (`Option<BTreeMap<String, Value>>`) carries sidecar-specific
metadata. The following keys are emitted by the Claude Agent SDK surfaces:

| ext key | Event type | Emitted by | Description |
|---------|-----------|------------|-------------|
| `hook` | `tool_call`, `tool_result` | All surfaces | Hook phase: `"pre_tool_use"`, `"post_tool_use"`, `"post_tool_use_failure"` |
| `decision` | `tool_call` | All surfaces | Hook decision when `hook = "pre_tool_use"`: `"allow"`, `"deny"` |
| `partial` | `assistant_delta` | Python V1 | Marks the event as a partial/streaming message |
| `notification` | `warning` | All surfaces | Boolean `true` indicating the warning originated from an SDK notification |
| `level` | `warning` | All surfaces | Severity level when `notification = true`: `"info"`, `"warn"`, `"error"` |
| `checkpoint_id` | `assistant_message` | All surfaces | Checkpoint identifier when the event represents a checkpoint creation |
| `checkpoint` | `assistant_message` | All surfaces | Boolean `true` when checkpoint_id is unavailable |
| `raw_message` | any | All surfaces (passthrough) | Original SDK message for lossless reconstruction |

These keys are conventions, not contract requirements. Consumers should handle their
absence gracefully. The `ext` field is `skip_serializing_if = "Option::is_none"` —
events without ext metadata omit the field entirely.
