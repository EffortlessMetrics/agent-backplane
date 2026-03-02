// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stream pipeline integration for the ABP runtime.
//!
//! Re-exports [`abp_stream`] types and provides helpers for wiring a
//! [`StreamPipeline`](abp_stream::StreamPipeline) into the runtime's two-stage event channel.

pub use abp_stream::{
    EventFilter, EventMultiplexer, EventRecorder, EventStats, EventStream, EventTransform,
    StreamPipeline, StreamPipelineBuilder, event_kind_name,
};

use abp_core::AgentEvent;
use tokio::sync::mpsc;

/// Process a single event through an optional pipeline before forwarding.
///
/// Returns `Some(event)` (possibly transformed) if the event passes all
/// filters, or `None` if it was filtered out.
pub fn apply_pipeline(pipeline: Option<&StreamPipeline>, event: AgentEvent) -> Option<AgentEvent> {
    match pipeline {
        Some(p) => p.process(event),
        None => Some(event),
    }
}

/// Drain events from `from_rx`, run each through the pipeline, and forward
/// survivors to `to_tx`. Returns the collected trace of events that were
/// forwarded.
pub async fn forward_events(
    from_rx: &mut mpsc::Receiver<AgentEvent>,
    to_tx: &mpsc::Sender<AgentEvent>,
    pipeline: Option<&StreamPipeline>,
) -> Vec<AgentEvent> {
    let mut trace = Vec::new();
    while let Some(ev) = from_rx.recv().await {
        if let Some(ev) = apply_pipeline(pipeline, ev) {
            trace.push(ev.clone());
            if to_tx.send(ev).await.is_err() {
                break;
            }
        }
    }
    trace
}
