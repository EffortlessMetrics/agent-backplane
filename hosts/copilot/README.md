# GitHub Copilot Sidecar for Agent Backplane

This directory contains the GitHub Copilot sidecar implementation for Agent Backplane.

The Copilot host follows the ABP JSONL protocol and emits normalized ABP events
(`hello`, `event`, `final`) while delegating Copilot-specific behavior to an adapter.

## Usage

```bash
# Start the sidecar directly
node hosts/copilot/host.js

# Use in ABP CLI (from repo root)
cargo run -p abp-cli -- run --backend sidecar:copilot --task "inspect this repo"

# Debug with a custom adapter module
ABP_COPILOT_ADAPTER_MODULE=./hosts/copilot/adapter.template.js \
  node hosts/copilot/host.js
```

## Architecture

```
┌───────────────────────────────────────┐
│ GitHub Copilot Sidecar (host.js)      │
├───────────────────────────────────────┤
│ - protocol handshake + event stream     │
│ - policy enforcement                   │
│ - artifact capture                     │
│ - receipt assembly                     │
├───────────────────────────────────────┤
│ adapter.run(ctx) → emits ABP events    │
│        ↕                              │
│ Copilot adapter (adapter.js or custom) │
│ - invoke CLI / SDK                    │
│ - map tool calls + usage                │
└───────────────────────────────────────┘
```

## Files

| File | Purpose |
|------|---------|
| `host.js` | Sidecar runtime, JSONL protocol handling, events, receipt |
| `adapter.js` | Default adapter scaffold for Copilot integration |
| `adapter.template.js` | Template showing a custom Copilot integration module |
| `capabilities.js` | Capability manifest and support levels |

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ABP_COPILOT_ADAPTER_MODULE` | Custom adapter module path | `./adapter.js` |
| `ABP_COPILOT_MAX_INLINE_OUTPUT_BYTES` | Max inline artifact size | `8192` |
| `ABP_COPILOT_RUNNER` | Optional command that accepts Copilot request JSON on stdin |
| `ABP_COPILOT_CMD` | Default command name for non-runner flows | `copilot` |
| `ABP_COPILOT_ARGS` | JSON array of arguments for `ABP_COPILOT_CMD` | `[]` |

## Protocol Notes

- `hello` must be first output line.
- The adapter receives fully built `workOrder` plus normalized policy helpers.
- Receipts follow the ABP contract and include deterministic `receipt_sha256`.

## Minimal Security Posture

- Policy is enforced from `work_order.policy` and runtime defaults from the CLI.
- Typical default policy from `abp-cli` blocks:
  - `KillBash`, `NotebookEdit`
  - write paths under `.git`
  - `deny_read` / `deny_write` patterns
- The sidecar also rejects tools marked in `require_approval_for` until a custom
  permission callback is added.
- Path checks are relative to `workOrder.workspace.root` when path-like arguments are present.

## Example custom adapter integration

```js
module.exports = {
  name: "my_copilot_adapter",
  version: "0.1.0",
  async run(ctx) {
    const { emitAssistantMessage, emitToolCall, emitToolResult, emitError } = ctx;
    // Call your Copilot transport here (SDK, custom runner, etc.)
    emitAssistantMessage("copilot adapter started");
    emitToolCall({
      toolName: "example_tool",
      toolUseId: "toolu_01",
      input: { example: true },
    });
    emitToolResult({
      toolName: "example_tool",
      toolUseId: "toolu_01",
      output: { status: "ok" },
    });
    return { usageRaw: { mock: true }, usage: {}, outcome: "complete" };
  },
};
```
