# Agent Backplane - Code Mode Rules

## Envelope Serialization (Critical)
- Use `#[serde(tag = "t", rename_all = "snake_case")]` for envelopes
- Tag field is `"t"`, NOT `"type"` - see [`abp-protocol/src/lib.rs:20`](crates/abp-protocol/src/lib.rs:20)

## Receipt Hash Implementation
```rust
// ALWAYS null the hash before computing - see abp-core/src/lib.rs:380
let mut v = serde_json::to_value(receipt)?;
if let serde_json::Value::Object(map) = &mut v {
    map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
}
```

## Backend Trait Pattern
- Implement [`Backend`](crates/abp-integrations/src/lib.rs:24) trait for new SDK adapters
- Stream events via `mpsc::Sender<AgentEvent>` - don't return until complete
- Always call `.with_hash()?` on receipt before returning

## Workspace Staging
- Use [`WorkspaceManager::prepare()`](crates/abp-workspace/src/lib.rs:33) - handles both modes
- Staged mode auto-inits git repo - don't add your own git init

## Adding New Capabilities
- Register in [`Capability`](crates/abp-core/src/lib.rs:153) enum first
- Map to `SupportLevel` in backend's `capabilities()` method
