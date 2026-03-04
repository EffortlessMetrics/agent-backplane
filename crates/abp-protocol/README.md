# abp-protocol

JSONL wire protocol for Agent Backplane sidecar communication.

## Key Types

| Type | Description |
|------|-------------|
| `Envelope` | JSONL message envelope — discriminated by `#[serde(tag = "t")]` |
| `JsonlCodec` | Stateless codec for encoding/decoding envelopes over stdio |
| `ProtocolError` | Errors from JSONL encoding/decoding or protocol violations |

## Protocol Flow

```text
Sidecar ──hello──▸ Control Plane     (identity + capabilities)
Sidecar ◂──run─── Control Plane      (WorkOrder)
Sidecar ──event──▸ Control Plane     (AgentEvent stream)
Sidecar ──final──▸ Control Plane     (Receipt)
```

## Usage

```rust
use abp_core::{BackendIdentity, CapabilityManifest};
use abp_protocol::{Envelope, JsonlCodec};

let hello = Envelope::hello(
    BackendIdentity {
        id: "my-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    },
    CapabilityManifest::new(),
);

let line = JsonlCodec::encode(&hello).unwrap();
let decoded = JsonlCodec::decode(&line).unwrap();
```

> **Note:** The envelope discriminator field is `t` (not `type`).

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
