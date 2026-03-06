# abp-daemon

HTTP control-plane daemon for Agent Backplane -- REST API with WebSocket and SSE support for work order execution, backend management, and receipt retrieval.

The daemon exposes a full REST API for programmatic access to ABP's runtime. All endpoints are also available under the `/api/v1` prefix for versioned access.

## Endpoints

| Method | Route | Description |
|--------|-------|-------------|
| GET | `/health` | Health check with contract version and timestamp |
| GET | `/status` | Runtime status: backends, active runs, total runs |
| GET | `/metrics` | Aggregate run metrics (total, running, completed, failed) |
| GET | `/backends` | List registered backend names |
| GET | `/capabilities` | Backend capability manifests (optional `?backend=` filter) |
| GET | `/config` | Current daemon configuration |
| POST | `/validate` | Validate a work order + backend combination |
| GET | `/schema/{type}` | JSON schema for work_order, receipt, capability_requirements, or backplane_config |
| POST | `/run` | Execute a work order (also available at POST `/runs`) |
| GET | `/runs` | List all tracked run IDs |
| GET | `/runs/{run_id}` | Get run status and receipt |
| DELETE | `/runs/{run_id}` | Delete a completed/failed run |
| GET | `/runs/{run_id}/receipt` | Get the receipt for a specific run |
| POST | `/runs/{run_id}/cancel` | Cancel a pending or running run |
| GET | `/runs/{run_id}/events` | SSE stream of agent events for a run |
| GET | `/receipts` | List receipt IDs (optional `?limit=N`) |
| GET | `/receipts/{run_id}` | Get a receipt by run ID |
| GET | `/ws` | WebSocket endpoint for real-time communication |

## Key Types

| Type | Description |
|------|-------------|
| `AppState` | Shared application state: runtime, receipts cache, run tracker |
| `DaemonConfig` | Static configuration (bind address, port, auth token) |
| `DaemonState` | Shared mutable state for backends and active runs |
| `RunTracker` | In-memory run lifecycle tracker (pending, running, completed, failed, cancelled) |
| `RunRequest` | Request body for the `/run` endpoint |
| `RunResponse` | Response body with run ID, events, and receipt |
| `DaemonError` | Error type with automatic HTTP status code mapping |

## Usage

```bash
# Start the daemon on the default address (127.0.0.1:8088)
abp-daemon

# Start with a custom bind address and receipts directory
abp-daemon --bind 0.0.0.0:9090 --receipts-dir ./receipts

# Start with debug logging and a config file
abp-daemon --debug --config backplane.toml
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
