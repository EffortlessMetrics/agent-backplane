# Claude Agent TypeScript V1 Surface

The TS V1 surface (`hosts/claude/adapter.js`) is the default Claude Agent SDK adapter.
It uses the `@anthropic-ai/claude-agent-sdk` npm package.

## Transport modes

- **Query** (default): One-shot `sdk.query(prompt, options)` call
- **Client**: Session-based `ClaudeSDKClient()` with multi-turn capability
  - Enable via `abp.client_mode = true`
  - Session persistence via `abp.client_persist = true`
  - Timeout via `abp.client_timeout_ms`

## Options

Full option mapping with 30+ fields. See the
[integration guide](claude_agent_sdk_integration.md) for the complete table.

Key V1-specific options:
- `forkSession`: Fork an existing session
- `cliPath`: Custom Claude CLI path
- `extraArgs`: Additional CLI arguments
- `stderr`: stderr handling mode
- `maxBufferSize`: Maximum buffer size
- `includePartialMessages`: Emit partial/streaming events
- `persistSession`: Persist session state

## Retry behavior

V1 includes exponential backoff retry:
- Default: 1 retry with 1000ms delay
- Configurable via `claude.retryCount` / `claude.retryDelayMs`
- Environment overrides: `ABP_CLAUDE_RETRY_COUNT`, `ABP_CLAUDE_RETRY_DELAY_MS`
- Retriable errors: HTTP 408/409/425/429/500/502/503/504, timeouts, rate limits

## Session lifecycle

Events emitted during session operations:
- `session_started`: New session created
- `session_resumed`: Existing session resumed (when `resume` + `sessionId` provided)

## Hook events

SDK hook events are normalized to ABP event types:
- `pre_tool_use` → `tool_call` with `ext.hook = "pre_tool_use"`
- `post_tool_use` → `tool_result` with `ext.hook = "post_tool_use"`
- `post_tool_use_failure` → `tool_result` with `ext.hook = "post_tool_use_failure"`

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
| `subagents` | Native |
| `custom_tools` | Native |
| `interrupt` | Native |
| `extended_thinking` | Native |
| `tool_ask_user` | Native |
