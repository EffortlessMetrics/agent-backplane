# Agent Backplane - AI Agent Context

## Build Commands
```bash
cargo run -p xtask -- schema          # Generate JSON schemas to contracts/schemas/
cargo test -p <crate> <test_name>     # Run single test in workspace crate
```

## Critical Conventions

### Contract Version
- `CONTRACT_VERSION = "abp/v0.1"` in [`abp-core/src/lib.rs:14`](crates/abp-core/src/lib.rs:14)
- Used in all wire protocols and receipts

### Receipt Hashing (Gotcha)
- [`receipt_hash()`](crates/abp-core/src/lib.rs:380) sets `receipt_sha256` to `null` before hashing
- Never include the hash in its own input - self-referential prevention

### JSONL Protocol Handshake
- Sidecar MUST send `hello` envelope first (see [`docs/sidecar_protocol.md`](docs/sidecar_protocol.md))
- Envelope discriminator: `#[serde(tag = "t")]` - not `type`

### Staged Workspace Behavior
- Auto-initializes git repo with initial commit for meaningful diffs
- Excludes `.git` directory by default during copy

## Dependency Hierarchy
```
abp-core (stable contract, take only this if needed)
  ↑
abp-protocol (JSONL wire format)
  ↑
abp-host (sidecar supervision)
  ↑
abp-integrations (Backend trait)
  ↑
abp-runtime (orchestration)
```

## Tracing Targets
- `abp.sidecar.stderr` - Sidecar stderr capture
- `abp.runtime` - Runtime events
- `abp.workspace` - Workspace staging
