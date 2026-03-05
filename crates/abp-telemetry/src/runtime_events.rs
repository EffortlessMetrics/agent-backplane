// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code)]
//! Predefined runtime telemetry events for key ABP lifecycle moments.
//!
//! [`RuntimeEvent`] captures the eight core lifecycle events that occur during
//! agent run processing: start, completion, event streaming, backend selection,
//! policy evaluation, receipt generation, errors, and dialect rewrites.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// RuntimeEvent
// ---------------------------------------------------------------------------

/// Strongly-typed runtime events emitted during agent run processing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    /// A run has been initiated.
    RunStarted {
        /// Unique run identifier.
        run_id: String,
        /// Backend selected for the run.
        backend: String,
        /// ISO-8601 timestamp.
        timestamp: String,
    },
    /// A run completed (successfully or otherwise).
    RunCompleted {
        /// Unique run identifier.
        run_id: String,
        /// Wall-clock duration in milliseconds.
        duration_ms: u64,
        /// Whether the run succeeded.
        success: bool,
        /// ISO-8601 timestamp.
        timestamp: String,
    },
    /// An agent event was streamed from the backend.
    EventStreamed {
        /// Unique run identifier.
        run_id: String,
        /// Kind of agent event (e.g. `"tool_call"`, `"message"`).
        event_kind: String,
        /// Sequence number within the run.
        sequence: u64,
    },
    /// A backend was selected for a run.
    BackendSelected {
        /// Unique run identifier.
        run_id: String,
        /// Name of the backend that was selected.
        backend: String,
    },
    /// A policy was evaluated against a run.
    PolicyEvaluated {
        /// Unique run identifier.
        run_id: String,
        /// Name of the policy profile that was evaluated.
        policy_name: String,
        /// Whether the policy allowed the operation.
        allowed: bool,
    },
    /// A receipt was generated for a completed run.
    ReceiptGenerated {
        /// Unique run identifier.
        run_id: String,
        /// SHA-256 hash of the receipt, if computed.
        receipt_hash: Option<String>,
    },
    /// An error occurred during processing.
    ErrorOccurred {
        /// Unique run identifier.
        run_id: String,
        /// Machine-readable error code.
        code: String,
        /// Human-readable error message.
        message: String,
    },
    /// A dialect rewrite was applied during processing.
    RewriteApplied {
        /// Unique run identifier.
        run_id: String,
        /// Source dialect (e.g. `"openai"`).
        from_dialect: String,
        /// Target dialect (e.g. `"anthropic"`).
        to_dialect: String,
    },
}

impl RuntimeEvent {
    /// Return the run_id associated with this event.
    pub fn run_id(&self) -> &str {
        match self {
            Self::RunStarted { run_id, .. }
            | Self::RunCompleted { run_id, .. }
            | Self::EventStreamed { run_id, .. }
            | Self::BackendSelected { run_id, .. }
            | Self::PolicyEvaluated { run_id, .. }
            | Self::ReceiptGenerated { run_id, .. }
            | Self::ErrorOccurred { run_id, .. }
            | Self::RewriteApplied { run_id, .. } => run_id,
        }
    }

    /// Return a short label for the event variant.
    pub fn label(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::RunCompleted { .. } => "run_completed",
            Self::EventStreamed { .. } => "event_streamed",
            Self::BackendSelected { .. } => "backend_selected",
            Self::PolicyEvaluated { .. } => "policy_evaluated",
            Self::ReceiptGenerated { .. } => "receipt_generated",
            Self::ErrorOccurred { .. } => "error_occurred",
            Self::RewriteApplied { .. } => "rewrite_applied",
        }
    }
}

// ---------------------------------------------------------------------------
// RuntimeEventCollector
// ---------------------------------------------------------------------------

/// Thread-safe collector for [`RuntimeEvent`]s.
#[derive(Debug, Clone, Default)]
pub struct RuntimeEventCollector {
    events: Arc<Mutex<Vec<RuntimeEvent>>>,
}

impl RuntimeEventCollector {
    /// Create a new, empty collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event.
    pub fn emit(&self, event: RuntimeEvent) {
        let mut guard = self
            .events
            .lock()
            .expect("runtime event collector lock poisoned");
        guard.push(event);
    }

