// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code)]
//! [`MetricEvent`] — canonical enum of all metric events emitted during agent
//! run processing.
//!
//! This provides a single discriminated union covering request lifecycle,
//! token usage, errors, and span tracking events.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// MetricEvent
// ---------------------------------------------------------------------------

/// Canonical metric event enum covering the full ABP request lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MetricEvent {
    /// A request has been initiated.
    RequestStarted {
        /// Unique run identifier.
        run_id: String,
        /// Backend handling the request.
        backend: String,
        /// ISO-8601 timestamp.
        timestamp: String,
    },
    /// A request completed (successfully or with error).
    RequestCompleted {
        /// Unique run identifier.
        run_id: String,
        /// Wall-clock duration in milliseconds.
        duration_ms: u64,
        /// Whether the request succeeded.
        success: bool,
        /// ISO-8601 timestamp.
        timestamp: String,
    },
    /// Token usage was recorded for a request.
    TokensUsed {
        /// Unique run identifier.
        run_id: String,
        /// Number of input tokens consumed.
        input_tokens: u64,
        /// Number of output tokens produced.
        output_tokens: u64,
    },
    /// An error occurred during request processing.
    Error {
        /// Unique run identifier.
        run_id: String,
        /// Machine-readable error code.
        code: String,
        /// Human-readable error message.
        message: String,
        /// Error classification (transient, permanent, unknown).
        classification: String,
    },
    /// A backend was selected for a request.
    BackendSelected {
        /// Unique run identifier.
        run_id: String,
        /// Name of the selected backend.
        backend: String,
    },
    /// A policy was evaluated.
    PolicyEvaluated {
        /// Unique run identifier.
        run_id: String,
        /// Name of the policy profile.
        policy: String,
        /// Whether the policy allowed the operation.
        allowed: bool,
    },
    /// A tracking span was started.
    SpanStarted {
        /// Unique span identifier.
        span_id: String,
        /// Human-readable span name.
        name: String,
        /// Optional parent span identifier.
        parent_span_id: Option<String>,
    },
    /// A tracking span has ended.
    SpanEnded {
        /// Unique span identifier.
        span_id: String,
        /// Duration in milliseconds.
        duration_ms: u64,
    },
}

impl MetricEvent {
    /// Return the run_id associated with this event, if any.
    pub fn run_id(&self) -> Option<&str> {
        match self {
            Self::RequestStarted { run_id, .. }
            | Self::RequestCompleted { run_id, .. }
            | Self::TokensUsed { run_id, .. }
            | Self::Error { run_id, .. }
            | Self::BackendSelected { run_id, .. }
            | Self::PolicyEvaluated { run_id, .. } => Some(run_id),
            Self::SpanStarted { .. } | Self::SpanEnded { .. } => None,
        }
    }

