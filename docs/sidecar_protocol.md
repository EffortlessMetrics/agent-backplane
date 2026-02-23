# Sidecar protocol (JSONL)

Transport: **newline-delimited JSON** over stdio.

## Envelope types

### `hello`

First line a sidecar writes.

```json
{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"..."},"capabilities":{"streaming":"native"}}
```

### `run`

Sent by the control plane.

```json
{"t":"run","id":"<run_id>","work_order":{...}}
```

### `event`

Streamed by sidecar.

```json
{"t":"event","ref_id":"<run_id>","event":{"ts":"...","type":"assistant_message","text":"..."}}
```

### `final`

Final receipt.

```json
{"t":"final","ref_id":"<run_id>","receipt":{...}}
```

### `fatal`

Sidecar cannot proceed.

```json
{"t":"fatal","ref_id":"<run_id>","error":"..."}
```

## Rules

- Sidecar **must** send `hello` first.
- `event` and `final` must include `ref_id` matching the `run.id`.
- Sidecar should treat stdin as the command queue; v0.1 assumes one run at a time.

