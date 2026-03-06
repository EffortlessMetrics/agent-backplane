# abp-sidecar-utils

Reusable sidecar protocol utilities for Agent Backplane.

Provides higher-level building blocks on top of `abp-protocol` for
implementing sidecar hosts and clients:

- **StreamingCodec** -- Enhanced JSONL codec with chunked reading, line-length limits, error recovery, and metrics.
- **HandshakeManager** -- Async hello handshake with timeout and contract-version validation.
- **EventStreamProcessor** -- Validates ref_id correlation, detects out-of-order events, tracks event counts.
- **ProtocolHealth** -- Heartbeat, timeout detection, and graceful shutdown signaling.
- **SidecarProcess** -- Process management helpers for sidecar child processes.
- **StdioPipes** -- Stdio pipe setup and buffered I/O wrappers.
- **TimeoutManager** -- Per-phase timeout management for the sidecar protocol lifecycle.
- **SidecarRegistry** -- Sidecar discovery and registration.

## Key Types

| Type | Description |
|------|-------------|
| `StreamingCodec` | Enhanced JSONL codec with error recovery and metrics |
| `HandshakeManager` | Manages the hello handshake with timeout and version checks |
| `EventStreamProcessor` | Validates event correlation and ordering |
| `ProtocolHealth` | Heartbeat and shutdown signaling |
| `SidecarProcess` | Process lifecycle management for sidecar children |
| `TimeoutManager` | Per-phase timeout tracking |
| `SidecarRegistry` | Discovery and registration of available sidecars |

## Usage

```rust,no_run
use abp_sidecar_utils::{encode_hello, decode_envelope, validate_hello};
use abp_core::{BackendIdentity, CapabilityManifest};

let identity = BackendIdentity {
    id: "my-sidecar".into(),
    backend_version: Some("1.0".into()),
    adapter_version: None,
};
let line = encode_hello(&identity, &CapabilityManifest::new()).unwrap();
let envelope = decode_envelope(&line).unwrap();
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
