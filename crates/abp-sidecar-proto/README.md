# abp-sidecar-proto

Utilities for implementing the sidecar side of the ABP JSONL protocol.

This is the counterpart to `abp-host`: while `abp-host` manages sidecar
processes from the control-plane, `abp-sidecar-proto` provides helpers for
**writing** a sidecar that speaks the protocol over stdin/stdout.