    /// Return a short label for the event variant.
    pub fn label(&self) -> &'static str {
        match self {
            Self::RequestStarted { .. } => "request_started",
            Self::RequestCompleted { .. } => "request_completed",
            Self::TokensUsed { .. } => "tokens_used",
            Self::Error { .. } => "error",
            Self::BackendSelected { .. } => "backend_selected",
            Self::PolicyEvaluated { .. } => "policy_evaluated",
            Self::SpanStarted { .. } => "span_started",
            Self::SpanEnded { .. } => "span_ended",
        }
    }

    /// Whether this event represents an error.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Whether this event relates to token usage.
    pub fn is_token_event(&self) -> bool {
        matches!(self, Self::TokensUsed { .. })
    }

    /// Whether this event is a request lifecycle event (started or completed).
    pub fn is_request_lifecycle(&self) -> bool {
        matches!(
            self,
            Self::RequestStarted { .. } | Self::RequestCompleted { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// MetricEventCollector
// ---------------------------------------------------------------------------

/// Thread-safe collector for [`MetricEvent`]s.
#[derive(Debug, Clone, Default)]
pub struct MetricEventCollector {
    events: Arc<Mutex<Vec<MetricEvent>>>,
}

impl MetricEventCollector {
    /// Create a new, empty collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a metric event.
    pub fn record(&self, event: MetricEvent) {
        let mut guard = self.events.lock().expect("metric event lock poisoned");
        guard.push(event);
    }

    /// Return all collected events.
    pub fn events(&self) -> Vec<MetricEvent> {
        let guard = self.events.lock().expect("metric event lock poisoned");
        guard.clone()
    }

    /// Number of collected events.
    pub fn len(&self) -> usize {
        let guard = self.events.lock().expect("metric event lock poisoned");
        guard.len()
    }

    /// Whether the collector is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return events matching a specific label.
    pub fn events_with_label(&self, label: &str) -> Vec<MetricEvent> {
        let guard = self.events.lock().expect("metric event lock poisoned");
        guard
            .iter()
            .filter(|e| e.label() == label)
            .cloned()
            .collect()
    }

    /// Return events for a specific run.
    pub fn events_for_run(&self, run_id: &str) -> Vec<MetricEvent> {
        let guard = self.events.lock().expect("metric event lock poisoned");
        guard
            .iter()
            .filter(|e| e.run_id() == Some(run_id))
            .cloned()
            .collect()
    }

    /// Count events grouped by label.
    pub fn event_counts(&self) -> BTreeMap<String, usize> {
        let guard = self.events.lock().expect("metric event lock poisoned");
        let mut counts = BTreeMap::new();
        for event in guard.iter() {
            *counts.entry(event.label().to_string()).or_insert(0) += 1;
        }
        counts
    }

    /// Total token usage across all `TokensUsed` events.
    pub fn total_tokens(&self) -> (u64, u64) {
        let guard = self.events.lock().expect("metric event lock poisoned");
        let mut input = 0u64;
        let mut output = 0u64;
        for event in guard.iter() {
            if let MetricEvent::TokensUsed {
                input_tokens,
                output_tokens,
                ..
            } = event
            {
                input += input_tokens;
                output += output_tokens;
            }
        }
        (input, output)
    }

    /// Error count.
    pub fn error_count(&self) -> usize {
        let guard = self.events.lock().expect("metric event lock poisoned");
        guard.iter().filter(|e| e.is_error()).count()
    }

    /// Error rate: errors / total request completions.
    pub fn error_rate(&self) -> f64 {
        let guard = self.events.lock().expect("metric event lock poisoned");
        let completions = guard
            .iter()
            .filter(|e| matches!(e, MetricEvent::RequestCompleted { .. }))
            .count();
        let errors = guard.iter().filter(|e| e.is_error()).count();
        if completions == 0 {
            return 0.0;
        }
        errors as f64 / completions as f64
    }

    /// Clear all collected events.
    pub fn clear(&self) {
        let mut guard = self.events.lock().expect("metric event lock poisoned");
        guard.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_event_request_started_label() {
        let ev = MetricEvent::RequestStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        };
        assert_eq!(ev.label(), "request_started");
        assert_eq!(ev.run_id(), Some("r1"));
        assert!(ev.is_request_lifecycle());
        assert!(!ev.is_error());
    }

    #[test]
    fn metric_event_request_completed_label() {
        let ev = MetricEvent::RequestCompleted {
            run_id: "r1".into(),
            duration_ms: 100,
            success: true,
            timestamp: "t".into(),
        };
        assert_eq!(ev.label(), "request_completed");
        assert!(ev.is_request_lifecycle());
    }

    #[test]
    fn metric_event_tokens_used_label() {
        let ev = MetricEvent::TokensUsed {
            run_id: "r1".into(),
            input_tokens: 50,
            output_tokens: 100,
        };
        assert_eq!(ev.label(), "tokens_used");
        assert!(ev.is_token_event());
        assert!(!ev.is_error());
    }

    #[test]
    fn metric_event_error_label() {
        let ev = MetricEvent::Error {
            run_id: "r1".into(),
            code: "E001".into(),
            message: "fail".into(),
            classification: "transient".into(),
        };
        assert_eq!(ev.label(), "error");
        assert!(ev.is_error());
        assert!(!ev.is_token_event());
    }

    #[test]
    fn metric_event_backend_selected_label() {
        let ev = MetricEvent::BackendSelected {
            run_id: "r1".into(),
            backend: "sidecar:node".into(),
        };
        assert_eq!(ev.label(), "backend_selected");
    }

    #[test]
    fn metric_event_policy_evaluated_label() {
        let ev = MetricEvent::PolicyEvaluated {
            run_id: "r1".into(),
            policy: "strict".into(),
            allowed: true,
        };
        assert_eq!(ev.label(), "policy_evaluated");
    }

    #[test]
    fn metric_event_span_started_has_no_run_id() {
        let ev = MetricEvent::SpanStarted {
            span_id: "s1".into(),
            name: "op".into(),
            parent_span_id: None,
        };
        assert_eq!(ev.label(), "span_started");
        assert_eq!(ev.run_id(), None);
    }

    #[test]
    fn metric_event_span_ended_label() {
        let ev = MetricEvent::SpanEnded {
            span_id: "s1".into(),
            duration_ms: 50,
        };
        assert_eq!(ev.label(), "span_ended");
        assert_eq!(ev.run_id(), None);
    }

    #[test]
    fn metric_event_all_labels() {
        let events: Vec<(MetricEvent, &str)> = vec![
            (
                MetricEvent::RequestStarted {
                    run_id: "r".into(),
                    backend: "b".into(),
                    timestamp: "t".into(),
                },
                "request_started",
            ),
            (
                MetricEvent::RequestCompleted {
                    run_id: "r".into(),
                    duration_ms: 0,
                    success: true,
                    timestamp: "t".into(),
                },
                "request_completed",
            ),
            (
                MetricEvent::TokensUsed {
                    run_id: "r".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                },
                "tokens_used",
            ),
            (
                MetricEvent::Error {
                    run_id: "r".into(),
                    code: "E".into(),
                    message: "m".into(),
                    classification: "unknown".into(),
                },
                "error",
            ),
            (
                MetricEvent::BackendSelected {
                    run_id: "r".into(),
                    backend: "b".into(),
                },
                "backend_selected",
            ),
            (
                MetricEvent::PolicyEvaluated {
                    run_id: "r".into(),
                    policy: "p".into(),
                    allowed: true,
                },
                "policy_evaluated",
            ),
            (
                MetricEvent::SpanStarted {
                    span_id: "s".into(),
                    name: "n".into(),
                    parent_span_id: None,
                },
                "span_started",
            ),
            (
                MetricEvent::SpanEnded {
                    span_id: "s".into(),
                    duration_ms: 0,
                },
                "span_ended",
            ),
        ];
        for (ev, expected) in events {
            assert_eq!(ev.label(), expected);
        }
    }

    #[test]
    fn metric_event_serde_roundtrip_request_started() {
        let ev = MetricEvent::RequestStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: MetricEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn metric_event_serde_roundtrip_tokens_used() {
        let ev = MetricEvent::TokensUsed {
            run_id: "r1".into(),
            input_tokens: 500,
            output_tokens: 1000,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: MetricEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn metric_event_serde_roundtrip_error() {
        let ev = MetricEvent::Error {
            run_id: "r1".into(),
            code: "E001".into(),
            message: "timeout".into(),
            classification: "transient".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: MetricEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn metric_event_json_tag_format() {
        let ev = MetricEvent::TokensUsed {
            run_id: "r1".into(),
            input_tokens: 10,
            output_tokens: 20,
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"tokens_used\""));
    }

    #[test]
    fn collector_new_is_empty() {
        let c = MetricEventCollector::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn collector_record_and_len() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::RequestStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.len(), 1);
        assert!(!c.is_empty());
    }

    #[test]
    fn collector_events_with_label() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::RequestStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.record(MetricEvent::TokensUsed {
            run_id: "r1".into(),
            input_tokens: 10,
            output_tokens: 20,
        });
        c.record(MetricEvent::RequestCompleted {
            run_id: "r1".into(),
            duration_ms: 50,
            success: true,
            timestamp: "t".into(),
        });
        assert_eq!(c.events_with_label("request_started").len(), 1);
        assert_eq!(c.events_with_label("tokens_used").len(), 1);
        assert_eq!(c.events_with_label("request_completed").len(), 1);
    }

    #[test]
    fn collector_events_for_run() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::RequestStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.record(MetricEvent::RequestStarted {
            run_id: "r2".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.events_for_run("r1").len(), 1);
        assert_eq!(c.events_for_run("r2").len(), 1);
        assert_eq!(c.events_for_run("r3").len(), 0);
    }

    #[test]
    fn collector_event_counts() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::RequestStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.record(MetricEvent::RequestStarted {
            run_id: "r2".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.record(MetricEvent::Error {
            run_id: "r1".into(),
            code: "E001".into(),
            message: "fail".into(),
            classification: "transient".into(),
        });
        let counts = c.event_counts();
        assert_eq!(counts["request_started"], 2);
        assert_eq!(counts["error"], 1);
    }

    #[test]
    fn collector_total_tokens() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::TokensUsed {
            run_id: "r1".into(),
            input_tokens: 100,
            output_tokens: 200,
        });
        c.record(MetricEvent::TokensUsed {
            run_id: "r2".into(),
            input_tokens: 50,
            output_tokens: 75,
        });
        let (input, output) = c.total_tokens();
        assert_eq!(input, 150);
        assert_eq!(output, 275);
    }

    #[test]
    fn collector_error_count() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::Error {
            run_id: "r1".into(),
            code: "E001".into(),
            message: "a".into(),
            classification: "transient".into(),
        });
        c.record(MetricEvent::Error {
            run_id: "r2".into(),
            code: "E002".into(),
            message: "b".into(),
            classification: "permanent".into(),
        });
        c.record(MetricEvent::RequestStarted {
            run_id: "r3".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.error_count(), 2);
    }

    #[test]
    fn collector_error_rate() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::RequestCompleted {
            run_id: "r1".into(),
            duration_ms: 50,
            success: true,
            timestamp: "t".into(),
        });
        c.record(MetricEvent::RequestCompleted {
            run_id: "r2".into(),
            duration_ms: 60,
            success: false,
            timestamp: "t".into(),
        });
        c.record(MetricEvent::Error {
            run_id: "r2".into(),
            code: "E001".into(),
            message: "fail".into(),
            classification: "transient".into(),
        });
        assert!((c.error_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn collector_error_rate_no_completions() {
        let c = MetricEventCollector::new();
        assert_eq!(c.error_rate(), 0.0);
    }

    #[test]
    fn collector_clear() {
        let c = MetricEventCollector::new();
        c.record(MetricEvent::RequestStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn collector_thread_safety() {
        let c = MetricEventCollector::new();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let c = c.clone();
                std::thread::spawn(move || {
                    for i in 0..50 {
                        c.record(MetricEvent::TokensUsed {
                            run_id: format!("r{i}"),
                            input_tokens: 10,
                            output_tokens: 20,
                        });
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(c.len(), 200);
    }
}
