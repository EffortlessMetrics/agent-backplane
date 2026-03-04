// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded ring buffer for recent [`AgentEvent`]s and event buffering.
#![allow(dead_code, unused_imports)]

use abp_core::AgentEvent;
use std::collections::VecDeque;
use std::fmt;

/// A bounded buffer that retains the most recent events, evicting the oldest
/// when capacity is reached.
#[derive(Debug, Clone)]
pub struct StreamBuffer {
    events: Vec<AgentEvent>,
    capacity: usize,
}

impl StreamBuffer {
    /// Create a new buffer with the given maximum capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "StreamBuffer capacity must be > 0");
        Self {
            events: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Push an event into the buffer. If the buffer is full, the oldest event
    /// is evicted.
    pub fn push(&mut self, event: AgentEvent) {
        if self.events.len() == self.capacity {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Return the most recent `n` events as a slice. If fewer than `n` events
    /// are stored, returns all of them.
    #[must_use]
    pub fn recent(&self, n: usize) -> &[AgentEvent] {
        let start = self.events.len().saturating_sub(n);
        &self.events[start..]
    }

    /// Drain all events from the buffer, returning them in insertion order.
    pub fn drain(&mut self) -> Vec<AgentEvent> {
        std::mem::take(&mut self.events)
    }

    /// Returns `true` if the buffer has reached its maximum capacity.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.events.len() == self.capacity
    }

    /// Returns the number of events currently in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` if the buffer contains no events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the maximum capacity of the buffer.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ---------------------------------------------------------------------------
// EventBuffer — error on full
// ---------------------------------------------------------------------------

/// Error returned when pushing to a full [`EventBuffer`].
#[derive(Debug, Clone)]
pub struct BufferFullError(pub AgentEvent);

impl fmt::Display for BufferFullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "event buffer is full")
    }
}

impl std::error::Error for BufferFullError {}

/// A bounded event buffer that rejects pushes when full.
///
/// Unlike [`StreamBuffer`] which silently evicts old events, this buffer
/// returns an error when capacity is exceeded, allowing callers to apply
/// their own backpressure strategy.
#[derive(Debug, Clone)]
pub struct EventBuffer {
    events: VecDeque<AgentEvent>,
    max_size: usize,
}

impl EventBuffer {
    /// Create a new buffer with the given maximum size.
    ///
    /// # Panics
    ///
    /// Panics if `max_size` is zero.
    pub fn new(max_size: usize) -> Self {
        assert!(max_size > 0, "EventBuffer max_size must be > 0");
        Self {
            events: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    /// Push an event. Returns `Err(BufferFullError)` if the buffer is at
    /// capacity.
    pub fn push(&mut self, event: AgentEvent) -> Result<(), BufferFullError> {
        if self.events.len() >= self.max_size {
            return Err(BufferFullError(event));
        }
        self.events.push_back(event);
        Ok(())
    }

    /// Drain all events, returning them in insertion order.
    pub fn drain(&mut self) -> Vec<AgentEvent> {
        self.events.drain(..).collect()
    }

    /// Peek at the oldest event without removing it.
    pub fn peek(&self) -> Option<&AgentEvent> {
        self.events.front()
    }

    /// Number of buffered events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Whether the buffer is at capacity.
    pub fn is_full(&self) -> bool {
        self.events.len() >= self.max_size
    }

    /// Maximum size of the buffer.
    pub fn max_size(&self) -> usize {
        self.max_size
    }
}

// ---------------------------------------------------------------------------
// RingBuffer — fixed-size sliding window
// ---------------------------------------------------------------------------

/// A fixed-size ring buffer that overwrites the oldest event when full.
///
/// Uses a [`VecDeque`] internally for O(1) push/pop at both ends.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    events: VecDeque<AgentEvent>,
    capacity: usize,
}

impl RingBuffer {
    /// Create a new ring buffer with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "RingBuffer capacity must be > 0");
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push an event. If the buffer is full, the oldest event is evicted and
    /// returned.
    pub fn push(&mut self, event: AgentEvent) -> Option<AgentEvent> {
        let evicted = if self.events.len() == self.capacity {
            self.events.pop_front()
        } else {
            None
        };
        self.events.push_back(event);
        evicted
    }

    /// Drain all events in order.
    pub fn drain(&mut self) -> Vec<AgentEvent> {
        self.events.drain(..).collect()
    }

    /// Peek at the oldest event.
    pub fn peek(&self) -> Option<&AgentEvent> {
        self.events.front()
    }

