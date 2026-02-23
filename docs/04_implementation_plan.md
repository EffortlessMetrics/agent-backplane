# Implementation plan

## Milestone 0: contract hardening

- [ ] Freeze `abp-core` v0.1 types (WorkOrder, Receipt, events).
- [ ] Generate JSON schemas in CI.
- [ ] Add compatibility tests (schema diff).

## Milestone 1: sidecar adapters

- [ ] Implement a “reference sidecar” in Node and Python (this repo includes simple examples).
- [ ] Add adapter conformance tests:
  - handshake must be first line
  - event stream ordering
  - receipt required fields

## Milestone 2: first real SDK shim

Pick one SDK and ship a drop-in shim.

- [ ] `agent-backplane-<sdk>` package exposing the SDK’s public API.
- [ ] Shim maps each call to a WorkOrder/internal commands.
- [ ] Sidecar backend routes to the real provider.
- [ ] Receipt emitted and validated.

## Milestone 3: projection matrix

- [ ] Capability registry + satisfiability checks.
- [ ] Per-backend capability manifests.
- [ ] Emulation layer:
  - emulate tool calling for providers without tools
  - emulate structured output with JSON validator post-check

## Milestone 4: governance

- [ ] Enforce policy in adapters.
- [ ] Approval hooks / UI callbacks.
- [ ] Sandbox option (container execution).

## Milestone 5: daemon + persistence

- [ ] HTTP daemon API
- [ ] receipt store
- [ ] replay/debug tools

