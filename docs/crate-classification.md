# Crate Classification & Publish Surface Audit

This document classifies all 55 workspace crates into publication tiers
and audits the dependency chain to identify which crates can safely be
marked `publish = false`.

## Tiers

| Tier | Meaning |
|------|---------|
| **public-stable** | Intended for external use. API changes are breaking changes. |
| **public-experimental** | Published and usable, but API may change before 1.0. |
| **internal** | Published only because a publishable crate depends on it. Not intended for direct external use. |
| **unpublished** | Already `publish = false`. Not on crates.io. |

## Classification

### Public-stable

Core contract types and standalone utilities. These are the crates
external users should depend on.

| Crate | Workspace deps |
|-------|----------------|
| `abp-core` | `abp-error` |
| `abp-error` | — |
| `abp-protocol` | `abp-core`, `abp-error`, `sidecar-kit` |
| `abp-glob` | — |
| `abp-config` | — |
| `sidecar-kit` | `abp-core` |

### Public-experimental

Published and usable. APIs are not yet frozen.

| Crate | Workspace deps |
|-------|----------------|
| **CLI & daemon** | |
| `abp-cli` | `abp-claude-sdk`, `abp-codex-sdk`, `abp-config`, `abp-copilot-sdk`, `abp-core`, `abp-dialect`, `abp-gemini-sdk`, `abp-host`, `abp-integrations`, `abp-kimi-sdk`, `abp-mapper`, `abp-runtime` |
| `abp-daemon` | `abp-claude-sdk`, `abp-codex-sdk`, `abp-config`, `abp-copilot-sdk`, `abp-core`, `abp-dialect`, `abp-gemini-sdk`, `abp-host`, `abp-integrations`, `abp-kimi-sdk`, `abp-projection`, `abp-runtime` |
| **SDK shims** | |
| `abp-shim-openai` | `abp-core`, `abp-openai-sdk`, `abp-sdk-types` |
| `abp-shim-claude` | `abp-claude-sdk`, `abp-core`, `abp-sdk-types` |
| `abp-shim-gemini` | `abp-core`, `abp-gemini-sdk` |
| `abp-shim-codex` | `abp-codex-sdk`, `abp-core` |
| `abp-shim-kimi` | `abp-core`, `abp-kimi-sdk` |
| `abp-shim-copilot` | `abp-copilot-sdk`, `abp-core` |
| **Vendor bridges** | |
| `claude-bridge` | `abp-core`, `abp-dialect`, `sidecar-kit` |
| `openai-bridge` | `abp-core`, `abp-sdk-types`, `sidecar-kit` |
| `gemini-bridge` | `abp-core`, `abp-sdk-types`, `sidecar-kit` |
| `codex-bridge` | `abp-codex-sdk`, `abp-dialect` |
| `kimi-bridge` | `abp-core`, `abp-dialect`, `sidecar-kit` |
| `copilot-bridge` | `abp-core`, `abp-sdk-types`, `sidecar-kit` |
| **Receipt & policy** | |
| `abp-receipt` | `abp-core` |
| `abp-policy` | `abp-core`, `abp-glob` |
| **Sidecar helpers** | |
| `abp-sidecar-proto` | `abp-core`, `abp-protocol` |
| `abp-sidecar-utils` | `abp-core`, `abp-protocol` |

### Internal

These crates are published because they are in the transitive dependency
chain of `abp-cli`, `abp-daemon`, or the shim/bridge crates. They are
not intended for direct external consumption.

