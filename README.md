# Agent Backplane (v0.1 skeleton)

Agent Backplane is a **translation layer** between *agent SDKs*.

You ship “drop‑in” SDK shims (one per vendor SDK) that map the vendor’s surface area onto a stable internal contract. The control plane then routes work orders to any backend (OpenAI, Anthropic, Gemini, local models, etc.) with a best‑effort projection matrix.

This repo is a **compilable, runnable scaffold**:

- A Rust microcrate workspace with a stable contract (`abp-core`)
- A JSONL sidecar protocol (`abp-protocol`)
- A sidecar host/supervisor (`abp-host`)
- Workspace staging + git harness utilities (`abp-workspace`)
- Policy utilities (`abp-policy`)
- Backend trait + `mock` + `sidecar` backends (`abp-integrations`)
- Orchestration runtime (`abp-runtime`)
- CLI (`abp`) and a stub daemon (`abp-daemon`)
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

# run the claude sidecar backend (requires node installed)
cargo run -p abp-cli -- run --task "hello from claude sidecar" --backend sidecar:claude
```

Receipts land in `.agent-backplane/receipts/<run_id>.json`.

## Repository layout

- `crates/abp-core`: stable Rust types (WorkOrder, Receipt, events, capabilities)
- `crates/abp-protocol`: JSONL envelope + codec
- `crates/abp-host`: spawn a sidecar process and stream messages
- `crates/abp-workspace`: staging + git harness utilities
- `crates/abp-policy`: policy compilation + allow/deny checks
- `crates/abp-integrations`: backend trait + implementations
- `crates/abp-runtime`: orchestration (workspace -> backend -> receipt)
- `crates/abp-cli`: `abp` CLI
- `crates/abp-daemon`: placeholder daemon
- `hosts/node`: example sidecar (JSONL over stdio)
- `hosts/python`: example sidecar (JSONL over stdio)
- `hosts/claude`: Claude-oriented sidecar with pluggable adapter module
- `contracts/schemas`: generated JSON schemas

## What’s intentionally missing

This is a *skeleton*, not the finished backplane:

- No real vendor SDK adapters yet (OpenAI / Anthropic / etc.)
- No projection matrix implementation
- No durable receipt store
- No policy enforcement inside sidecars (yet)
- No sandboxing/containers

Those are the next layers; the structure here is meant to make them incremental.

## License

Dual-licensed MIT OR Apache-2.0.
