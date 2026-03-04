// SPDX-License-Identifier: MIT OR Apache-2.0
//! Replay buffer: stores events for late subscribers.

use std::collections::VecDeque;

use abp_core::AgentEvent;
use tokio::sync::broadcast;

/// A replay buffer that stores up to `capacity` events and provides them to
/// late subscribers along with a live stream of future events.
///
/// New subscribers receive all currently buffered events plus a
/// [`broadcast::Receiver`] for events sent after subscription.
#[derive(Debug)]
pub struct ReplayBuffer {
    buffer: VecDeque<AgentEvent>,
    capacity: usize,
    tx: broadcast::Sender<AgentEvent>,
}

/// A subscription returned by [`ReplayBuffer::subscribe`].
///
/// Contains a snapshot of buffered events and a receiver for live events.
#[derive(Debug)]
pub struct ReplaySubscription {
    /// Events that were buffered at the time of subscription.
    pub buffered: Vec<AgentEvent>,
    /// Receiver for events sent after the subscription was created.
    pub receiver: broadcast::Receiver<AgentEvent>,
}

impl ReplayBuffer {
    /// Create a new replay buffer with the given event capacity and broadcast
    /// channel capacity.
    ///
    /// `capacity` is the maximum number of historical events retained.
    /// `channel_capacity` is the broadcast channel buffer size for live events.
    ///
    /// # Panics
    ///
    /// Panics if either capacity is 0.
    pub fn new(capacity: usize, channel_capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        assert!(channel_capacity > 0, "channel_capacity must be > 0");
        let (tx, _) = broadcast::channel(channel_capacity);
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
            tx,
        }
    }

    /// Record an event: store it in the replay buffer and broadcast to
    /// live subscribers.
    ///
    /// If the buffer is at capacity the oldest event is evicted.
    /// Returns the number of live subscribers that received the event.
    pub fn send(&mut self, event: &AgentEvent) -> usize {
        if self.buffer.len() == self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(event.clone());
        self.tx.send(event.clone()).unwrap_or(0)
    }

    /// Subscribe to the replay buffer. Returns all buffered events and a
    /// receiver for future events.
    pub fn subscribe(&self) -> ReplaySubscription {
        let buffered: Vec<AgentEvent> = self.buffer.iter().cloned().collect();
        let receiver = self.tx.subscribe();
        ReplaySubscription { buffered, receiver }
    }

    /// Number of events currently in the buffer.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// The maximum number of events the buffer retains.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of active live subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Clear all buffered events (live subscribers are unaffected).
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEventKind;
    use chrono::Utc;

    fn delta(text: &str) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: text.to_string(),
            },
            ext: None,
        }
    }

    fn error_ev(msg: &str) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: msg.to_string(),
                error_code: None,
            },
            ext: None,
        }
    }

    #[test]
    fn replay_late_subscriber_gets_history() {
        let mut rb = ReplayBuffer::new(10, 16);
        rb.send(&delta("a"));
        rb.send(&delta("b"));
        rb.send(&delta("c"));

        let sub = rb.subscribe();
        assert_eq!(sub.buffered.len(), 3);
        assert!(
            matches!(&sub.buffered[0].kind, AgentEventKind::AssistantDelta { text } if text == "a")
        );
        assert!(
            matches!(&sub.buffered[2].kind, AgentEventKind::AssistantDelta { text } if text == "c")
        );
    }

    #[test]
    fn replay_live_events_after_subscribe() {
        let mut rb = ReplayBuffer::new(10, 16);
        let sub = rb.subscribe();
        assert!(sub.buffered.is_empty());

        rb.send(&delta("live"));
        let mut rx = sub.receiver;
        let received = rx.try_recv().unwrap();
        assert!(
            matches!(&received.kind, AgentEventKind::AssistantDelta { text } if text == "live")
        );
    }

    #[test]
    fn replay_capacity_evicts_oldest() {
        let mut rb = ReplayBuffer::new(3, 16);
        rb.send(&delta("1"));
        rb.send(&delta("2"));
        rb.send(&delta("3"));
        rb.send(&delta("4")); // evicts "1"

        assert_eq!(rb.len(), 3);
        let sub = rb.subscribe();
        assert_eq!(sub.buffered.len(), 3);
        assert!(
            matches!(&sub.buffered[0].kind, AgentEventKind::AssistantDelta { text } if text == "2")
        );
        assert!(
            matches!(&sub.buffered[2].kind, AgentEventKind::AssistantDelta { text } if text == "4")
        );
    }

    #[test]
    fn replay_empty_buffer() {
        let rb = ReplayBuffer::new(5, 16);
        let sub = rb.subscribe();
        assert!(sub.buffered.is_empty());
        assert!(rb.is_empty());
    }

    #[test]
    fn replay_clear() {
        let mut rb = ReplayBuffer::new(10, 16);
        rb.send(&delta("a"));
        rb.send(&delta("b"));
        assert_eq!(rb.len(), 2);

        rb.clear();
        assert!(rb.is_empty());
        let sub = rb.subscribe();
        assert!(sub.buffered.is_empty());
    }

    #[test]
    fn replay_subscriber_count() {
        let rb = ReplayBuffer::new(10, 16);
        assert_eq!(rb.subscriber_count(), 0);

        let sub1 = rb.subscribe();
        assert_eq!(rb.subscriber_count(), 1);

        let sub2 = rb.subscribe();
        assert_eq!(rb.subscriber_count(), 2);

        drop(sub1);
        assert_eq!(rb.subscriber_count(), 1);

        drop(sub2);
        assert_eq!(rb.subscriber_count(), 0);
    }

    #[test]
    fn replay_capacity_getter() {
        let rb = ReplayBuffer::new(42, 16);
        assert_eq!(rb.capacity(), 42);
    }

    #[test]
    fn replay_multiple_subscribers_get_same_history() {
        let mut rb = ReplayBuffer::new(10, 16);
        rb.send(&delta("x"));
        rb.send(&delta("y"));

        let s1 = rb.subscribe();
        let s2 = rb.subscribe();
        assert_eq!(s1.buffered.len(), 2);
        assert_eq!(s2.buffered.len(), 2);
    }

    #[test]
    fn replay_different_event_types() {
        let mut rb = ReplayBuffer::new(10, 16);
        rb.send(&delta("text"));
        rb.send(&error_ev("oops"));

        let sub = rb.subscribe();
        assert_eq!(sub.buffered.len(), 2);
        assert!(matches!(
            sub.buffered[0].kind,
            AgentEventKind::AssistantDelta { .. }
        ));
        assert!(matches!(sub.buffered[1].kind, AgentEventKind::Error { .. }));
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn replay_zero_capacity_panics() {
        let _ = ReplayBuffer::new(0, 16);
    }

    #[test]
    #[should_panic(expected = "channel_capacity must be > 0")]
    fn replay_zero_channel_capacity_panics() {
        let _ = ReplayBuffer::new(10, 0);
    }
}
