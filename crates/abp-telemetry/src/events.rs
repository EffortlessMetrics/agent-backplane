// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Telemetry event types and emission infrastructure.
//!
//! This module provides a strongly-typed event enum for key ABP lifecycle
//! moments, an [`EventEmitter`] trait for publishing, and an
//! [`InMemoryEventCollector`] for testing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// TelemetryEvent
// ---------------------------------------------------------------------------

/// Strongly-typed telemetry events emitted during agent runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelemetryEvent {
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
    /// An error occurred during processing.
    ErrorOccurred {
        /// Unique run identifier.
        run_id: String,
        /// Machine-readable error code.
        code: String,
        /// Human-readable error message.
        message: String,
    },
    /// A backend was selected for a run.
    BackendSelected {
        /// Unique run identifier.
        run_id: String,
        /// Name of the backend that was selected.
        backend: String,
    },
}

impl TelemetryEvent {
    /// Return the run_id associated with this event.
    pub fn run_id(&self) -> &str {
        match self {
            Self::RunStarted { run_id, .. }
            | Self::RunCompleted { run_id, .. }
            | Self::EventStreamed { run_id, .. }
            | Self::ErrorOccurred { run_id, .. }
            | Self::BackendSelected { run_id, .. } => run_id,
        }
    }

    /// Return a short label for the event variant.
    pub fn label(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::RunCompleted { .. } => "run_completed",
            Self::EventStreamed { .. } => "event_streamed",
            Self::ErrorOccurred { .. } => "error_occurred",
            Self::BackendSelected { .. } => "backend_selected",
        }
    }
}

// ---------------------------------------------------------------------------
// EventEmitter trait
// ---------------------------------------------------------------------------

/// Trait for publishing telemetry events.
pub trait EventEmitter: Send + Sync {
    /// Emit a single telemetry event.
    fn emit(&self, event: TelemetryEvent);
}

// ---------------------------------------------------------------------------
// InMemoryEventCollector
// ---------------------------------------------------------------------------

/// An [`EventEmitter`] that collects events in memory for testing.
#[derive(Debug, Clone, Default)]
pub struct InMemoryEventCollector {
    events: Arc<Mutex<Vec<TelemetryEvent>>>,
}

impl InMemoryEventCollector {
    /// Create a new, empty collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return all collected events.
    pub fn events(&self) -> Vec<TelemetryEvent> {
        let guard = self.events.lock().expect("event collector lock poisoned");
        guard.clone()
    }

    /// Number of collected events.
    pub fn len(&self) -> usize {
        let guard = self.events.lock().expect("event collector lock poisoned");
        guard.len()
    }

    /// Whether the collector is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return events matching a specific label.
    pub fn events_with_label(&self, label: &str) -> Vec<TelemetryEvent> {
        let guard = self.events.lock().expect("event collector lock poisoned");
        guard
            .iter()
            .filter(|e| e.label() == label)
            .cloned()
            .collect()
    }

    /// Return events for a specific run.
    pub fn events_for_run(&self, run_id: &str) -> Vec<TelemetryEvent> {
        let guard = self.events.lock().expect("event collector lock poisoned");
        guard
            .iter()
            .filter(|e| e.run_id() == run_id)
            .cloned()
            .collect()
    }

    /// Clear all collected events.
    pub fn clear(&self) {
        let mut guard = self.events.lock().expect("event collector lock poisoned");
        guard.clear();
    }
}

impl EventEmitter for InMemoryEventCollector {
    fn emit(&self, event: TelemetryEvent) {
        let mut guard = self.events.lock().expect("event collector lock poisoned");
        guard.push(event);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_labels() {
        let ev = TelemetryEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
        };
        assert_eq!(ev.label(), "run_started");
        assert_eq!(ev.run_id(), "r1");
    }

    #[test]
    fn event_labels_all_variants() {
        let cases: Vec<(TelemetryEvent, &str)> = vec![
            (
                TelemetryEvent::RunStarted {
                    run_id: "r".into(),
                    backend: "b".into(),
                    timestamp: "t".into(),
                },
                "run_started",
            ),
            (
                TelemetryEvent::RunCompleted {
                    run_id: "r".into(),
                    duration_ms: 100,
                    success: true,
                    timestamp: "t".into(),
                },
                "run_completed",
            ),
            (
                TelemetryEvent::EventStreamed {
                    run_id: "r".into(),
                    event_kind: "tool_call".into(),
                    sequence: 0,
                },
                "event_streamed",
            ),
            (
                TelemetryEvent::ErrorOccurred {
                    run_id: "r".into(),
                    code: "E001".into(),
                    message: "oops".into(),
                },
                "error_occurred",
            ),
            (
                TelemetryEvent::BackendSelected {
                    run_id: "r".into(),
                    backend: "mock".into(),
                },
                "backend_selected",
            ),
        ];
        for (event, expected) in cases {
            assert_eq!(event.label(), expected);
        }
    }

    #[test]
    fn in_memory_collector_basic() {
        let c = InMemoryEventCollector::new();
        assert!(c.is_empty());

        c.emit(TelemetryEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.len(), 1);
        assert!(!c.is_empty());
    }

    #[test]
    fn in_memory_collector_filter_by_label() {
        let c = InMemoryEventCollector::new();
        c.emit(TelemetryEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.emit(TelemetryEvent::ErrorOccurred {
            run_id: "r1".into(),
            code: "E001".into(),
            message: "bad".into(),
        });
        c.emit(TelemetryEvent::RunCompleted {
            run_id: "r1".into(),
            duration_ms: 50,
            success: false,
            timestamp: "t".into(),
        });
        assert_eq!(c.events_with_label("error_occurred").len(), 1);
        assert_eq!(c.events_with_label("run_started").len(), 1);
    }

    #[test]
    fn in_memory_collector_filter_by_run() {
        let c = InMemoryEventCollector::new();
        c.emit(TelemetryEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        c.emit(TelemetryEvent::RunStarted {
            run_id: "r2".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.events_for_run("r1").len(), 1);
        assert_eq!(c.events_for_run("r2").len(), 1);
        assert_eq!(c.events_for_run("r3").len(), 0);
    }

    #[test]
    fn in_memory_collector_clear() {
        let c = InMemoryEventCollector::new();
        c.emit(TelemetryEvent::RunStarted {
            run_id: "r1".into(),
            backend: "mock".into(),
            timestamp: "t".into(),
        });
        assert_eq!(c.len(), 1);
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn in_memory_collector_thread_safety() {
        let c = InMemoryEventCollector::new();
        let c2 = c.clone();
        let handle = std::thread::spawn(move || {
            for i in 0..100 {
                c2.emit(TelemetryEvent::EventStreamed {
                    run_id: "r1".into(),
                    event_kind: "msg".into(),
                    sequence: i,
                });
            }
        });
        for i in 100..200 {
            c.emit(TelemetryEvent::EventStreamed {
                run_id: "r1".into(),
                event_kind: "msg".into(),
                sequence: i,
            });
        }
        handle.join().unwrap();
        assert_eq!(c.len(), 200);
    }

    #[test]
    fn event_serde_roundtrip() {
        let ev = TelemetryEvent::RunCompleted {
            run_id: "r1".into(),
            duration_ms: 42,
            success: true,
            timestamp: "2025-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: TelemetryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
    }
}
