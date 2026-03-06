# Node Sidecar

Minimal Node.js sidecar example for Agent Backplane. Returns a static greeting
and serves as a starting point for building real SDK adapters.

## Files

| File | Description |
|------|-------------|
| `host.js` | Main sidecar entry point |
| `test/` | Test directory |

## Usage

```bash
# Via the ABP CLI
cargo run -p abp-cli -- run --backend sidecar:node --task "hello"

# Standalone (for debugging)
echo '{"t":"run","id":"1","work_order":{"id":"1","task":"hello"}}' | node hosts/node/host.js
```

No dependencies required beyond Node.js itself.

## Capabilities

| Capability | Level |
|------------|-------|
| `streaming` | Native |
| `tool_read` | Emulated |
| `tool_write` | Emulated |
| `tool_edit` | Emulated |
| `structured_output_json_schema` | Emulated |

## Protocol

Speaks the ABP JSONL protocol over stdio. See [docs/sidecar_protocol.md](../../docs/sidecar_protocol.md).
