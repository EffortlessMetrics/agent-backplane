# Requirements

## Functional requirements

### Contract and compatibility

- **R1. Contract stability**: `abp-core` must be versioned and treated like an API surface.
- **R2. Schema generation**: contract types must have JSON Schemas generated via CI.
- **R3. Backwards compatibility**: changes are additive where possible. Breaking changes require a new contract version.

### Drop-in SDK shims

- **R4. Shim per SDK**: one package/crate per SDK surface area (e.g. `agent-backplane-openai-agents-sdk`, `agent-backplane-claude-sdk`).
- **R5. API fidelity**: the shim must match function names, parameters, return types, and streaming behavior closely enough to avoid application changes.
- **R6. Mapping completeness**: every SDK call must map to an internal command or fail with a deterministic error that names the missing capability.

### Routing

- **R7. Backend selection**: route based on config (env/CLI), per-request override, or policy.
- **R8. Capability negotiation**: before execution, verify that the chosen backend meets `CapabilityRequirements`.
- **R9. Graceful downgrade**: when possible, emulate missing features (e.g. tool calling in a provider without native tools) and mark the capability as `emulated`.

### Observability and receipts

- **R10. Event stream**: all backends must stream normalized events.
- **R11. Receipts**: every run produces a receipt containing:
  - backend identity and capability manifest
  - usage (raw + normalized)
  - full trace (events)
  - verification metadata (git diff/status when relevant)
  - outcome status
- **R12. Deterministic hashing**: receipts must be canonicalized and hashed (`receipt_sha256`).

### Workspace and harness

- **R13. Workspace modes**: pass-through vs staged.
- **R14. Git harness**: staged runs should have a baseline commit, so diffs are meaningful.
- **R15. Artifact capture**: support attaching artifacts by reference.

### Policy and governance

- **R16. Tool allow/deny**: policy can allow/deny tool usage.
- **R17. Path allow/deny**: policy can deny reading/writing specific globs.
- **R18. Network allow/deny**: policy can constrain outbound network.
- **R19. Approval hooks**: policy can require explicit approval for dangerous tools.

### Sidecars

- **R20. Sidecar protocol**: JSONL over stdio, with `hello/run/event/final/fatal`.
- **R21. Sidecar identity**: sidecars declare backend ID and capabilities.
- **R22. Sidecar isolation**: support running sidecars in separate processes/containers.

## Non-functional requirements

### Performance

- **N1. Low overhead**: mapping + routing should add minimal latency.
- **N2. Streaming fidelity**: do not buffer/aggregate unless required.

### Safety and correctness

- **N3. Explicit failure modes**: missing capability => deterministic error.
- **N4. Best-effort with labels**: emulation must be labeled in the capability manifest.

### Operability

- **N5. Microcrate structure**: small crates with single responsibility and minimal dependency edges.
- **N6. Logging**: structured logs in host/runtime.
- **N7. Testability**: mock backend for unit tests.

