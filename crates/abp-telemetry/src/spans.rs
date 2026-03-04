// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Structured tracing span helpers and custom span types for ABP operations.
//!
//! The first part of this module provides thin helpers that create pre-populated
//! [`tracing::Span`] values.  The second part provides standalone span types
//! ([`SpanContext`], [`TelemetrySpan`], [`SpanBuilder`], [`SpanRecorder`]) that
//! are independent of the `tracing` crate and can be exported or inspected
//! directly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{Span, info_span};

/// Create a tracing span for processing a work-order request.
///
/// The returned span carries `work_order_id`, `task`, and `lane` fields
/// extracted from the provided parameters.
///
/// # Example
///
/// ```
/// let span = abp_telemetry::spans::request_span("wo-1", "refactor auth", "mapped");
/// let _guard = span.enter();
/// ```
pub fn request_span(work_order_id: &str, task: &str, lane: &str) -> Span {
    info_span!(
        "abp.request",
        work_order_id = %work_order_id,
        task = %task,
        lane = %lane,
    )
}

/// Create a tracing span for processing a single agent event.
///
/// The returned span carries `event_kind` and `sequence` fields.
///
/// # Example
///
/// ```
/// let span = abp_telemetry::spans::event_span("tool_call", 3);
/// let _guard = span.enter();
/// ```
pub fn event_span(event_kind: &str, sequence: u64) -> Span {
    info_span!(
        "abp.event",
        event_kind = %event_kind,
        sequence = sequence,
    )
}

/// Create a tracing span for a backend call.
///
/// The returned span carries the `backend` name.
///
/// # Example
///
/// ```
/// let span = abp_telemetry::spans::backend_span("sidecar:node");
/// let _guard = span.enter();
/// ```
pub fn backend_span(backend_name: &str) -> Span {
    info_span!(
        "abp.backend",
        backend = %backend_name,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_span_does_not_panic() {
        let span = request_span("wo-123", "do stuff", "mapped");
        let _guard = span.enter();
    }

    #[test]
    fn event_span_does_not_panic() {
        let span = event_span("tool_call", 7);
        let _guard = span.enter();
    }

    #[test]
    fn backend_span_does_not_panic() {
        let span = backend_span("mock");
        let _guard = span.enter();
    }
}
