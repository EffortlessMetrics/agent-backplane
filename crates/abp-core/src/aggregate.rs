// SPDX-License-Identifier: MIT OR Apache-2.0
//! Event aggregation and analytics for [`AgentEvent`] sequences.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::filter::kind_name;
use crate::{AgentEvent, AgentEventKind};

/// Incrementally aggregates statistics from a stream of [`AgentEvent`]s.
///
/// # Examples
///
/// ```
/// use abp_core::aggregate::EventAggregator;
/// use abp_core::{AgentEvent, AgentEventKind};
/// use chrono::Utc;
///
/// let mut agg = EventAggregator::new();
///
/// agg.add(&AgentEvent { ts: Utc::now(), kind: AgentEventKind::RunStarted { message: "go".into() }, ext: None });
/// agg.add(&AgentEvent { ts: Utc::now(), kind: AgentEventKind::AssistantMessage { text: "hello".into() }, ext: None });
///
/// assert_eq!(agg.event_count(), 2);
/// assert!(!agg.has_errors());
///
/// let summary = agg.summary();
/// assert_eq!(summary.total_events, 2);
/// assert_eq!(summary.total_text_chars, 5); // "hello"
/// ```
#[derive(Debug, Clone)]
pub struct EventAggregator {
    events: Vec<AgentEvent>,
}

impl EventAggregator {
    /// Create a new, empty aggregator.
    #[must_use]
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Record an event for aggregation.
    pub fn add(&mut self, event: &AgentEvent) {
        self.events.push(event.clone());
    }

    /// Total number of events recorded.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Count events grouped by their serde kind name.
    #[must_use]
    pub fn count_by_kind(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for e in &self.events {
            *counts.entry(kind_name(&e.kind)).or_insert(0) += 1;
        }
        counts
    }

    /// RFC 3339 timestamp of the first recorded event, if any.
    #[must_use]
    pub fn first_timestamp(&self) -> Option<String> {
        self.events.first().map(|e| e.ts.to_rfc3339())
    }

    /// RFC 3339 timestamp of the last recorded event, if any.
    #[must_use]
    pub fn last_timestamp(&self) -> Option<String> {
        self.events.last().map(|e| e.ts.to_rfc3339())
    }

    /// Wall-clock duration in milliseconds between first and last event.
    ///
    /// Returns `None` when fewer than two events have been recorded or the
    /// duration is negative.
    #[must_use]
    pub fn duration_ms(&self) -> Option<u64> {
        if self.events.len() < 2 {
            return None;
        }
        let first = self.events.first()?.ts;
        let last = self.events.last()?.ts;
        let delta = (last - first).to_std().ok()?;
        Some(delta.as_millis() as u64)
    }

    /// Names of all tools invoked via `ToolCall` events, in order.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::ToolCall { tool_name, .. } => Some(tool_name.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Number of distinct tools used.
    #[must_use]
    pub fn unique_tool_count(&self) -> usize {
        self.events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::ToolCall { tool_name, .. } => Some(tool_name.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// Returns `true` if any `Error` event has been recorded.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::Error { .. }))
    }

    /// Collects all error messages from `Error` events.
    #[must_use]
    pub fn error_messages(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::Error { message } => Some(message.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Total character count across `AssistantDelta` and `AssistantMessage`
    /// text payloads.
    #[must_use]
    pub fn text_length(&self) -> usize {
        self.events
            .iter()
            .map(|e| match &e.kind {
                AgentEventKind::AssistantDelta { text } => text.len(),
                AgentEventKind::AssistantMessage { text } => text.len(),
                _ => 0,
            })
            .sum()
    }

    /// Produce a serializable summary snapshot of the current aggregation.
    #[must_use]
    pub fn summary(&self) -> AggregationSummary {
        AggregationSummary {
            total_events: self.event_count(),
            by_kind: self.count_by_kind(),
            tool_calls: self.tool_calls().len(),
            unique_tools: self.unique_tool_count(),
            errors: self.error_messages().len(),
            total_text_chars: self.text_length(),
            duration_ms: self.duration_ms(),
        }
    }
}

impl Default for EventAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable snapshot of aggregated event statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AggregationSummary {
    /// Total number of events.
    pub total_events: usize,
    /// Event counts keyed by kind name.
    pub by_kind: BTreeMap<String, usize>,
    /// Total number of tool call events.
    pub tool_calls: usize,
    /// Number of distinct tools used.
    pub unique_tools: usize,
    /// Number of error events.
    pub errors: usize,
    /// Total characters of assistant text.
    pub total_text_chars: usize,
    /// Wall-clock duration in milliseconds, if available.
    pub duration_ms: Option<u64>,
}

/// High-level analytics derived from a sequence of [`AgentEvent`]s.
#[derive(Debug, Clone)]
pub struct RunAnalytics {
    aggregator: EventAggregator,
}

impl RunAnalytics {
    /// Build analytics from a slice of events.
    #[must_use]
    pub fn from_events(events: &[AgentEvent]) -> Self {
        let mut aggregator = EventAggregator::new();
        for e in events {
            aggregator.add(e);
        }
        Self { aggregator }
    }

    /// Returns `true` when the run completed without any error events.
    #[must_use]
    pub fn is_successful(&self) -> bool {
        !self.aggregator.has_errors()
    }

    /// Ratio of tool-call events to total events.
    ///
    /// Returns `0.0` when there are no events.
    #[must_use]
    pub fn tool_usage_ratio(&self) -> f64 {
        let total = self.aggregator.event_count();
        if total == 0 {
            return 0.0;
        }
        self.aggregator.tool_calls().len() as f64 / total as f64
    }

    /// Average text characters per event.
    ///
    /// Returns `0.0` when there are no events.
    #[must_use]
    pub fn average_text_per_event(&self) -> f64 {
        let total = self.aggregator.event_count();
        if total == 0 {
            return 0.0;
        }
        self.aggregator.text_length() as f64 / total as f64
    }

    /// Access the underlying [`AggregationSummary`].
    #[must_use]
    pub fn summary(&self) -> AggregationSummary {
        self.aggregator.summary()
    }
}
