# abp-sidecar-utils

Reusable sidecar protocol utilities for Agent Backplane.

Provides higher-level building blocks on top of `abp-protocol`:

- **StreamingCodec** — Enhanced JSONL codec with chunked reading, line-length limits, error recovery, and metrics.
- **HandshakeManager** — Async hello handshake with timeout and version validation.
- **EventStreamProcessor** — Validates ref_id correlation, detects out-of-order events, tracks event counts.
- **ProtocolHealth** — Heartbeat, timeout detection, and graceful shutdown signaling.
