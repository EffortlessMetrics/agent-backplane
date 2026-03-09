# Claude Agent Python Surface

The Python surface (`hosts/python/host.py`) is a first-class ABP sidecar that speaks
the JSONL protocol and integrates with the `claude_agent_sdk` Python package.

## Transport modes

- **Query** (default): One-shot `claude_agent_sdk.query()` call
- **Client**: Session-based `ClaudeSDKClient()` with multi-turn capability
  - Enable via `abp.client_mode = true`
  - Session persistence via `abp.client_persist = true`
  - Timeout via `abp.client_timeout_ms`

## Options

All 30+ options from the TS V1 surface are supported, mapped via `_pick(cfg, camelCase, snake_case)`:

| Option | Type | Description |
|--------|------|-------------|
| `model` | string | Model identifier |
| `systemPrompt` | string | System prompt |
| `permissionMode` | string | Permission mode |
| `sessionId` | string | Session ID for resume |
| `resume` | bool | Resume existing session |
| `settingSources` | string[] | Settings sources (default: `["project"]`) |
| `allowedTools` | string[] | Tool allowlist |
| `disallowedTools` | string[] | Tool denylist |
| `maxTurns` | number | Maximum agent turns |
| `mcpServers` | object | MCP server configuration |
| `tools` | array | Custom tool definitions |
| `maxBudgetUsd` | number | Budget cap in USD |
| `thinking` | object | Extended thinking |
| `effort` | string | Reasoning effort |
| `sandbox` | object | Sandbox configuration |
| `enableFileCheckpointing` | bool | Enable file checkpointing |
| `includePartialMessages` | bool | Emit partial/streaming events (default: true) |
| `forkSession` | bool | Fork an existing session |
| `persistSession` | bool | Persist session state |

## Policy engine

The Python sidecar includes a full policy engine matching the TS V1 host:

- **Tool allow/deny**: Glob-based tool name filtering
- **Read/write path control**: `deny_read`, `deny_write` glob patterns
- **Network access control**: `allow_network`, `deny_network` patterns
- **Path escape detection**: Prevents tools from accessing files outside workspace root
- **Approval requirements**: `require_approval_for` patterns

## Artifacts

Large tool outputs (> 8KB) are automatically offloaded to `.agent-backplane/artifacts/{run_id}/`.
Secret patterns (API keys, tokens) are redacted before logging.

## Capabilities

| Capability | Level |
|-----------|-------|
| `streaming` | Native |
| `hooks_pre_tool_use` | Native |
| `hooks_post_tool_use` | Native |
| `session_resume` | Native |
| `session_fork` | Emulated |
| `checkpointing` | Native |
| `mcp_client` | Native |
| `permission_callback` | Native |
| `interrupt` | Native |
| `extended_thinking` | Native |
| `tool_ask_user` | Native |

## Known limitations

- No sub-agent support (SDK limitation in Python)
- Session fork is emulated (creates new session with same config)
- Structured output JSON schema is emulated