    /// Return all collected events.
    pub fn events(&self) -> Vec<RuntimeEvent> {
        let guard = self
            .events
            .lock()
            .expect("runtime event collector lock poisoned");
        guard.clone()
    }

    /// Number of collected events.
    pub fn len(&self) -> usize {
        let guard = self
            .events
            .lock()
            .expect("runtime event collector lock poisoned");
        guard.len()
    }

    /// Whether the collector is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return events matching a specific label.
    pub fn events_with_label(&self, label: &str) -> Vec<RuntimeEvent> {
        let guard = self
            .events
            .lock()
            .expect("runtime event collector lock poisoned");
        guard
            .iter()
            .filter(|e| e.label() == label)
            .cloned()
            .collect()
    }

    /// Return events for a specific run.
    pub fn events_for_run(&self, run_id: &str) -> Vec<RuntimeEvent> {
        let guard = self
            .events
            .lock()
            .expect("runtime event collector lock poisoned");
        guard
            .iter()
            .filter(|e| e.run_id() == run_id)
            .cloned()
            .collect()
    }

    /// Clear all collected events.
    pub fn clear(&self) {
        let mut guard = self
            .events
            .lock()
            .expect("runtime event collector lock poisoned");
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
    fn run_started_construction() {
        let ev = RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "run_started");
    }

    #[test]
    fn run_completed_construction() {
        let ev = RuntimeEvent::RunCompleted {
            run_id: "r1".into(),
            duration_ms: 150,
            success: true,
            timestamp: "2025-01-01T00:00:00Z".into(),
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "run_completed");
    }

    #[test]
    fn event_streamed_construction() {
        let ev = RuntimeEvent::EventStreamed {
            run_id: "r1".into(),
            event_kind: "tool_call".into(),
            sequence: 3,
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "event_streamed");
    }

    #[test]
    fn backend_selected_construction() {
        let ev = RuntimeEvent::BackendSelected {
            run_id: "r1".into(),
            backend: "sidecar:node".into(),
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "backend_selected");
    }

    #[test]
    fn policy_evaluated_construction() {
        let ev = RuntimeEvent::PolicyEvaluated {
            run_id: "r1".into(),
            policy_name: "strict".into(),
            allowed: true,
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "policy_evaluated");
    }

    #[test]
    fn policy_evaluated_denied() {
        let ev = RuntimeEvent::PolicyEvaluated {
            run_id: "r2".into(),
            policy_name: "readonly".into(),
            allowed: false,
        };
        assert_eq!(ev.run_id(), "r2");
        assert_eq!(ev.label(), "policy_evaluated");
        if let RuntimeEvent::PolicyEvaluated { allowed, .. } = &ev {
            assert!(!allowed);
        }
    }

    #[test]
    fn receipt_generated_construction() {
        let ev = RuntimeEvent::ReceiptGenerated {
            run_id: "r1".into(),
            receipt_hash: Some("abc123".into()),
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "receipt_generated");
    }

    #[test]
    fn receipt_generated_no_hash() {
        let ev = RuntimeEvent::ReceiptGenerated {
            run_id: "r1".into(),
            receipt_hash: None,
        };
        if let RuntimeEvent::ReceiptGenerated { receipt_hash, .. } = &ev {
            assert!(receipt_hash.is_none());
        }
    }

    #[test]
    fn error_occurred_construction() {
        let ev = RuntimeEvent::ErrorOccurred {
            run_id: "r1".into(),
            code: "E001".into(),
            message: "something failed".into(),
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "error_occurred");
    }

    #[test]
    fn rewrite_applied_construction() {
        let ev = RuntimeEvent::RewriteApplied {
            run_id: "r1".into(),
            from_dialect: "openai".into(),
            to_dialect: "anthropic".into(),
        };
        assert_eq!(ev.run_id(), "r1");
        assert_eq!(ev.label(), "rewrite_applied");
    }

