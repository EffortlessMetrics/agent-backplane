# Python Sidecar

Python sidecar for Agent Backplane with optional Claude SDK client mode.
Supports both `query()` and persistent `ClaudeSDKClient` transports when a
compatible Python SDK module is available.

## Files

| File | Description |
|------|-------------|
| `host.py` | Main sidecar entry point (async, uses `asyncio`) |

## Usage

```bash
# Via the ABP CLI
cargo run -p abp-cli -- run --backend sidecar:python --task "hello"

# Standalone (for debugging)
echo '{"t":"run","id":"1","work_order":{"id":"1","task":"hello"}}' | python hosts/python/host.py
```

## Configuration

| Environment Variable | Description |
|---------------------|-------------|
| `ABP_CLAUDE_SDK_MODULE` | Override the Python SDK module name (default: `claude_agent_sdk`) |

Client mode and persistence are configured via vendor params (`abp.client_mode`,
`abp.client_persist`, `abp.client_timeout_ms`).

## Capabilities

| Capability | Level |
|------------|-------|
| `streaming` | Native |
| `tool_read` | Emulated |
| `tool_write` | Emulated |
| `tool_edit` | Emulated |
| `structured_output_json_schema` | Emulated |
| `hooks_pre_tool_use` | Native |
| `hooks_post_tool_use` | Native |
| `session_resume` | Emulated |

## Protocol

Speaks the ABP JSONL protocol over stdio. See [docs/sidecar_protocol.md](../../docs/sidecar_protocol.md).