| Crate | Workspace deps | Required by |
|-------|----------------|-------------|
| `abp-backend-core` | `abp-core` | `abp-integrations` → `abp-runtime` |
| `abp-backend-mock` | `abp-backend-core`, `abp-core` | `abp-integrations` |
| `abp-backend-sidecar` | `abp-backend-core`, `abp-core`, `abp-host` | `abp-integrations` |
| `abp-capability` | `abp-core`, `abp-sdk-types` | `abp-projection`, `abp-emulation` → `abp-runtime` |
| `abp-dialect` | — | `abp-mapper`, `abp-projection`, `abp-validate` |
| `abp-emulation` | `abp-capability`, `abp-core` | `abp-runtime` |
| `abp-host` | `abp-core`, `abp-error`, `abp-protocol`, `sidecar-kit` | `abp-backend-sidecar`, `abp-sidecar-sdk` |
| `abp-integrations` | `abp-backend-core`, `abp-backend-mock`, `abp-backend-sidecar`, `abp-claude-sdk`, `abp-codex-sdk`, `abp-core`, `abp-gemini-sdk`, `abp-host`, `abp-kimi-sdk`, `abp-openai-sdk` | `abp-runtime` |
| `abp-mapper` | `abp-core`, `abp-dialect`, `abp-sdk-types` | `abp-cli`, `abp-projection` |
| `abp-mapping` | `abp-dialect` | `abp-projection` |
| `abp-projection` | `abp-capability`, `abp-core`, `abp-dialect`, `abp-mapper`, `abp-mapping` | `abp-daemon`, `abp-runtime` |
| `abp-ratelimit` | — | `abp-runtime` |
| `abp-retry` | — | `abp-runtime` |
| `abp-runtime` | `abp-backend-core`, `abp-capability`, `abp-core`, `abp-dialect`, `abp-emulation`, `abp-error`, `abp-integrations`, `abp-policy`, `abp-projection`, `abp-ratelimit`, `abp-receipt`, `abp-retry`, `abp-stream`, `abp-validate`, `abp-workspace` | `abp-cli`, `abp-daemon`, `abp-sidecar-sdk` |
| `abp-sdk-types` | — | `abp-capability`, `abp-mapper`, `abp-ir`, shims, bridges |
| `abp-sidecar-sdk` | `abp-core`, `abp-error`, `abp-host`, `abp-integrations`, `abp-protocol`, `abp-runtime` | `abp-*-sdk` vendor adapters |
| `abp-stream` | `abp-core` | `abp-runtime` |
| `abp-validate` | `abp-core`, `abp-dialect`, `abp-protocol` | `abp-runtime` |
| `abp-workspace` | `abp-core`, `abp-git`, `abp-glob` | `abp-runtime` |
| `abp-git` | — | `abp-workspace` |
| **Vendor SDK adapters** | | |
| `abp-claude-sdk` | `abp-core`, `abp-runtime`, `abp-sidecar-sdk` | `abp-integrations`, `abp-shim-claude` |
| `abp-codex-sdk` | `abp-core`, `abp-runtime`, `abp-sdk-types`, `abp-sidecar-sdk` | `abp-integrations`, `abp-shim-codex`, `codex-bridge` |
| `abp-openai-sdk` | `abp-core`, `abp-host`, `abp-integrations`, `abp-runtime` | `abp-shim-openai` |
| `abp-gemini-sdk` | `abp-core`, `abp-runtime`, `abp-sidecar-sdk` | `abp-integrations`, `abp-shim-gemini` |
| `abp-kimi-sdk` | `abp-core`, `abp-runtime`, `abp-sdk-types`, `abp-sidecar-sdk` | `abp-integrations`, `abp-shim-kimi` |
| `abp-copilot-sdk` | `abp-core`, `abp-runtime`, `abp-sdk-types`, `abp-sidecar-sdk` | `abp-integrations`, `abp-shim-copilot` |

### Unpublished

Already marked `publish = false`.

| Crate | Reason |
|-------|--------|
| `agent-backplane` | Root workspace metapackage |
| `xtask` | Build automation |

## Orphaned crates

These crates are **not** in the transitive dependency chain of any
publishable binary or library crate. They are only depended on by the
root `agent-backplane` metapackage (which is `publish = false`).

They are candidates for `publish = false` in a follow-up PR **if** they
are not intended for standalone external use.

| Crate | Workspace deps | Notes |
|-------|----------------|-------|
| `abp-error-taxonomy` | `abp-error` | Error classification helpers. Useful for tooling that maps error codes to severity/recovery. |
| `abp-ir` | `abp-core`, `abp-sdk-types` | IR normalization. Useful for users building custom cross-dialect mappers. |
| `abp-receipt-store` | `abp-core` | Receipt persistence. Useful for operators who want receipt storage beyond files. |
| `abp-telemetry` | — | Telemetry primitives. No workspace deps — fully standalone. |

**Decision needed:** Should any of these remain published for standalone
use, or should they all be marked `publish = false`?

## Dependency chain summary

Publishing `abp-cli` or `abp-daemon` transitively requires **47 crates**
to be publishable. This is the full workspace minus:

- 2 already unpublished (`agent-backplane`, `xtask`)
- 4 orphaned (listed above)
- 2 experimental that are only pulled in by shims/bridges but not by the
  runtime chain (`abp-sidecar-proto`, `abp-sidecar-utils`)

**Before marking any internal crate `publish = false`:** verify it is not
in the transitive closure of `abp-cli`, `abp-daemon`, any shim, or any
bridge crate. Use `cargo publish --dry-run -p <crate>` on leaf crates to
confirm the publish chain is intact.
