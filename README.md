# Agent Backplane (v0.1 skeleton)

Agent Backplane is a **translation layer** between *agent SDKs*.

You ship “drop‑in” SDK shims (one per vendor SDK) that map the vendor’s surface area onto a stable internal contract. The control plane then routes work orders to any backend (OpenAI, Anthropic, Gemini, local models, etc.) with a best‑effort projection matrix.

This repo is a **compilable, runnable scaffold**:

- A Rust microcrate workspace with a stable contract (`abp-core`)
- A JSONL sidecar protocol (`abp-protocol`)
- A sidecar host/supervisor (`abp-host`)
- Shared include/exclude glob matching utilities (`abp-glob`)
- Workspace staging + git harness utilities (`abp-workspace`)
- Policy utilities (`abp-policy`)
- Backend trait + `mock` + `sidecar` backends (`abp-integrations`)
- Orchestration runtime (`abp-runtime`)
- CLI (`abp`) and an HTTP daemon control plane (`abp-daemon`)
- Simple Node + Python sidecar examples under `hosts/`

The important point: **the contract is the product.** Everything else exists to faithfully map SDK semantics into that contract and back out again.

## Quick start

```bash
# build
cargo build

# generate JSON schemas for the public contract
cargo run -p xtask -- schema

# run the mock backend
cargo run -p abp-cli -- run --task "say hello" --backend mock

# run the node sidecar backend (requires node installed)
cargo run -p abp-cli -- run --task "hello from node" --backend sidecar:node

# run the python sidecar backend (requires python installed)
cargo run -p abp-cli -- run --task "hello from python" --backend sidecar:python

# run the codex sidecar backend (requires node installed)
cargo run -p abp-cli -- run --task "hello from codex sidecar" --backend sidecar:codex

# run the claude sidecar backend (requires node installed)
cargo run -p abp-cli -- run --task "hello from claude sidecar" --backend sidecar:claude

# run the copilot sidecar backend (requires node installed)
cargo run -p abp-cli -- run --task "hello from copilot sidecar" --backend sidecar:copilot

# run the kimi sidecar backend (requires node installed)
cargo run -p abp-cli -- run --task "hello from kimi sidecar" --backend sidecar:kimi

# run the gemini sidecar backend (requires node installed)
# npm --prefix hosts/gemini install
cargo run -p abp-cli -- run --task "hello from gemini sidecar" --backend sidecar:gemini

# alias + runtime vendor params
cargo run -p abp-cli -- run --task "summarize this codebase" --backend gemini \
  --model gemini-2.5-flash --param stream=true --param vertex=false

# run the daemon control plane
cargo run -p abp-daemon -- --bind 127.0.0.1:8088
```

Receipts land in `.agent-backplane/receipts/<run_id>.json`.

### Python Claude Client Mode

`hosts/python/host.py` now supports the same `vendor.abp.client_mode` feature flag used by the Claude sidecar adapter.  
If `claude_agent_sdk` is installed and `client_mode=true`, it will use a stateful SDK client path; otherwise it falls back to `query()` when available (or to deterministic partial fallback if the SDK is missing).

## Daemon API

- `GET /health`
- `GET /backends`
- `GET /capabilities` or `GET /capabilities?backend=<name>`
- `POST /run` with JSON body: `{ "backend": "mock", "work_order": {...} }`
- `GET /receipts`
- `GET /receipts/:run_id`

## Repository layout

- `crates/abp-core`: stable Rust types (WorkOrder, Receipt, events, capabilities)
- `crates/abp-protocol`: JSONL envelope + codec
- `crates/abp-host`: spawn a sidecar process and stream messages
- `crates/abp-glob`: compile and evaluate include/exclude glob rules
- `crates/abp-workspace`: staging + git harness utilities
- `crates/abp-policy`: policy compilation + allow/deny checks
- `crates/abp-integrations`: backend trait + implementations
- `crates/abp-runtime`: orchestration (workspace -> backend -> receipt)
- `crates/abp-claude-sdk`: Claude sidecar integration microcrate
- `crates/abp-codex-sdk`: Codex sidecar integration microcrate
- `crates/abp-gemini-sdk`: Gemini CLI sidecar integration microcrate
- `crates/abp-kimi-sdk`: Kimi sidecar integration microcrate
- `crates/abp-cli`: `abp` CLI
- `crates/abp-daemon`: HTTP control plane + receipt persistence
- `hosts/node`: example sidecar (JSONL over stdio)
- `hosts/python`: example sidecar (JSONL over stdio)
- `hosts/claude`: Claude-oriented sidecar with pluggable adapter module
- `hosts/codex`: Codex-oriented sidecar with passthrough/mapped modes
- `hosts/copilot`: GitHub Copilot sidecar scaffold with Copilot adapter contract
- `hosts/kimi`: Kimi sidecar scaffold with runnable adapter contract
- `hosts/gemini`: Gemini CLI sidecar scaffold with runnable adapter contract
- `contracts/schemas`: generated JSON schemas

## What’s intentionally missing

This is a *skeleton*, not the finished backplane:

- No real vendor SDK adapters yet (OpenAI / Anthropic / etc.)
- No projection matrix implementation
- HTTP daemon and durable receipt persistence are now available
- No projection matrix implementation
- No policy enforcement inside sidecars (yet)
- No sandboxing/containers

Those are the next layers; the structure here is meant to make them incremental.

## License

Dual-licensed MIT OR Apache-2.0.
