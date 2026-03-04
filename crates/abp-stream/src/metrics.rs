// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stream metrics tracking for [`AgentEvent`] sequences.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, Instant};

use abp_core::AgentEvent;

use crate::event_kind_name;

/// Formatted snapshot of stream metrics.
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    /// Total number of events recorded.
    pub event_count: u64,
    /// Total bytes from assistant delta text payloads.
    pub total_bytes: u64,
    /// Time elapsed since the first event was recorded.
    pub elapsed: Duration,
    /// Events per second throughput.
    pub events_per_second: f64,
    /// Event counts grouped by kind name (deterministic order).
    pub event_type_counts: BTreeMap<String, u64>,
}

impl fmt::Display for MetricsSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Events: {}", self.event_count)?;
        writeln!(f, "Bytes: {}", self.total_bytes)?;
        writeln!(f, "Elapsed: {:.3}s", self.elapsed.as_secs_f64())?;
        writeln!(f, "Throughput: {:.2} events/s", self.events_per_second)?;
        for (kind, count) in &self.event_type_counts {
            writeln!(f, "  {kind}: {count}")?;
        }
        Ok(())
    }
}

/// Tracks stream metrics: event counts, byte totals, timing, latency, and per-kind breakdowns.
#[derive(Debug)]
pub struct StreamMetrics {
    event_count: u64,
    total_bytes: u64,
    first_event_time: Option<Instant>,
    last_event_time: Option<Instant>,
    event_type_counts: BTreeMap<String, u64>,
    /// Per-event latency (time between consecutive events).
    latencies: Vec<Duration>,
}

impl StreamMetrics {
    /// Create a new empty metrics tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            event_count: 0,
            total_bytes: 0,
            first_event_time: None,
            last_event_time: None,
            event_type_counts: BTreeMap::new(),
            latencies: Vec::new(),
        }
    }

    /// Record an event, updating all tracked metrics.
    pub fn record_event(&mut self, event: &AgentEvent) {
        let now = Instant::now();
        if let Some(last) = self.last_event_time {
            self.latencies.push(now.duration_since(last));
        }
        if self.first_event_time.is_none() {
            self.first_event_time = Some(now);
        }
        self.last_event_time = Some(now);

        self.event_count += 1;

        if let abp_core::AgentEventKind::AssistantDelta { ref text } = event.kind {
            self.total_bytes += text.len() as u64;
        }

        let kind = event_kind_name(&event.kind);
        *self.event_type_counts.entry(kind).or_insert(0) += 1;
    }

    /// Return a formatted summary of all tracked metrics.
    #[must_use]
    pub fn summary(&self) -> MetricsSummary {
        MetricsSummary {
            event_count: self.event_count,
            total_bytes: self.total_bytes,
            elapsed: self.elapsed(),
            events_per_second: self.throughput(),
            event_type_counts: self.event_type_counts.clone(),
        }
    }

    /// Duration since the first event was recorded.
    ///
    /// Returns [`Duration::ZERO`] if no events have been recorded.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        match self.first_event_time {
            Some(first) => first.elapsed(),
            None => Duration::ZERO,
        }
    }

    /// Events per second throughput based on time between first and last event.
    ///
    /// Returns `0.0` if fewer than two events have been recorded or if
    /// no measurable time has passed between the first and last event.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        match (self.first_event_time, self.last_event_time) {
            (Some(first), Some(last)) => {
                let secs = last.duration_since(first).as_secs_f64();
                if secs > 0.0 {
                    self.event_count as f64 / secs
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    /// Total number of events recorded.
    #[must_use]
    pub fn event_count(&self) -> u64 {
        self.event_count
    }

    /// Total bytes from assistant delta text payloads.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Per-kind event counts (deterministic `BTreeMap` ordering).
    #[must_use]
    pub fn event_type_counts(&self) -> &BTreeMap<String, u64> {
        &self.event_type_counts
    }

    /// Per-event latencies (time between consecutive events).
    ///
    /// The vec contains `event_count - 1` entries (one per inter-event gap).
    #[must_use]
    pub fn latencies(&self) -> &[Duration] {
        &self.latencies
    }

    /// Average latency between consecutive events.
    ///
    /// Returns [`Duration::ZERO`] if fewer than two events have been recorded.
    #[must_use]
    pub fn average_latency(&self) -> Duration {
        if self.latencies.is_empty() {
            return Duration::ZERO;
        }
        let total: Duration = self.latencies.iter().sum();
        total / self.latencies.len() as u32
    }

    /// Minimum latency between consecutive events.
    ///
    /// Returns [`Duration::ZERO`] if fewer than two events have been recorded.
    #[must_use]
    pub fn min_latency(&self) -> Duration {
        self.latencies.iter().copied().min().unwrap_or(Duration::ZERO)
    }

    /// Maximum latency between consecutive events.
    ///
    /// Returns [`Duration::ZERO`] if fewer than two events have been recorded.
    #[must_use]
    pub fn max_latency(&self) -> Duration {
        self.latencies.iter().copied().max().unwrap_or(Duration::ZERO)
    }
}

impl Default for StreamMetrics {
    fn default() -> Self {
        Self::new()
    }
}
