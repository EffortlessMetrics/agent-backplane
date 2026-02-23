# Architecture

## Layers

Think of the system like a power distribution panel:

- The **contract** is your voltage/frequency standard.
- Each **SDK shim** is a plug adapter.
- Each **backend** is a generator.
- The **runtime** is the breaker box.

### `abp-core` (contract)

- Stable types: `WorkOrder`, `Receipt`, `AgentEvent`, capabilities.
- Must stay small and conservative.

### `abp-protocol` (wire)

- JSONL envelope for sidecars.
- Designed to be easy to implement in any language.

### `abp-host` (supervision)

- Spawns sidecar processes.
- Handles handshake, streams events, collects final receipt.

### `abp-workspace` (reversibility)

- Staged workspace creation.
- Git harness initialization.
- Diff/status capture for receipts.

### `abp-policy` (governance)

- Compiles allow/deny globs.
- Evaluates basic decisions.
- In v0.1 it’s a utility crate; enforcement happens in adapters.

### `abp-integrations` (backends)

- `Backend` trait.
- `MockBackend` for tests.
- `SidecarBackend` for “run an external adapter process”.

### `abp-runtime` (orchestration)

- Prepares workspace.
- Runs backend.
- Multiplexes event stream.
- Produces canonical receipt.

### `abp-cli` / `abp-daemon`

- CLI for local usage.
- Daemon is a stub for an eventual control-plane service.

## Sidecars vs in-process adapters

You will probably need both:

- **Sidecars** when the vendor SDK is only available in a language that isn’t Rust (Node/Python), or when you want isolation.
- **In-process adapters** when the SDK is Rust-native and you want minimal overhead.

The contract should not care which one you used; it only cares about receipts and capabilities.

## Projection matrix

You will inevitably have “non-isomorphic” concepts across SDKs:

- tool calling formats
- streaming payload shapes
- image/audio inputs
- structured output / JSON schema
- session state (resume/fork)

The design goal is not to pretend these are identical.

Instead:

- express **capabilities** precisely
- emulate when you can
- fail loudly when you cannot

