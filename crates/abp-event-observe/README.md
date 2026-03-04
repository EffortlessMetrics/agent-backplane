# abp-event-observe

Small, SRP-focused observability helpers for `AgentEvent` streams:

- `EventRecorder` for replay/inspection snapshots
- `EventStats` for aggregate counters and sizes
- `event_kind_name` for stable event-kind labels

This crate is intentionally runtime-agnostic so stream and host layers can share it.
