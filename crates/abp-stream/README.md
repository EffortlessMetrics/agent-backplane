# abp-stream

Agent event stream processing, transformation, and multiplexing for Agent Backplane.

## Features

- **EventStream** — wrapper around `mpsc::Receiver<AgentEvent>` implementing `futures_core::Stream`
- **EventCollector** — collects events into a `Vec` while forwarding them through a stream
- **EventFilter** — filter events by kind, source, or custom predicate
- **EventTransform** — transform events in-flight (add metadata, modify content)
- **MergedStream** — merges multiple event streams with round-robin interleaving
- **TimeoutStream** — wraps a stream with per-item timeout
- **BufferedStream** — buffers events and emits them in batches
- **EventMultiplexer** — combine multiple event streams into one, maintaining timestamp ordering
- **EventRecorder** — record all events for replay/inspection
- **EventStats** — track event statistics (count by kind, total tokens, timing)
- **StreamMetrics** — tracks event counts, throughput, latency
- **StreamPipeline** — compose filters, transforms, and recording into a processing pipeline

## Usage

```rust,no_run
use abp_core::AgentEvent;
use abp_stream::{StreamPipelineBuilder, EventFilter, EventTransform, EventStats};

let (tx, rx) = tokio::sync::mpsc::channel::<AgentEvent>(256);
let stats = EventStats::new();

let pipeline = StreamPipelineBuilder::new()
    .filter(EventFilter::by_kind("assistant_delta"))
    .transform(EventTransform::new(|ev| { ev }))
    .with_stats(stats.clone())
    .record()
    .build();
```

## License

MIT OR Apache-2.0
