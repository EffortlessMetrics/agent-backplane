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

- The sidecar tries a best-effort probe for `@anthropic-ai/claude-agent-sdk` and `claude-agent-sdk`.
- If no compatible entrypoint is found, it runs a deterministic fallback mode and emits `outcome: "partial"`.

## Environment variables

- `ABP_CLAUDE_ADAPTER_MODULE`: Path to the custom adapter module.
- `ABP_CLAUDE_MAX_INLINE_OUTPUT_BYTES`: Threshold for in-trace tool output before artifact offload (default `8192`).
