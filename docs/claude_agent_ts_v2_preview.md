# Claude Agent TypeScript V2 Preview Surface

The TS V2 surface (`hosts/claude/adapter-v2.js`) is a separate adapter for the
unstable V2 preview API in the Claude Agent SDK.

> **Preview:** This surface uses `unstable_v2_*` methods. The API may change
> without notice. All V2 receipts carry `preview: true` in `usage_raw`.

## Activation

Enable V2 via work order config:
```json
{ "config": { "vendor": { "claude": { "v2_preview": true } } } }
```
Or: `claude.sdk_surface = "ts_v2_preview"`

## Transport modes

- **Prompt**: One-shot via `sdk.unstable_v2_prompt(prompt, options)`
- **Session**: Stateful via `sdk.unstable_v2_createSession(options)` / `session.send(prompt)`
- **Resume**: Session resume via `sdk.unstable_v2_resumeSession(sessionId, options)`

Transport is auto-selected: session when `sessionId` is present, prompt otherwise.

## V1-only options (rejected)

These V1 options are stripped with a warning in V2 mode:
- `forkSession`
- `cliPath`
- `extraArgs`
- `stderr`
- `maxBufferSize`

## Retry behavior

V2 now includes the same retry logic as V1:
- Default: 1 retry with 1000ms delay
- Configurable via `claude.retryCount` / `claude.retryDelayMs`
- Only retriable errors (HTTP 5xx, timeouts, rate limits) trigger retry

## Hook and event translation

V2 now translates the same event types as V1 and Python:
- Hook events: `pre_tool_use`, `post_tool_use`, `post_tool_use_failure`
- Permission events: `permission_request` → `permission_requested` + `permission_resolved`
- Notification: → `warning` with `ext.notification = true`
- Subagent: `subagent_start`/`subagent_stop` → `subagent_spawned`/`subagent_completed`
- Checkpoint: → `assistant_message` with `ext.checkpoint_id`

## Preview labeling

All V2 receipts include:
```json
{ "sdk_surface": "ts_v2_preview", "api_version": "v2_preview", "preview": true }
```

## Capabilities

Same as TS V1, plus:

| Capability | Level |
|-----------|-------|
| `structured_output_json_schema` | Native |
| `v2_preview` | Native |

## Unsupported V1 features

- `forkSession` — V2 sessions are not forkable
- `cliPath` / `extraArgs` — V2 does not use the CLI subprocess
- `stderr` / `maxBufferSize` — N/A for V2 transports