    #[test]
    fn all_labels_correct() {
        let cases: Vec<(RuntimeEvent, &str)> = vec![
            (
                RuntimeEvent::RunStarted {
                    run_id: "r".into(),
                    backend: "b".into(),
                    timestamp: "t".into(),
                },
                "run_started",
            ),
            (
                RuntimeEvent::RunCompleted {
                    run_id: "r".into(),
                    duration_ms: 0,
                    success: true,
                    timestamp: "t".into(),
                },
                "run_completed",
            ),
            (
                RuntimeEvent::EventStreamed {
                    run_id: "r".into(),
                    event_kind: "msg".into(),
                    sequence: 0,
                },
                "event_streamed",
            ),
            (
                RuntimeEvent::BackendSelected {
                    run_id: "r".into(),
                    backend: "mock".into(),
                },
                "backend_selected",
            ),
            (
                RuntimeEvent::PolicyEvaluated {
                    run_id: "r".into(),
                    policy_name: "p".into(),
                    allowed: true,
                },
                "policy_evaluated",
            ),
            (
                RuntimeEvent::ReceiptGenerated {
                    run_id: "r".into(),
                    receipt_hash: None,
                },
                "receipt_generated",
            ),
            (
                RuntimeEvent::ErrorOccurred {
                    run_id: "r".into(),
                    code: "E".into(),
                    message: "m".into(),
                },
                "error_occurred",
            ),
            (
                RuntimeEvent::RewriteApplied {
                    run_id: "r".into(),
                    from_dialect: "a".into(),
                    to_dialect: "b".into(),
                },
                "rewrite_applied",
            ),
        ];
        for (event, expected) in cases {
            assert_eq!(event.label(), expected);
        }
    }

    #[test]
    fn serde_roundtrip_run_started() {
        let ev = RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: RuntimeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn serde_roundtrip_policy_evaluated() {
        let ev = RuntimeEvent::PolicyEvaluated {
            run_id: "r1".into(),
            policy_name: "strict".into(),
            allowed: false,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: RuntimeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn serde_roundtrip_receipt_generated() {
        let ev = RuntimeEvent::ReceiptGenerated {
            run_id: "r1".into(),
            receipt_hash: Some("sha256:abc".into()),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: RuntimeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn serde_roundtrip_rewrite_applied() {
        let ev = RuntimeEvent::RewriteApplied {
            run_id: "r1".into(),
            from_dialect: "openai".into(),
            to_dialect: "anthropic".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: RuntimeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn json_tag_format() {
        let ev = RuntimeEvent::PolicyEvaluated {
            run_id: "r1".into(),
            policy_name: "p".into(),
            allowed: true,
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"policy_evaluated\""));
    }

    #[test]
    fn json_tag_format_rewrite() {
        let ev = RuntimeEvent::RewriteApplied {
            run_id: "r1".into(),
            from_dialect: "a".into(),
            to_dialect: "b".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"rewrite_applied\""));
    }

    #[test]
    fn collector_basic() {
        let c = RuntimeEventCollector::new();
        assert!(c.is_empty());
        c.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.len(), 1);
        assert!(!c.is_empty());
    }

    #[test]
    fn collector_filter_by_label() {
        let c = RuntimeEventCollector::new();
        c.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.emit(RuntimeEvent::PolicyEvaluated {
            run_id: "r1".into(),
            policy_name: "p".into(),
            allowed: true,
        });
        c.emit(RuntimeEvent::RewriteApplied {
            run_id: "r1".into(),
            from_dialect: "a".into(),
            to_dialect: "b".into(),
        });
        assert_eq!(c.events_with_label("policy_evaluated").len(), 1);
        assert_eq!(c.events_with_label("rewrite_applied").len(), 1);
        assert_eq!(c.events_with_label("run_started").len(), 1);
    }

    #[test]
    fn collector_filter_by_run() {
        let c = RuntimeEventCollector::new();
        c.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.emit(RuntimeEvent::RunStarted {
            run_id: "r2".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.events_for_run("r1").len(), 1);
        assert_eq!(c.events_for_run("r2").len(), 1);
        assert_eq!(c.events_for_run("r3").len(), 0);
    }

    #[test]
    fn collector_clear() {
        let c = RuntimeEventCollector::new();
        c.emit(RuntimeEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn collector_thread_safety() {
        let c = RuntimeEventCollector::new();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let c = c.clone();
                std::thread::spawn(move || {
                    for i in 0..50 {
                        c.emit(RuntimeEvent::EventStreamed {
                            run_id: "r1".into(),
                            event_kind: "msg".into(),
                            sequence: i,
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
