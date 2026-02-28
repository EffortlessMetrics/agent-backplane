## Summary

This PR makes the sidecars **independently valuable** as "Rust ↔ vendor SDK/CLI bridges" (SDK-first with CLI fallbacks), and extracts that value into reusable Rust crates:

- `crates/sidecar-kit`: generic JSONL-over-stdio sidecar transport (value-based frames, lifecycle, cancellation).
- `crates/claude-bridge`: a standalone Claude bridge crate using sidecar-kit (raw/passthrough + mapped; optional typed normalization behind a feature).
- Sidecar host updates across Claude/Copilot/Gemini/Kimi/Codex to be **SDK-first**, contract-correct, and testable.
- Conformance tests updated to assert the **real** wire protocol (`hello` / `event` / `final.receipt`) and current receipt shape (`meta.*`, `outcome`, etc).

This is the "adoption wedge": teams can use a single provider bridge today, and adopt the full ABP translation/receipt story later.

---

## What changed

### 1) New reusable Rust crates

#### `crates/sidecar-kit`
A transport crate with **no dependency on `abp-core`**, designed for reuse outside this repo.

- Value-based `Frame` enum (`t`-tagged JSONL): `hello`, `run`, `event`, `final`, `fatal` (+ cancel/ping/pong scaffolding if present)
- `SidecarProcess`: spawn + stdio JSONL send/recv, stderr forwarding
- `SidecarClient`: strict `hello` handshake first, then run dispatch
- `RawRun`: event stream + final result channel + cancellation token

> Reviewer focus: cancellation/shutdown semantics and deterministic terminal completion.

#### `crates/claude-bridge`
A provider-specific "bridge crate" for Claude built on sidecar-kit.

- Config + discovery (node + host script resolution)
- `run_raw(...)`: passthrough vendor request → raw stream
- `run_mapped_raw(...)`: task/options → raw stream
- Optional `normalized` feature: map raw JSON → typed `AgentEvent` + `Receipt`

> Reviewer focus: handshake timeout enforcement and raw/passthrough invariants.

---

### 2) Sidecar SDK adapters (SDK-first, CLI fallback)

Each vendor host now includes a `package.json` for dependency management and (where applicable) Node tests with mock SDK modules.

- **Claude**: `hosts/claude/adapter.js` (SDK integration + tests)
  - mapped + passthrough
  - improved error stringification (`Error` no longer becomes `{}`)
- **Copilot**: `hosts/copilot/adapter.js`
  - transport selection: `sdk` → `acp` → `legacy` (env-driven)
  - added SDK-path test w/ mock module
- **Gemini**: `hosts/gemini/adapter.js` + `hosts/gemini/host.js`
  - SDK-first via `@google/genai`, CLI fallback
  - outcome normalization to ABI-safe lowercase (`complete|partial|failed`)
  - command discovery returns `null` (not `false`) when `PATH` is missing
- **Kimi**: `hosts/kimi/adapter.js` + tests
  - SDK-first via `@moonshot-ai/kimi-agent-sdk`, CLI fallback
  - outcome normalization
- **Codex**: `hosts/codex/host.js`
  - passthrough mode now honors `vendor.abp.request` for prompt input when present

---

### 3) CLI / daemon ergonomics and registration

- Adds/extends backend registration microcrates:
  - `crates/abp-claude-sdk`
  - `crates/abp-kimi-sdk`
- `abp-cli` enhanced to make backend usage less "work order JSON by hand":
  - backend aliases (e.g., `--backend gemini` → `sidecar:gemini`)
  - `--model`, repeated `--param key=value`, repeated `--env KEY=VALUE`
  - `--max-budget-usd`, `--max-turns` wired into runtime config

---

### 4) Conformance suite updated to match real protocol and receipts

The harness previously asserted legacy envelope tags and legacy receipt fields.

- Runner now executes only the files that exist (drops stale `receipt.test.js` / `error.test.js`)
- Tests assert:
  - `t: "event"` (not `agent_event`)
  - `t: "final"` + `final.receipt` (not `t: "receipt"`)
  - receipt structure: `receipt.meta.*`, `receipt.outcome`, timestamps, etc.

---

### 5) Workspace / dependency changes

- Workspace moved to **Rust Edition 2024** and dependency set refreshed.
  - **Note:** Edition 2024 implies an MSRV floor (Rust ≥ 1.85). If we want this explicit, we should add `rust-version = "1.85"` in `[workspace.package]` or a `rust-toolchain.toml`.

---

## Contract / protocol correctness (why this matters)

These are "contract lies" fixes:

- Codex passthrough is now **truthful** (declared passthrough actually runs passthrough input).
- Gemini outcomes are now **ABI-safe** (`complete|partial|failed` everywhere).
- Conformance now matches the actual wire contract (`event`, `final.receipt`), so it catches real regressions instead of failing on correct behavior.

---

## How to test

### Rust
```bash
cargo test
# or focused:
cargo test -p sidecar-kit -p claude-bridge -p abp-host -p abp-protocol
```

### Node conformance + adapter tests

```bash
node tests/conformance/runner.js

node --test hosts/claude/test/mapped.test.js
node --test hosts/copilot/test/sdk-adapter.test.js
node --test hosts/kimi/test/sdk-adapter.test.js
```

### Local smoke runs (after installing host deps)

```bash
npm --prefix hosts/claude install
npm --prefix hosts/gemini install
npm --prefix hosts/copilot install
npm --prefix hosts/kimi install
npm --prefix hosts/codex install

cargo run -p abp-cli -- backends
cargo run -p abp-cli -- run --backend gemini --task "hello" --model gemini-2.5-flash --param stream=true
cargo run -p abp-cli -- run --backend claude --task "hello"
```

---

## Reviewer notes / suggested review order

If you want a clean pass:

1. `crates/sidecar-kit/*` (transport + cancellation semantics)
2. `crates/claude-bridge/*` (passthrough invariants + discovery)
3. `hosts/*/adapter.js` + tests (SDK-first + fallback behaviors)
4. `tests/conformance/*` (protocol/receipt assumptions)
5. CLI/daemon wiring (`crates/abp-cli`, microcrates)

---

## Known follow-ups (tracked but not blocking this PR)

- Enforce `ClaudeBridgeConfig.handshake_timeout` with an actual `tokio::time::timeout(...)` around sidecar spawn/hello.
- In `sidecar-kit`, cancellation path should resolve `RawRun.result` with a deterministic cancellation error instead of dropping the oneshot.
- Consider making outcome normalization default "unknown → failed" (or at least emit a warning) instead of "unknown → complete".
