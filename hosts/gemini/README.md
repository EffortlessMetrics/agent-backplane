# Gemini CLI Sidecar for Agent Backplane

This directory contains a Gemini CLI sidecar implementation that speaks the ABP JSONL protocol.

## Quick Start

```bash
node hosts/gemini/host.js
```

## Environment Variables

- `ABP_GEMINI_CMD` (default: `gemini`)  
  Command executed by the default adapter.
- `ABP_GEMINI_ARGS`  
  Command arguments (JSON array or shell-style tokens) for `ABP_GEMINI_CMD`.
- `ABP_GEMINI_RUNNER`  
  Full command line override used in place of `ABP_GEMINI_CMD`.
- `ABP_GEMINI_RUNNER_ARGS`  
  Arguments for `ABP_GEMINI_RUNNER` (JSON array or shell-style tokens).
- `ABP_GEMINI_ADAPTER_MODULE`  
  Optional custom adapter module path.
- `ABP_GEMINI_MAX_INLINE_OUTPUT_BYTES`  
  Threshold used before tool output is written to artifacts.

## Protocol

This host conforms to:
- `hello` handshake as the first line
- `run` request handling
- `event` and `final` stream messages
- `fatal` on protocol/runtime errors

## Backend Identity

- Backend ID: `gemini`
- Contract Version: `abp/v0.1`
