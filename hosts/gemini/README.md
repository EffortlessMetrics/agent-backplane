# Gemini Sidecar for Agent Backplane

This host implements the ABP sidecar protocol for Gemini with two transport modes:

- `sdk` mode via `@google/genai` (default in `auto`)
- `cli` mode via an external Gemini command/runner

## Install

```bash
npm --prefix hosts/gemini install
```

## Quick Start

```bash
# Preferred: SDK mode with API key
set GEMINI_API_KEY=YOUR_KEY
cargo run -p abp-cli -- run --backend gemini --task "Summarize this repo"

# With model + vendor params
cargo run -p abp-cli -- run --backend gemini --task "Explain build steps" ^
  --model gemini-2.5-flash ^
  --param stream=true ^
  --param vertex=false
```

## Runtime Flags

`abp run` supports:

- `--backend gemini` (alias for `sidecar:gemini`)
- `--model <model>`
- repeated `--param key=value` (mapped into `work_order.config.vendor`)
- repeated `--env KEY=VALUE` (mapped into `work_order.config.env`)

For Gemini, un-namespaced params (for example `--param stream=true`) are written under `vendor.gemini.*`.

## Environment Variables

- `ABP_GEMINI_TRANSPORT`: `auto` (default), `sdk`, `cli`
- `ABP_GEMINI_MODEL`: default model when none is provided
- `ABP_GEMINI_RETRY_ATTEMPTS`: SDK retry attempts (default `3`)
- `ABP_GEMINI_RETRY_BASE_DELAY_MS`: SDK retry base backoff (default `1000`)
- `ABP_GEMINI_CMD` / `ABP_GEMINI_ARGS`: CLI command path + args
- `ABP_GEMINI_RUNNER` / `ABP_GEMINI_RUNNER_ARGS`: custom runner (JSON stdin contract)
- `ABP_GEMINI_CLI_INPUT`: `prompt-arg` (default) or `json-stdin`
- `ABP_GEMINI_ADAPTER_MODULE`: optional custom adapter module path
- `ABP_GEMINI_MAX_INLINE_OUTPUT_BYTES`: max inline tool output before artifact spill

Auth-related env vars are passed through as usual (`GEMINI_API_KEY`, `GOOGLE_API_KEY`, `GOOGLE_GENAI_USE_VERTEXAI`, `GOOGLE_CLOUD_PROJECT`, `GOOGLE_CLOUD_LOCATION`).
