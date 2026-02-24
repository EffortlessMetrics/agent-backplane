# Claude Sidecar

This folder contains a Claude-oriented sidecar for Agent Backplane (ABP).

## Purpose

- Keep ABP as the system-of-record for work orders, events, policy, and receipts.
- Keep Claude SDK invocation details isolated behind an adapter boundary.

## Usage

From the repository root:

```powershell
cargo run -p abp-cli -- run --backend sidecar:claude --task "analyze this repo"
```

Install sidecar dependencies first:

```powershell
npm --prefix hosts/claude install
```

## Adapter model

`host.js` supports a custom adapter module:

```powershell
$env:ABP_CLAUDE_ADAPTER_MODULE="hosts/claude/my_claude_adapter.js"
cargo run -p abp-cli -- run --backend sidecar:claude --task "..."
```

The adapter module must export:

```js
module.exports = {
  name: "my_claude_adapter",
  version: "0.1.0",
  capabilities: {
    // optional ABP capability overrides
  },
  async run(ctx) {
    // invoke Claude Agent SDK here and stream through ctx.emit* helpers
    return {
      usageRaw: {},
      usage: {},
      outcome: "complete",
    };
  },
};
```

`ctx` includes helpers to emit ABP events (`emitAssistantDelta`, `emitAssistantMessage`, `emitToolCall`, `emitToolResult`, `emitWarning`, `emitError`) and to persist artifacts (`writeArtifact`).

## Defaults

Without a custom adapter:

- Mapped mode uses `hosts/claude/adapter.js` (Claude SDK adapter).
- Passthrough mode uses the host's native passthrough stream wrapper.
- If no compatible SDK is found, it runs deterministic fallback mode and emits `outcome: "partial"`.

## Client Mode (Feature Flag)

The built-in adapter now supports a stateful SDK-client path behind `vendor.abp.client_mode`.

```json
{
  "config": {
    "vendor": {
      "abp": {
        "client_mode": true,
        "client_persist": false,
        "client_timeout_ms": 120000
      }
    }
  }
}
```

Notes:

- Default remains `query()` mode for backward compatibility.
- If `client_mode=true` is set but the loaded SDK module has no client class/factory, the adapter emits a warning and falls back to `query()`.
- `client_persist=true` reuses the SDK client in-process by session key (`abp.client_session_key`, then `options.sessionId`, then workspace root).
- Receipt `usage_raw.transport` indicates which path was used (`\"query\"` or `\"client\"`).

## Environment variables

- `ABP_CLAUDE_ADAPTER_MODULE`: Path to the custom adapter module.
- `ABP_CLAUDE_MAX_INLINE_OUTPUT_BYTES`: Threshold for in-trace tool output before artifact offload (default `8192`).
- `ABP_CLAUDE_SDK_MODULE`: Optional SDK module override (useful for testing).
- `ABP_CLAUDE_CLIENT_TIMEOUT_MS`: Default timeout for client-mode query calls when no per-run timeout is set.
