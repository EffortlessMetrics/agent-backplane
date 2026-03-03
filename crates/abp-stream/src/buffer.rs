// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded ring buffer for recent [`AgentEvent`]s.

use abp_core::AgentEvent;

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
