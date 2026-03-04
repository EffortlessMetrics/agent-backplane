// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

use abp_core::{AgentEvent, AgentEventKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Return the snake_case discriminant name for an [`AgentEventKind`].
pub fn event_kind_name(kind: &AgentEventKind) -> String {
    match kind {
        AgentEventKind::RunStarted { .. } => "run_started".to_string(),
        AgentEventKind::RunCompleted { .. } => "run_completed".to_string(),
        AgentEventKind::AssistantDelta { .. } => "assistant_delta".to_string(),
        AgentEventKind::AssistantMessage { .. } => "assistant_message".to_string(),
        AgentEventKind::ToolCall { .. } => "tool_call".to_string(),
        AgentEventKind::ToolResult { .. } => "tool_result".to_string(),
        AgentEventKind::FileChanged { .. } => "file_changed".to_string(),
        AgentEventKind::CommandExecuted { .. } => "command_executed".to_string(),
        AgentEventKind::Warning { .. } => "warning".to_string(),
        AgentEventKind::Error { .. } => "error".to_string(),
    }
}

/// Records all events for later replay or inspection.
#[derive(Debug, Clone, Default)]
pub struct EventRecorder {
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

impl EventRecorder {
    /// Create a new empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event.
    pub fn record(&self, event: &AgentEvent) {
        self.events
            .lock()
            .expect("recorder lock poisoned")
            .push(event.clone());
    }

    /// Return a snapshot of all recorded events.
    pub fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().expect("recorder lock poisoned").clone()
    }

    /// Number of recorded events.
    pub fn len(&self) -> usize {
        self.events.lock().expect("recorder lock poisoned").len()
    }

    /// Whether no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all recorded events.
    pub fn clear(&self) {
        self.events.lock().expect("recorder lock poisoned").clear();
    }
}

/// Tracks event statistics: count by kind, total tokens, timing.
#[derive(Debug, Clone, Default)]
pub struct EventStats {
    inner: Arc<Mutex<StatsInner>>,
}

#[derive(Debug, Default)]
struct StatsInner {
    counts: HashMap<String, u64>,
    total_events: u64,
    total_delta_bytes: u64,
    error_count: u64,
}

impl EventStats {
    /// Create a new empty stats tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event's statistics.
    pub fn observe(&self, event: &AgentEvent) {
        let mut inner = self.inner.lock().expect("stats lock poisoned");
        let name = event_kind_name(&event.kind);
        *inner.counts.entry(name).or_insert(0) += 1;
        inner.total_events += 1;

        if let AgentEventKind::AssistantDelta { ref text } = event.kind {
            inner.total_delta_bytes += text.len() as u64;
        }
        if matches!(event.kind, AgentEventKind::Error { .. }) {
            inner.error_count += 1;
        }
    }

    /// Total number of events observed.
    pub fn total_events(&self) -> u64 {
        self.inner.lock().expect("stats lock poisoned").total_events
    }

    /// Count of events for a given kind name.
    pub fn count_for(&self, kind_name: &str) -> u64 {
        self.inner
            .lock()
            .expect("stats lock poisoned")
            .counts
            .get(kind_name)
            .copied()
            .unwrap_or(0)
    }

    /// Total bytes from `AssistantDelta` text payloads.
    pub fn total_delta_bytes(&self) -> u64 {
        self.inner
            .lock()
            .expect("stats lock poisoned")
            .total_delta_bytes
    }

    /// Number of error events observed.
    pub fn error_count(&self) -> u64 {
        self.inner.lock().expect("stats lock poisoned").error_count
    }

    /// Return a snapshot of per-kind counts.
    pub fn kind_counts(&self) -> HashMap<String, u64> {
        self.inner
            .lock()
            .expect("stats lock poisoned")
            .counts
            .clone()
    }

    /// Reset all statistics.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().expect("stats lock poisoned");
        inner.counts.clear();
        inner.total_events = 0;
        inner.total_delta_bytes = 0;
        inner.error_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    #[test]
    fn recorder_roundtrip() {
        let recorder = EventRecorder::new();
        recorder.record(&event(AgentEventKind::Warning {
            message: "warn".into(),
        }));
        assert_eq!(recorder.len(), 1);
        assert!(!recorder.is_empty());
        recorder.clear();
        assert!(recorder.is_empty());
    }

    #[test]
    fn stats_counts_and_sizes() {
        let stats = EventStats::new();
        stats.observe(&event(AgentEventKind::AssistantDelta {
            text: "abc".into(),
        }));
        stats.observe(&event(AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        }));

        assert_eq!(stats.total_events(), 2);
        assert_eq!(stats.count_for("assistant_delta"), 1);
        assert_eq!(stats.total_delta_bytes(), 3);
        assert_eq!(stats.error_count(), 1);
    }

    #[test]
    fn kind_names_cover_all_variants() {
        assert_eq!(
            event_kind_name(&AgentEventKind::RunStarted {
                message: String::new()
            }),
            "run_started"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::RunCompleted {
                message: String::new()
            }),
            "run_completed"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::AssistantDelta {
                text: String::new()
            }),
            "assistant_delta"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::AssistantMessage {
                text: String::new()
            }),
            "assistant_message"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::ToolCall {
                tool_name: String::new(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!(null),
            }),
            "tool_call"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::ToolResult {
                tool_name: String::new(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: false,
            }),
            "tool_result"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::FileChanged {
                path: String::new(),
                summary: String::new(),
            }),
            "file_changed"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::CommandExecuted {
                command: String::new(),
                exit_code: None,
                output_preview: None,
            }),
            "command_executed"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::Warning {
                message: String::new()
            }),
            "warning"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::Error {
                message: String::new(),
                error_code: None,
            }),
            "error"
        );
    }
}
