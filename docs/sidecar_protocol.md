# Sidecar Protocol (JSONL)

> Contract version: `abp/v0.1`

Transport: **newline-delimited JSON** (JSONL) over stdio.

Every message is a single JSON object terminated by `\n`. The discriminator
field is **`t`** (not `type`).

---

## Lifecycle

```
  Control Plane                           Sidecar Process
  ═════════════                           ═══════════════

  spawn process ──────────────────────►   starts up
                                          │
                ◄── hello ────────────────┘  (MUST be first line)

  ── run {id, work_order} ──────────►

                ◄── event {ref_id} ──────   (zero or more)
                ◄── event {ref_id} ──────
                ...

                ◄── final {ref_id, receipt}  (success)
                    OR
                ◄── fatal {ref_id, error}    (failure)

                                             process may exit
```

---

## Envelope Types

### `hello`

First line a sidecar writes. Announces identity, capabilities, and execution
mode.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `t` | `"hello"` | yes | Discriminator |
| `contract_version` | `string` | yes | Protocol version (e.g. `"abp/v0.1"`) |
| `backend` | `BackendIdentity` | yes | `{ "id": "...", "backend_version": "...", "adapter_version": "..." }` |
| `capabilities` | `CapabilityManifest` | yes | Map of capability → support level |
| `mode` | `ExecutionMode` | no | `"passthrough"` or `"mapped"` (defaults to `"mapped"` if absent) |

```json
{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"my-sidecar","backend_version":"1.0.0"},"capabilities":{"streaming":"native","tool_read":"emulated"},"mode":"mapped"}
```

### `run`

Sent by the control plane to start a work order.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `t` | `"run"` | yes | Discriminator |
| `id` | `string` (UUID) | yes | Unique run identifier |
| `work_order` | `WorkOrder` | yes | The work order to execute |

```json
{"t":"run","id":"550e8400-e29b-41d4-a716-446655440000","work_order":{...}}
```

### `event`

Streamed by the sidecar during execution.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `t` | `"event"` | yes | Discriminator |
| `ref_id` | `string` (UUID) | yes | Must match the `run.id` |
| `event` | `AgentEvent` | yes | `{ "ts": "...", "type": "...", ... }` |

```json
{"t":"event","ref_id":"550e8400-...","event":{"ts":"2024-01-15T10:30:00Z","type":"assistant_delta","text":"Hello"}}
```

Event types include: `run_started`, `assistant_delta`, `assistant_message`,
`tool_call`, `tool_result`, `file_changed`, `command_executed`, `warning`,
`error`, `run_completed`.

### `final`

Concludes a successful run with a receipt.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `t` | `"final"` | yes | Discriminator |
| `ref_id` | `string` (UUID) | yes | Must match the `run.id` |
| `receipt` | `Receipt` | yes | Structured execution record |

```json
{"t":"final","ref_id":"550e8400-...","receipt":{...}}
```

### `fatal`

Sidecar signals an unrecoverable error.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `t` | `"fatal"` | yes | Discriminator |
| `ref_id` | `string` (UUID) | no | Match the `run.id` if available |
| `error` | `string` | yes | Human-readable error message |

```json
{"t":"fatal","ref_id":"550e8400-...","error":"ANTHROPIC_API_KEY not set"}
```

`ref_id` is optional on `fatal` because the error may occur before a run is
assigned (e.g. during initialization).

---

## Version Negotiation

The `contract_version` field in the `hello` envelope enables compatibility
checking.

**Format:** `"abp/vMAJOR.MINOR"` (e.g. `"abp/v0.1"`, `"abp/v0.2"`)

**Compatibility rule:** versions are compatible if they share the same
**major** version number.

| Control Plane | Sidecar | Compatible? |
|---------------|---------|-------------|
| `abp/v0.1` | `abp/v0.1` | ✓ |
| `abp/v0.1` | `abp/v0.2` | ✓ (same major) |
| `abp/v0.1` | `abp/v1.0` | ✗ (different major) |

If the versions are incompatible, the control plane should reject the sidecar
and report a protocol error.

---

## Transport-Level Extensions

The `sidecar-kit` transport layer adds frame types that are not part of the
`abp-protocol` Envelope but are used for process management:

### `cancel`

Sent by the control plane to request graceful cancellation of an in-progress
run.

```json
{"t":"cancel","ref_id":"550e8400-...","reason":"user requested"}
```

The sidecar should stop work and send a `final` (with partial receipt) or
`fatal` in response.

### `ping` / `pong`

Heartbeat mechanism for stall detection.

```json
{"t":"ping","seq":1}
{"t":"pong","seq":1}
```

The control plane sends `ping` periodically; the sidecar responds with
`pong` echoing the sequence number. Missing pong responses indicate a
stalled sidecar.

---

## Rules

1. Sidecar **MUST** send `hello` as its very first stdout line.
2. `event` and `final` envelopes **MUST** include `ref_id` matching the
   `run.id`.
3. `fatal` **SHOULD** include `ref_id` when the run ID is known.
4. Sidecar should treat stdin as the command queue; v0.1 assumes **one run
   at a time** per process.
5. Sidecar **MUST NOT** write non-JSON to stdout (redirect debug output to
   stderr).
6. Sidecar **SHOULD** respond to `ping` with `pong` for heartbeat.
7. Sidecar **SHOULD** honor `cancel` requests by stopping work promptly.

---

## Error Handling

| Situation | Control Plane Behavior |
|-----------|----------------------|
| Sidecar sends invalid JSON | `ProtocolError::Json` — parse failure |
| First line is not `hello` | `HostError::Violation` — handshake failed |
| `ref_id` doesn't match `run.id` | `ProtocolError::Violation` — correlation mismatch |
| Sidecar exits before `final`/`fatal` | `HostError::Exited` with exit code |
| Sidecar sends `fatal` | `HostError::Fatal` with error message |
| Incompatible `contract_version` | Protocol error — version mismatch |
| Pong not received within timeout | Sidecar considered stalled |

---

## Implementing a Sidecar

Minimal implementation checklist:

1. Read nothing from stdin until you have written `hello` to stdout.
2. Write a valid `hello` JSON line with `contract_version`, `backend`, and
   `capabilities`.
3. Read the `run` envelope from stdin (one JSON line).
4. Stream `event` envelopes to stdout as work progresses.
5. Write exactly one `final` (with receipt) or `fatal` envelope.
6. Exit cleanly (exit code 0 for success).

### Example (Node.js)

```javascript
const readline = require('readline');

// 1. Send hello
const hello = {
  t: 'hello',
  contract_version: 'abp/v0.1',
  backend: { id: 'my-node-sidecar' },
  capabilities: { streaming: 'native' },
};
process.stdout.write(JSON.stringify(hello) + '\n');

// 2. Read run
const rl = readline.createInterface({ input: process.stdin });
for await (const line of rl) {
  const msg = JSON.parse(line);
  if (msg.t === 'run') {
    const refId = msg.id;

    // 3. Stream events
    process.stdout.write(JSON.stringify({
      t: 'event',
      ref_id: refId,
      event: { ts: new Date().toISOString(), type: 'assistant_message', text: 'Done' },
    }) + '\n');

    // 4. Send final
    process.stdout.write(JSON.stringify({
      t: 'final',
      ref_id: refId,
      receipt: { /* ... */ },
    }) + '\n');

    break;
  }
}
```

See `hosts/node/`, `hosts/python/`, and `hosts/claude/` for complete examples.