    /// Peek at the newest event.
    pub fn peek_back(&self) -> Option<&AgentEvent> {
        self.events.back()
    }

    /// Number of events currently stored.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Whether the buffer is at capacity.
    pub fn is_full(&self) -> bool {
        self.events.len() == self.capacity
    }

    /// Maximum capacity of the ring buffer.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Iterate over events from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &AgentEvent> {
        self.events.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEventKind;
    use chrono::Utc;

    fn make_event(text: &str) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: text.to_string(),
            },
            ext: None,
        }
    }

    // -- EventBuffer tests --

    #[test]
    fn event_buffer_push_and_drain() {
        let mut buf = EventBuffer::new(3);
        assert!(buf.push(make_event("a")).is_ok());
        assert!(buf.push(make_event("b")).is_ok());
        assert_eq!(buf.len(), 2);

        let events = buf.drain();
        assert_eq!(events.len(), 2);
        assert!(buf.is_empty());
    }

    #[test]
    fn event_buffer_rejects_when_full() {
        let mut buf = EventBuffer::new(2);
        assert!(buf.push(make_event("a")).is_ok());
        assert!(buf.push(make_event("b")).is_ok());
        assert!(buf.is_full());

        let err = buf.push(make_event("c")).unwrap_err();
        assert!(matches!(&err.0.kind, AgentEventKind::AssistantDelta { text } if text == "c"));
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn event_buffer_peek() {
        let mut buf = EventBuffer::new(4);
        assert!(buf.peek().is_none());
        buf.push(make_event("first")).unwrap();
        buf.push(make_event("second")).unwrap();

        let peeked = buf.peek().unwrap();
        assert!(matches!(&peeked.kind, AgentEventKind::AssistantDelta { text } if text == "first"));
        // peek doesn't remove
        assert_eq!(buf.len(), 2);
    }

    #[test]
    #[should_panic(expected = "max_size must be > 0")]
    fn event_buffer_zero_capacity_panics() {
        EventBuffer::new(0);
    }

    // -- RingBuffer tests --

    #[test]
    fn ring_buffer_push_and_evict() {
        let mut ring = RingBuffer::new(3);
        assert!(ring.push(make_event("a")).is_none());
        assert!(ring.push(make_event("b")).is_none());
        assert!(ring.push(make_event("c")).is_none());
        assert!(ring.is_full());

        // Pushing a 4th evicts "a"
        let evicted = ring.push(make_event("d")).unwrap();
        assert!(matches!(&evicted.kind, AgentEventKind::AssistantDelta { text } if text == "a"));
        assert_eq!(ring.len(), 3);

        // Oldest is now "b"
        let front = ring.peek().unwrap();
        assert!(matches!(&front.kind, AgentEventKind::AssistantDelta { text } if text == "b"));
    }

    #[test]
    fn ring_buffer_drain() {
        let mut ring = RingBuffer::new(4);
        ring.push(make_event("x"));
        ring.push(make_event("y"));

        let events = ring.drain();
        assert_eq!(events.len(), 2);
        assert!(ring.is_empty());
    }

    #[test]
    fn ring_buffer_peek_back() {
        let mut ring = RingBuffer::new(4);
        assert!(ring.peek_back().is_none());
        ring.push(make_event("first"));
        ring.push(make_event("last"));

        let back = ring.peek_back().unwrap();
        assert!(matches!(&back.kind, AgentEventKind::AssistantDelta { text } if text == "last"));
    }

    #[test]
    fn ring_buffer_iter_order() {
        let mut ring = RingBuffer::new(3);
        ring.push(make_event("a"));
        ring.push(make_event("b"));
        ring.push(make_event("c"));
        ring.push(make_event("d")); // evicts "a"

        let texts: Vec<String> = ring
            .iter()
            .map(|ev| match &ev.kind {
                AgentEventKind::AssistantDelta { text } => text.clone(),
                _ => panic!("unexpected"),
            })
            .collect();
        assert_eq!(texts, vec!["b", "c", "d"]);
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn ring_buffer_zero_capacity_panics() {
        RingBuffer::new(0);
    }

    // -- StreamBuffer existing behavior --

    #[test]
    fn stream_buffer_evicts_oldest() {
        let mut buf = StreamBuffer::new(2);
        buf.push(make_event("a"));
        buf.push(make_event("b"));
        buf.push(make_event("c")); // evicts "a"
        assert_eq!(buf.len(), 2);
        let events = buf.drain();
        assert!(matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "b"));
        assert!(matches!(&events[1].kind, AgentEventKind::AssistantDelta { text } if text == "c"));
    }
}
