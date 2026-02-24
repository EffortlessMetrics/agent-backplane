# GitHub Copilot SDK Integration Guide for Agent Backplane

This guide defines the implemented `sidecar:copilot` integration in this repository.

## 1) Why this is implemented as a micro-sidecar

- The GitHub ecosystem already owns LLM orchestration, context compaction, model routing, and tool execution via the Copilot runtime.
- ABP keeps `abp-core` stable by confining vendor behavior to `hosts/copilot/*`.
- The sidecar provides a deterministic boundary for receipts, event streams, and hashing.
- Policy can be enforced once at the boundary before tool invocation.

## 2) Runtime topology

```text
WorkOrder -> abp-runtime -> host runtime -> ABP host process (hosts/copilot/host.js)
   -> adapter (hosts/copilot/adapter.js) -> Copilot ACP transport (stdlib stdio/tcp)
   -> optional legacy runner fallback (ABP_COPILOT_RUNNER)
```

This split is intentional:

- `host.js` owns protocol, policy preflight, artifacts, receipts.
- `adapter.js` owns Copilot transport details.
- external runner logic can be swapped without changing ABP internals.

## 3) Protocol behavior in this stack

The sidecar uses the ABP JSONL envelope:

- `hello` must be first.
- each request is a `run` envelope with a full `work_order`.
- progress/events are emitted with `event` envelopes.
- final output is a `final` envelope containing an ABP `Receipt`.

`abp-host` enforces envelope order and hash injection at runtime. Any `receipt_sha256`
from the sidecar is replaced with the runtime canonical hash.

## 4) WorkOrder mapping

`hosts/copilot/adapter.js` currently consumes:

- `work_order.task`
- `work_order.workspace.root`
- `work_order.context` (files/snippets)
- `work_order.policy` (`allowed_tools`, `disallowed_tools`, `deny_*`, `allow_network`, `deny_network`, `require_approval_for`)
- `work_order.config.vendor.copilot`
- `work_order.config.vendor.abp.mode`

### Copilot vendor fields consumed

- `model`
- `reasoningEffort`
- `systemMessage`
- `availableTools`
- `excludedTools`
- `mcpServers` / `mcp_servers`

### ABI mode

- `work_order.config.vendor.abp.mode = passthrough` is accepted and reflected in receipt.
- mapped mode remains the default path and is where most behavior normalization lives.

## 5) Capabilities exposed

`hosts/copilot/capabilities.js` publishes these canonical capabilities:

- `streaming`
- `tool_read`, `tool_write`, `tool_edit`, `tool_bash`
- `tool_glob`, `tool_grep`
- `tool_web_search`, `tool_web_fetch`
- `structured_output_json_schema`
- `hooks_pre_tool_use`, `hooks_post_tool_use`
- `session_resume`, `session_fork`
- `checkpointing`
- `mcp_client`, `mcp_server`
- `tool_ask_user`

This manifest is what runtime checks use when capability requirements are declared.

## 6) Security and governance at the boundary

`host.js` enforces these pre-flight checks for every tool call:

- hard tool allow/deny patterns
- path checks against `work_order.workspace.root`
- deny rules for read/write paths
- optional network host allow/deny
- `require_approval_for` short-circuit (`warning` + denied tool result)

Security defaults are policy-driven, so they can evolve without changing the sidecar protocol.

## 7) Adapter contract (implemented)

`adapter.js` expects the following normalized shape:

- `request_id`, `prompt`, `workspace_root`, `model`, `reasoningEffort`
- `systemMessage`, `context`, `policy`, `availableTools`, `excludedTools`, `mcpServers`
- `streaming`, `raw_request`
- optional `env`

Runtime mode behavior:

- `ABP_COPILOT_PROTOCOL=acp` (default): start local Copilot in ACP mode or connect to an ACP endpoint, then run `initialize`, session creation, and `session/prompt` flow.
- `ABP_COPILOT_PROTOCOL=legacy`: send request JSON to a runner/command for line-delimited event mode.

ACP protocol mapping:

- `initialize` (fallback: `initializeClient`)
- `session/loadClient` (fallback: `session/load`) if `sessionId` exists
- `session/newClient` (fallback: `session/new`)
- `session/promptClient` (fallback: `session/prompt`)
- `session/request_permission` is answered with `{ decision, approval, action, reason }` and can be `allow_once`, `allow_always`, or `reject`.

Runner mode mapping:

- `ABP_COPILOT_RUNNER` (preferred): any executable that consumes the JSON request from stdin and emits JSON/line events.
- `ABP_COPILOT_CMD` + `ABP_COPILOT_ARGS`: fallback command mode.
- if no runner configured, adapter returns a deterministic fallback explanation and outcome `partial`.

Runner-emitted event kinds are normalized into ABP events:

- `assistant_delta`, `assistant_message`
- `tool_call`, `tool_result`
- `warning`, `error`
- `usage` for usage extraction

## 8) Artifacts and receipts

The host:

- writes `RunStarted`/`RunCompleted` markers,
- streams all recognized events to ABP,
- records tool call/results in trace,
- stores large tool output into `.agent-backplane/artifacts/<run_id>` with `ArtifactRef` entries,
- computes usage and receipt hash with deterministic nulling of `receipt_sha256`.

## 9) End-to-end activation

```bash
cargo run -p abp-cli -- run --backend sidecar:copilot --task "audit copilot compatibility"
```

Optional runtime overrides:

- `ABP_COPILOT_ADAPTER_MODULE=./path/to/adapter.js`
- `ABP_COPILOT_RUNNER=./bin/copilot-runner`
- `ABP_COPILOT_CMD=copilot`
- `ABP_COPILOT_ARGS='["agent","--acp"]'`
- `ABP_COPILOT_PROTOCOL=acp`
- `ABP_COPILOT_ACP_URL=tcp://127.0.0.1:3000`
- `ABP_COPILOT_ACP_PORT=3000`
- `ABP_COPILOT_ACP_ARGS='["agent","--acp","--stdio"]'`
- `ABP_COPILOT_PERMISSION_ALLOW_ALWAYS_TOOLS='["Write","Bash"]'`

## 10) Delivery path for production use

1. replace `hosts/copilot/adapter.js` with a real Copilot runner binding (or set `ABP_COPILOT_RUNNER`),
2. keep `host.js` unchanged unless event/receipt schema changes are required,
3. use policy assertions in integration tests:
   - hello-first ordering,
   - denied tool path checks,
   - require_approval_for rejection,
   - receipt hash determinism,
   - artifact persistence.
