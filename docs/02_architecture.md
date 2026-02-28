# Architecture

## Layers

Think of the system like a power distribution panel:

- The **contract** is your voltage/frequency standard.
- Each **SDK shim** is a plug adapter.
- Each **backend** is a generator.
- The **runtime** is the breaker box.
- The new GitHub Copilot sidecar is another isolated generator on the same bus.

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
- `abp-daemon` exposes a basic HTTP control-plane API and persists run receipts.

The stack prefers microcrates: small, single-purpose modules with one clear dependency edge.

## GitHub Copilot and Kimi sidecars in scope (microcrate pattern)

This repo now includes dedicated sidecar scaffolds and registration microcrates under:

- `hosts/copilot`
- `hosts/kimi`
- `crates/abp-kimi-sdk`
- `crates/abp-sidecar-sdk` (shared SRP helper for sidecar registration)

- `host.js`: protocol binding, policy gatekeeping, artifact capture, receipt assembly.
- `adapter.js`: default adapter entrypoint that can be replaced by `ABP_COPILOT_ADAPTER_MODULE`.
- `capabilities.js`: declared contract mapping for Copilot-compatible behaviors (tools, web, ACP/MCP hooks, sessions).

The integration intentionally keeps ABI and orchestration concerns in `abp-core` /
`abp-runtime`, while Copilot execution remains isolated in the host boundary.

This follows the microcrate pattern, with `abp-sidecar-sdk` isolating shared command/script registration behavior:

- no contract changes required,
- no runtime changes beyond backend registration,
- swap-in behavior by changing adapter module or host-level policy/environment.

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

### GitHub Copilot integration status

- Backend wire-up:
- `sidecar:copilot` and `sidecar:kimi` are available in `abp-cli` and `abp-daemon` when Node runtime is present.
  - hello/`run`/`event`/`final` protocol remains unchanged.
- Integration extension points:
  - `ABP_COPILOT_ADAPTER_MODULE` to inject your real SDK binding.
  - `ABP_COPILOT_RUNNER` for process-based runners that accept ABI-shaped JSON request payloads.
- `work_order.config.vendor.copilot` for Copilot overrides and `work_order.config.vendor.kimi` for Kimi overrides (`model`, `reasoningEffort`, `agentMode`, `agentSwarm`, `topP`, tool policy).


