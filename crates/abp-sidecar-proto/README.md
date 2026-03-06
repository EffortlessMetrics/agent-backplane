# abp-sidecar-proto

Sidecar-side helpers for the ABP JSONL protocol.

This is the counterpart to `abp-host`: while `abp-host` manages sidecar
processes from the control plane, this crate provides utilities for
*implementing* a sidecar in Rust that speaks the protocol over stdin/stdout.
Includes a `SidecarServer` that handles the full protocol lifecycle and an
`EventSender` for streaming events back to the control plane.

## Key Types

| Type | Description |
|------|-------------|
| `SidecarServer` | Reads JSONL from stdin and dispatches to a `SidecarHandler` |
| `SidecarHandler` | Trait implemented by sidecar authors to handle work orders |
| `EventSender` | Channel-based handle for streaming events and receipts |
| `SidecarProtoError` | Error type for protocol, I/O, and handler failures |

## Usage

```rust,no_run
use abp_sidecar_proto::{SidecarServer, SidecarHandler, EventSender, SidecarProtoError};
use abp_core::{WorkOrder, BackendIdentity, CapabilityManifest};
use async_trait::async_trait;

struct MyHandler;

#[async_trait]
impl SidecarHandler for MyHandler {
    async fn on_run(
        &self,
        _run_id: String,
        _wo: WorkOrder,
        sender: EventSender,
    ) -> Result<(), SidecarProtoError> {
        // stream events via sender, then send_final with a receipt
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let identity = BackendIdentity {
        id: "my-sidecar".into(),
        backend_version: Some("1.0".into()),
        adapter_version: None,
    };
    let server = SidecarServer::new(MyHandler, identity, CapabilityManifest::new());
    server.run().await.unwrap();
}
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
