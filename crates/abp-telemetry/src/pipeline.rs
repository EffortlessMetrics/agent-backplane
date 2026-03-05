// SPDX-License-Identifier: MIT OR Apache-2.0
//! Telemetry event pipeline: collecting, filtering, and summarising structured events.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// TelemetryEventType
// ---------------------------------------------------------------------------

/// Discriminator for telemetry events emitted during agent runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryEventType {
    /// A run has started.
    RunStarted,
    /// A run completed successfully.
    RunCompleted,
    /// A run failed.
    RunFailed,
    /// A backend was selected for the run.
    BackendSelected,
    /// A retry was attempted.
    RetryAttempted,
    /// A fallback was triggered.
    FallbackTriggered,
    /// Capability negotiation occurred.
    CapabilityNegotiated,
    /// A dialect mapping was performed.
    MappingPerformed,
}

impl std::fmt::Display for TelemetryEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", self));
        f.write_str(&s)
    }
}

// ---------------------------------------------------------------------------
// TelemetryEvent
// ---------------------------------------------------------------------------

/// A single structured telemetry event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Kind of event.
    pub event_type: TelemetryEventType,
    /// Associated run identifier.
    pub run_id: Option<String>,
    /// Backend that produced or handled this event.
    pub backend: Option<String>,
    /// Arbitrary key-value metadata (deterministic ordering).
    pub metadata: BTreeMap<String, serde_json::Value>,
    /// Duration in milliseconds (where applicable).
    pub duration_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// TelemetryFilter
// ---------------------------------------------------------------------------

/// Predicate for selecting which events pass through the pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryFilter {
    /// If set, only events whose type is in this list pass through.
    pub allowed_types: Option<Vec<TelemetryEventType>>,
    /// If set, events with `duration_ms` below this value are rejected.
    pub min_duration_ms: Option<u64>,
}

impl TelemetryFilter {
    /// Returns `true` when the event satisfies all filter predicates.
    pub fn matches(&self, event: &TelemetryEvent) -> bool {
        if let Some(ref allowed) = self.allowed_types {
            if !allowed.contains(&event.event_type) {
                return false;
            }
        }
        if let Some(min) = self.min_duration_ms {
            match event.duration_ms {
                Some(d) if d >= min => {}
                Some(_) => return false,
                // Events without a duration are not filtered by duration.
                None => {}
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// TelemetrySummary
// ---------------------------------------------------------------------------

/// Aggregate statistics produced by [`TelemetryCollector::summary`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySummary {
    /// Total number of collected events.
    pub total_events: usize,
    /// Event counts keyed by type name.
    pub events_by_type: BTreeMap<String, usize>,
    /// Mean duration of `RunCompleted` events (if any).
    pub average_run_duration_ms: Option<u64>,
    /// Fraction of `RunFailed` events over total run outcomes.
    pub error_rate: f64,
}

// ---------------------------------------------------------------------------
// TelemetryCollector
// ---------------------------------------------------------------------------

/// Collects telemetry events with optional filtering.
#[derive(Debug, Clone, Default)]
pub struct TelemetryCollector {
    events: Vec<TelemetryEvent>,
    filter: Option<TelemetryFilter>,
}

impl TelemetryCollector {
    /// Create a new collector with no filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a collector that applies the given filter on [`record`](Self::record).
    pub fn with_filter(filter: TelemetryFilter) -> Self {
        Self {
            events: Vec::new(),
            filter: Some(filter),
        }
    }

    /// Record an event. If a filter is set the event is silently dropped when
    /// it does not match.
    pub fn record(&mut self, event: TelemetryEvent) {
        if let Some(ref f) = self.filter {
            if !f.matches(&event) {
                return;
            }
        }
        self.events.push(event);
    }

    /// All collected events.
    pub fn events(&self) -> &[TelemetryEvent] {
        &self.events
    }

    /// Events matching a specific type.
    pub fn events_of_type(&self, t: TelemetryEventType) -> Vec<&TelemetryEvent> {
        self.events.iter().filter(|e| e.event_type == t).collect()
    }

    /// Events associated with a specific run.
    pub fn run_events(&self, run_id: &str) -> Vec<&TelemetryEvent> {
        self.events
            .iter()
            .filter(|e| e.run_id.as_deref() == Some(run_id))
            .collect()
    }

    /// Produce an aggregate summary of all collected events.
    pub fn summary(&self) -> TelemetrySummary {
        let total_events = self.events.len();

        let mut events_by_type: BTreeMap<String, usize> = BTreeMap::new();
        for e in &self.events {
            *events_by_type.entry(e.event_type.to_string()).or_insert(0) += 1;
        }

        // Average duration of RunCompleted events.
        let completed: Vec<u64> = self
            .events
            .iter()
            .filter(|e| e.event_type == TelemetryEventType::RunCompleted)
            .filter_map(|e| e.duration_ms)
            .collect();
        let average_run_duration_ms = if completed.is_empty() {
            None
        } else {
            Some(completed.iter().sum::<u64>() / completed.len() as u64)
        };

        // Error rate = RunFailed / (RunCompleted + RunFailed).
        let failed = self
            .events
            .iter()
            .filter(|e| e.event_type == TelemetryEventType::RunFailed)
            .count();
        let total_outcomes = completed.len() + failed;
        let error_rate = if total_outcomes == 0 {
            0.0
        } else {
            failed as f64 / total_outcomes as f64
        };

        TelemetrySummary {
            total_events,
            events_by_type,
            average_run_duration_ms,
            error_rate,
        }
    }

    /// Remove all collected events.
    pub fn clear(&mut self) {
        self.events.clear();
    }
}
