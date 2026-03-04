# Agent Backplane - AI Agent Context

## Build Commands
```bash
cargo build                          # Build all workspace crates
cargo test                           # Run all tests
cargo test -p <crate> <test_name>    # Run single test in workspace crate
cargo run -p xtask -- schema         # Generate JSON schemas to contracts/schemas/
cargo run -p abp-cli -- run --task "hello" --backend mock  # Run with mock backend
cargo run -p abp-cli -- backends     # List available backends
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
abp-glob ──────────┐
                    ├── abp-policy ──────────┐
abp-core ──────────┤                         │
  │   │            └── abp-workspace ────────┤
  │   │                      │               │
  │   ├── abp-ir ─── abp-mapper             │
  │   ├── abp-dialect ─── abp-mapping        │
  │   ├── abp-error ─── abp-error-taxonomy   │
  │   ├── abp-capability ─── abp-projection  │
  │   ├── abp-emulation                      │
  │   ├── abp-receipt ── abp-telemetry       │
  │   ├── abp-config                         │
  │   └── abp-sdk-types                      │
  │                                          │
abp-protocol ─── abp-host ─── abp-backend-core ─── abp-backend-mock
  │                  │              │                abp-backend-sidecar
  │              sidecar-kit        │
  │                  │         abp-integrations ─── abp-runtime ─── abp-cli
  │             *-bridge                                │             │
  │                                                  abp-stream   abp-daemon
  ├── abp-sidecar-proto                   abp-ratelimit
  └── abp-sidecar-utils

SDK shims: abp-shim-{openai,claude,gemini,codex,kimi,copilot}
Bridges:   {claude,gemini,openai,codex,copilot,kimi}-bridge
SDK crates: abp-{claude,codex,openai,gemini,kimi,copilot}-sdk
```

## Workspace Crates (54)

| Layer | Crates |
|-------|--------|
| Contract | abp-core, abp-ir, abp-sdk-types, abp-error, abp-error-taxonomy |
| Wire | abp-protocol, abp-sidecar-proto, abp-sidecar-utils |
| Infrastructure | abp-glob, abp-git, abp-workspace, abp-policy, abp-config |
| Dialect | abp-dialect, abp-mapper, abp-mapping, abp-projection, abp-capability, abp-emulation |
| Backend | abp-backend-core, abp-backend-mock, abp-backend-sidecar, abp-integrations |
| Runtime | abp-runtime, abp-stream, abp-receipt, abp-receipt-store, abp-telemetry, abp-ratelimit, abp-retry, abp-validate |
| Applications | abp-cli, abp-daemon |
| SDK Shims | abp-shim-openai, abp-shim-claude, abp-shim-gemini, abp-shim-codex, abp-shim-kimi, abp-shim-copilot |
| SDK Adapters | abp-claude-sdk, abp-codex-sdk, abp-openai-sdk, abp-gemini-sdk, abp-kimi-sdk, abp-copilot-sdk, abp-sidecar-sdk |
| Bridges | sidecar-kit, claude-bridge, gemini-bridge, openai-bridge, codex-bridge, copilot-bridge, kimi-bridge |

## Tracing Targets
- `abp.sidecar.stderr` - Sidecar stderr capture
- `abp.runtime` - Runtime events
- `abp.workspace` - Workspace staging
- `abp.sidecar` - Sidecar protocol I/O
