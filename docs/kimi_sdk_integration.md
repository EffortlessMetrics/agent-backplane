# Kimi SDK Integration Guide for Agent Backplane

This document captures the implemented `sidecar:kimi` adapter path.

## Runtime topology

```text
WorkOrder -> abp-runtime -> host runtime -> hosts/kimi/host.js
   -> adapter (hosts/kimi/adapter.js)
      -> SDK transport (@moonshot-ai/kimi-agent-sdk)
      -> CLI transport (ABP_KIMI_RUNNER / ABP_KIMI_CMD)
   -> external Kimi process/SDK
```

## Activation

```bash
npm --prefix hosts/kimi install
cargo run -p abp-cli -- run --backend sidecar:kimi --task "audit this repository"
```

## Configuration

- `work_order.config.vendor.kimi`:
  - `model`
  - `temperature`
  - `topP`
  - `reasoningEffort`
  - `thinkingMode`
  - `agentMode`
  - `agentSwarm`
  - `yolo`

- `work_order.config.vendor.abp.mode`:
  - `mapped` (default)
  - `passthrough`

## Environment variables

- `ABP_KIMI_ADAPTER_MODULE`
- `ABP_KIMI_TRANSPORT` (`auto`, `sdk`, `cli`)
- `ABP_KIMI_SDK_MODULE`
- `ABP_KIMI_MODEL`
- `ABP_KIMI_RETRY_ATTEMPTS`
- `ABP_KIMI_RETRY_BASE_DELAY_MS`
- `ABP_KIMI_RUNNER`
- `ABP_KIMI_CMD`
- `ABP_KIMI_ARGS`
- `ABP_KIMI_RUNNER_ARGS`
- `ABP_KIMI_MAX_INLINE_OUTPUT_BYTES`
- `KIMI_API_KEY` or `KIMI_API_CODE`

Authentication options:

- Prefer OAuth flow once by running `kimi /login` in your shell.
- For key-based mode, set either `KIMI_API_KEY` or `KIMI_API_CODE` in the environment before running the sidecar.

## Receipt behavior

The host emits contract-shaped events and final receipt fields:

- `meta` with run/task identifiers
- `backend.id = "kimi_agent_sdk"`
- `capabilities` from `hosts/kimi/capabilities.js`
- `usage` normalized where possible
- `outcome` normalized to ABP enum values (`complete`, `partial`, `failed`)
- deterministic `receipt_sha256` with `receipt_sha256` nulled before hashing
