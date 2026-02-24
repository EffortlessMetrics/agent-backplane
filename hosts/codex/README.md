# Codex Sidecar for Agent Backplane

This directory contains the OpenAI Codex SDK sidecar implementation for ABP.

The sidecar speaks ABP JSONL envelopes over stdio:

- `hello`
- `run`
- `event*`
- `final`

It uses `@openai/codex-sdk` and maps Codex events into ABP `AgentEvent` records.

## Prerequisites

- Node.js 18+
- OpenAI Codex SDK dependency installed
- Authentication configured (`CODEX_API_KEY` or `OPENAI_API_KEY`, or local Codex auth)

Install dependencies:

```bash
npm --prefix hosts/codex install
```

## Quick Start

From repo root:

```bash
# Run through ABP CLI
CODEX_API_KEY=sk-... cargo run -p abp-cli -- run --backend sidecar:codex --task "Summarize this repository"
```

or run the sidecar directly:

```bash
node hosts/codex/host.js
```

## WorkOrder Config (vendor.codex)

`work_order.config.vendor.codex` supports:

- `apiKey` / `api_key`
- `baseUrl` / `base_url`
- `model`
- `sandboxMode` / `sandbox_mode`
- `outputSchema` / `output_schema`
- `threadId` / `thread_id`
- `resume`
- `timeoutMs` / `timeout_ms`
- `retryCount` / `retry_count` / `retries`
- `webSearchMode` / `web_search_mode`
- `webSearchEnabled` / `web_search_enabled`
- `approvalPolicy` / `approval_policy`
- `additionalDirectories` / `additional_directories`
- `codexPathOverride` / `codex_path_override`
- `config` (raw Codex config object)
- `env` (extra process environment passed to Codex)

ABP execution mode is still read from `work_order.config.vendor.abp.mode` (`mapped` default, `passthrough` supported in receipt metadata).

## Behavior Notes

- Sidecar sends `hello` first, per protocol.
- Runtime sends one `run` per sidecar process.
- Host streams mapped events during `thread.runStreamed(...)`.
- Receipt hash is finalized by ABP runtime (`with_hash()`), so sidecar leaves `receipt_sha256: null`.
- Transient failures (rate limits, timeouts, temporary network issues) are retried with bounded backoff.

## Protocol Reference

- [Sidecar protocol](../../docs/sidecar_protocol.md)
