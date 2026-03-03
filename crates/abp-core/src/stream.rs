// SPDX-License-Identifier: MIT OR Apache-2.0
//! Event stream combinator utilities for [`AgentEvent`] sequences.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::filter::EventFilter;
use crate::{AgentEvent, filter::kind_name};

/// A wrapper around a `Vec<AgentEvent>` providing combinator utilities.
#[derive(Debug, Clone)]
pub struct EventStream {
    events: Vec<AgentEvent>,
}

impl EventStream {
    /// Create a new `EventStream` from a vector of events.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_core::stream::EventStream;
    /// # use abp_core::{AgentEvent, AgentEventKind};
    /// # use chrono::Utc;
    /// let events = vec![
    ///     AgentEvent { ts: Utc::now(), kind: AgentEventKind::RunStarted { message: "go".into() }, ext: None },
    ///     AgentEvent { ts: Utc::now(), kind: AgentEventKind::RunCompleted { message: "done".into() }, ext: None },
    /// ];
    /// let stream = EventStream::new(events);
    /// assert_eq!(stream.len(), 2);
    /// assert!(!stream.is_empty());
    /// ```
    #[must_use]
    pub fn new(events: Vec<AgentEvent>) -> Self {
        Self { events }
    }

    /// Return a new stream containing only events that pass `filter`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use abp_core::stream::EventStream;
    /// # use abp_core::filter::EventFilter;
    /// # use abp_core::{AgentEvent, AgentEventKind};
    /// # use chrono::Utc;
    /// let events = vec![
    ///     AgentEvent { ts: Utc::now(), kind: AgentEventKind::RunStarted { message: "go".into() }, ext: None },
    ///     AgentEvent { ts: Utc::now(), kind: AgentEventKind::Warning { message: "oops".into() }, ext: None },
    ///     AgentEvent { ts: Utc::now(), kind: AgentEventKind::RunCompleted { message: "done".into() }, ext: None },
    /// ];
    /// let stream = EventStream::new(events);
    /// let only_warnings = EventFilter::include_kinds(&["warning"]);
    /// let filtered = stream.filter(&only_warnings);
    /// assert_eq!(filtered.len(), 1);
    /// ```
    #[must_use]
    pub fn filter(&self, f: &EventFilter) -> Self {
        Self {
            events: self
                .events
                .iter()
                .filter(|e| f.matches(e))
                .cloned()
                .collect(),
        }
    }

    /// Return a new stream containing only events whose kind matches `kind`
    /// (case-insensitive, using the serde tag name).
    #[must_use]
    pub fn by_kind(&self, kind: &str) -> Self {
        let lower = kind.to_ascii_lowercase();
        Self {
            events: self
                .events
                .iter()
                .filter(|e| kind_name(&e.kind) == lower)
                .cloned()
                .collect(),
        }
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

    /// Return the first event matching `kind` (case-insensitive).
    #[must_use]
    pub fn first_of_kind(&self, kind: &str) -> Option<&AgentEvent> {
        let lower = kind.to_ascii_lowercase();
        self.events.iter().find(|e| kind_name(&e.kind) == lower)
    }

    /// Return the last event matching `kind` (case-insensitive).
    #[must_use]
    pub fn last_of_kind(&self, kind: &str) -> Option<&AgentEvent> {
        let lower = kind.to_ascii_lowercase();
        self.events
            .iter()
            .rev()
            .find(|e| kind_name(&e.kind) == lower)
    }

    /// Wall-clock duration between the first and last event timestamps.
    ///
    /// Returns `None` if the stream has fewer than two events or if the
    /// computed duration is negative.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        if self.events.len() < 2 {
            return None;
        }
        let first = self.events.first()?.ts;
        let last = self.events.last()?.ts;
        let delta = (last - first).to_std().ok()?;
        Some(delta)
    }

    /// Returns `true` if the stream contains no events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the number of events in the stream.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Iterate over the events by reference.
    pub fn iter(&self) -> std::slice::Iter<'_, AgentEvent> {
        self.events.iter()
    }

    /// Consume the stream and return the underlying vector.
    #[must_use]
    pub fn into_vec(self) -> Vec<AgentEvent> {
        self.events
    }

    // ── Combinator utilities ──────────────────────────────────────────

    /// Return a new stream keeping only events where `predicate` returns
    /// `true` (generic predicate filter).
    #[must_use]
    pub fn filter_pred(&self, predicate: impl Fn(&AgentEvent) -> bool) -> Self {
        Self {
            events: self
                .events
                .iter()
                .filter(|e| predicate(e))
                .cloned()
                .collect(),
        }
    }

    /// Return a new stream with every event transformed by `f`.
    #[must_use]
    pub fn map_events(&self, f: impl Fn(AgentEvent) -> AgentEvent) -> Self {
        Self {
            events: self.events.iter().cloned().map(f).collect(),
        }
    }

    /// Return a new stream containing events *before* the first event where
    /// `predicate` returns `true`. The matching event itself is **not**
    /// included.
    #[must_use]
    pub fn take_until(&self, predicate: impl Fn(&AgentEvent) -> bool) -> Self {
        let mut out = Vec::new();
        for e in &self.events {
            if predicate(e) {
                break;
            }
            out.push(e.clone());
        }
        Self { events: out }
    }

    /// Rate-limit the stream so that at most one event per `window` is kept.
    ///
    /// The first event is always included. Subsequent events are included only
    /// if their timestamp is at least `window` after the last included event.
    #[must_use]
    pub fn throttle(&self, window: Duration) -> Self {
        let mut out = Vec::new();
        let mut last_ts: Option<chrono::DateTime<chrono::Utc>> = None;
        for e in &self.events {
            match last_ts {
                None => {
                    last_ts = Some(e.ts);
                    out.push(e.clone());
                }
                Some(prev) => {
                    if let Ok(delta) = (e.ts - prev).to_std() {
                        if delta >= window {
                            last_ts = Some(e.ts);
                            out.push(e.clone());
                        }
                    }
                }
            }
        }
        Self { events: out }
    }

    /// Merge two streams into one, ordered by timestamp (stable — when
    /// timestamps are equal, events from `self` come first).
    #[must_use]
    pub fn merge(&self, other: &EventStream) -> Self {
        let mut merged = Vec::with_capacity(self.events.len() + other.events.len());
        let (mut i, mut j) = (0, 0);
        while i < self.events.len() && j < other.events.len() {
            if self.events[i].ts <= other.events[j].ts {
                merged.push(self.events[i].clone());
                i += 1;
            } else {
                merged.push(other.events[j].clone());
                j += 1;
            }
        }
        while i < self.events.len() {
            merged.push(self.events[i].clone());
            i += 1;
        }
        while j < other.events.len() {
            merged.push(other.events[j].clone());
            j += 1;
        }
        Self { events: merged }
    }
}

impl IntoIterator for EventStream {
    type Item = AgentEvent;
    type IntoIter = std::vec::IntoIter<AgentEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

impl<'a> IntoIterator for &'a EventStream {
    type Item = &'a AgentEvent;
    type IntoIter = std::slice::Iter<'a, AgentEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}
