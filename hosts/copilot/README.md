# GitHub Copilot Sidecar for Agent Backplane

This directory contains the GitHub Copilot sidecar implementation for Agent Backplane.

The Copilot host follows the ABP JSONL protocol and emits normalized ABP events
(`hello`, `event`, `final`) while delegating Copilot-specific behavior to an adapter.

## Install

```bash
npm --prefix hosts/copilot install
```

## Usage

```bash
# Start the sidecar directly
node hosts/copilot/host.js

# Use in ABP CLI (from repo root)
set GH_TOKEN=YOUR_TOKEN
cargo run -p abp-cli -- run --backend copilot --task "inspect this repo"

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
│ - request_permission handling          │
├───────────────────────────────────────┤
│ adapter.run(ctx) → emits ABP events    │
│        ↕                              │
│ Copilot adapter (adapter.js or custom) │
│ - invoke ACP transport by default       │
│ - map tool calls + usage               │
└───────────────────────────────────────┘
```

## Files

| File | Purpose |
|------|---------|
| `host.js` | Sidecar runtime, JSONL protocol handling, events, receipt |
| `adapter.js` | Default adapter with SDK-first transport and ACP/legacy fallback |
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
| `ABP_COPILOT_TRANSPORT` | `auto` (default), `sdk`, `acp`, or `legacy` | `auto` |
| `ABP_COPILOT_PROTOCOL` | Back-compat switch (`acp` / `legacy`) when transport not set | `acp` |
| `ABP_COPILOT_ACP_URL` | Remote ACP endpoint (`host:port`/`tcp://...`) | `` |
| `ABP_COPILOT_ACP_PORT` | Local ACP TCP port (stdio omitted) | `` |
| `ABP_COPILOT_ACP_ARGS` | JSON args for ACP startup process | `[]` |
| `ABP_COPILOT_SDK_MODULE` | Override SDK import path (tests/custom builds) | `@github/copilot-sdk` |
| `ABP_COPILOT_RETRY_ATTEMPTS` | SDK retry attempts for transient errors | `3` |
| `ABP_COPILOT_RETRY_BASE_DELAY_MS` | SDK retry base delay | `1000` |
| `ABP_COPILOT_PERMISSION_ALLOW_ALWAYS` | Allow every request (`allow_always`) | `false` |
| `ABP_COPILOT_PERMISSION_ALLOW_TOOLS` | Auto-approve listed tools (`allow_once`) | `[]` |
| `ABP_COPILOT_PERMISSION_DENY_TOOLS` | Quick deny list by prefix | `[]` |
| `ABP_COPILOT_PERMISSION_ALLOW_ALWAYS_TOOLS` | Always auto-allow list | `[]` |
| `ABP_COPILOT_PERMISSION_DENY_ALWAYS_TOOLS` | Always deny list | `[]` |

## Protocol Notes

- `hello` must be first output line.
- The adapter receives `workOrder` plus normalized policy helpers.
- `ABP_COPILOT_TRANSPORT=auto` tries SDK first, then ACP, then legacy runner.
- Receipts follow the ABP contract and include deterministic `receipt_sha256`.

## Minimal Security Posture

- Policy is enforced from `work_order.policy` and runtime defaults from the CLI.
- Typical default policy from `abp-cli` blocks:
  - `KillBash`, `NotebookEdit`
  - write paths under `.git`
  - `deny_read` / `deny_write` patterns
- The sidecar handles `session/request_permission` from ACP and can auto-approve
  or reject based on allow/deny lists.
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
