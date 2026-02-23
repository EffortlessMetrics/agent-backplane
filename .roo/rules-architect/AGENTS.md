# Agent Backplane - Architect Mode Rules

## Architectural Constraints

### Crate Dependency Direction (Strict)
```
abp-core → abp-protocol → abp-host → abp-integrations → abp-runtime
```
- `abp-core` must have NO dependencies on other abp crates
- Reverse dependencies will cause circular imports

### Contract Stability
- `abp-core` types are the stable contract - changes require version bump
- `CONTRACT_VERSION` must be updated for breaking changes
- New enum variants must be added at end for serde compatibility

### Backend Trait Contract
- Backends MUST stream events via channel - no polling
- Backends MUST return `Receipt` with valid hash
- See [`Backend` trait](crates/abp-integrations/src/lib.rs:24)

## Extension Points

### Adding New Backend
1. Implement `Backend` trait in separate crate or `abp-integrations`
2. Register with `Runtime::register_backend()`
3. Sidecar backends use JSONL protocol via `SidecarBackend`

### Adding New Capability
1. Add to `Capability` enum in [`abp-core/src/lib.rs:153`](crates/abp-core/src/lib.rs:153)
2. Add `SupportLevel` mapping in backend implementations
3. Update `CapabilityManifest` docs

### Adding New Envelope Type
1. Add variant to `Envelope` enum in [`abp-protocol/src/lib.rs:21`](crates/abp-protocol/src/lib.rs:21)
2. Update [`docs/sidecar_protocol.md`](docs/sidecar_protocol.md)
3. Regenerate schemas: `cargo run -p xtask -- schema`

## Workspace Modes
- `PassThrough`: Direct workspace access - no isolation
- `Staged`: Copy to temp dir, auto-init git - safe for mutation

## Policy Enforcement
- Policy checks in `abp-policy` are best-effort in v0.1
- Actual enforcement delegated to backends
