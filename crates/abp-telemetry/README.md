# abp-telemetry

Structured telemetry and metrics collection for Agent Backplane runs.

Captures per-run metrics (duration, token counts, tool calls, errors), aggregates them into statistical summaries, and integrates with the `tracing` ecosystem for structured logging and span tracking.

## Key Types

| Type | Description |
|------|-------------|
| `RunMetrics` | Per-run metrics: backend, dialect, duration, tokens, tool calls, errors |
| `MetricsSummary` | Aggregate statistics: mean/p50/p99 duration, token totals, error rate, per-backend counts |
| `MetricsCollector` | Thread-safe collector that records `RunMetrics` and computes summaries |
| `MetricEvent` | Structured metric event with name, value, labels, and timestamp |
| `TelemetryPipeline` | Configurable pipeline for processing and routing metric events |
| `SpanTracker` | Tracks hierarchical spans for run lifecycle visibility |

## Modules

| Module | Description |
|--------|-------------|
| `metrics` | Core metric types and counter/gauge primitives |
| `events` | Telemetry event definitions |
| `spans` | Span types for run and tool-call tracing |
| `span_tracker` | Hierarchical span lifecycle tracking |
| `pipeline` | Metric event processing pipeline |
| `export` | Metric export adapters (JSON, tracing) |
| `hooks` | Lifecycle hooks for metric collection |
| `report` | Summary report generation |
| `tracing_integration` | Bridge between ABP telemetry and the `tracing` crate |
| `runtime_events` | Runtime-level event definitions for the orchestration layer |

## Usage

```rust
use abp_telemetry::{MetricsCollector, RunMetrics};

let collector = MetricsCollector::new();

collector.record(RunMetrics {
    backend_name: "sidecar:claude".into(),
    dialect: "claude".into(),
    duration_ms: 1200,
    tokens_in: 500,
    tokens_out: 150,
    ..Default::default()
});

let summary = collector.summary();
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
