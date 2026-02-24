# Kimi Sidecar for Agent Backplane

This directory contains a Kimi-oriented sidecar scaffold for Agent Backplane.

The Kimi host follows the ABP JSONL protocol and emits normalized ABP events
(`hello`, `event`, `final`) while delegating Kimi transport concerns to an
adapter module.

## Usage

```bash
# Start the sidecar directly
node hosts/kimi/host.js

# Use in ABP CLI (from repo root)
cargo run -p abp-cli -- run --backend sidecar:kimi --task "analyze this repo"

# Debug with a custom adapter module
ABP_KIMI_ADAPTER_MODULE=./hosts/kimi/adapter.js \
  node hosts/kimi/host.js
```

## Files

| File | Purpose |
|------|---------|
| `host.js` | Sidecar protocol handling, policy checks, receipt assembly |
| `adapter.js` | Default adapter scaffold for Kimi runner/command integration |
| `capabilities.js` | Capability manifest and support levels |

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ABP_KIMI_ADAPTER_MODULE` | Custom adapter module path | `./adapter.js` |
| `ABP_KIMI_MAX_INLINE_OUTPUT_BYTES` | Max inline artifact size | `8192` |
| `KIMI_API_KEY` or `KIMI_API_CODE` | API key for non-interactive auth (if supported by installed runner/sdk) | unset |
| `ABP_KIMI_RUNNER` | Path to a command that accepts request JSON on stdin | (unset) |
| `ABP_KIMI_CMD` | Fallback command name | `kimi` |
| `ABP_KIMI_ARGS` | JSON array of args for `ABP_KIMI_CMD` | `[]` |
| `ABP_KIMI_RUNNER_ARGS` | JSON array of args for `ABP_KIMI_RUNNER` | `[]` |

## Protocol Notes

- `hello` is emitted as first output line.
- `run` envelopes are expected to include full `workOrder`.
- `receipt_sha256` is computed with `receipt_sha256` set to null before hashing.

## Minimal Security Posture

- Tool allowlist/denylist and path checks use `work_order.policy`.
- Path checks are relative to `work_order.workspace.root`.
- Tools marked in `require_approval_for` are denied until permission callbacks are added.

