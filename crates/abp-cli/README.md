# abp-cli

Command-line interface for Agent Backplane -- run work orders against any backend, list backends, validate artifacts, inspect receipts, and translate between SDK dialects.

The `abp` binary provides 10 subcommands covering the full lifecycle of agent work order execution, configuration management, and receipt verification.

## Subcommands

| Command | Description |
|---------|-------------|
| `run` | Execute a work order against a backend and stream agent events |
| `backends` | List registered and well-known sidecar backends |
| `validate` | Validate a JSON file as a WorkOrder, Receipt, or TOML config |
| `schema` | Print a JSON schema for WorkOrder, Receipt, or Config |
| `inspect` | Inspect a receipt file and verify its SHA-256 hash |
| `translate` | Translate a JSON request between SDK dialect formats |
| `health` | Check health of all configured backends |
| `config` | Configuration management (check, show, validate, diff) |
| `receipt` | Receipt inspection (verify hash, diff two receipts) |
| `status` | Show current runtime and daemon status |

## Key Flags for `run`

| Flag | Description |
|------|-------------|
| `--backend <name>` | Backend to use (mock, node, claude, copilot, kimi, gemini, codex) |
| `--task <text>` | Task description for the work order |
| `--model <name>` | Override model selection |
| `--workspace-mode <mode>` | PassThrough or Staged (default: Staged) |
| `--lane <lane>` | PatchFirst or WorkspaceFirst (default: PatchFirst) |
| `--max-budget-usd <N>` | Cap spend per run in USD |
| `--max-turns <N>` | Limit agent turn count |
| `--param key=value` | Vendor-specific parameters (repeatable) |
| `--env KEY=VALUE` | Environment variables for sidecar (repeatable) |
| `--timeout <secs>` | Timeout in seconds for the entire run |
| `--retry <N>` | Number of times to retry on failure |
| `--fallback <backend>` | Fallback backend if the primary fails |
| `--policy <path>` | Path to a policy profile JSON file |
| `--output <path>` | Write the receipt to a specific file path |
| `--events <path>` | Write streamed events as JSONL to a file |
| `--json` | Print JSON instead of pretty output |

## Usage

```bash
# Run a task with the mock backend
abp run --task "hello world" --backend mock

# Run with Claude and budget cap
abp run --task "fix the bug" --backend claude --max-budget-usd 1.0

# List all available backends
abp backends

# Validate a work order file
abp validate work_order.json

# Inspect a receipt and verify its hash
abp inspect .agent-backplane/receipts/abc123.json

# Translate between dialects
abp translate --from openai --to claude request.json

# Show effective config as JSON
abp config show --format json

# Diff two receipts
abp receipt diff receipt1.json receipt2.json
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
