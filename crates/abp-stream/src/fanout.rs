// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fan-out: send events to multiple consumers via [`tokio::sync::broadcast`].

use abp_core::AgentEvent;
use tokio::sync::broadcast;

/// Fan-out broadcaster that sends events to multiple subscribers using a
/// [`broadcast`] channel.
///
/// Late subscribers only receive events sent **after** they subscribe
/// (for replay semantics see [`crate::replay::ReplayBuffer`]).
#[derive(Debug)]
pub struct FanOut {
    tx: broadcast::Sender<AgentEvent>,
}

impl FanOut {
    /// Create a new fan-out with the given channel capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Add a subscriber. Returns a [`broadcast::Receiver`] that will receive
    /// all future broadcast events.
    pub fn add_subscriber(&self) -> broadcast::Receiver<AgentEvent> {
        self.tx.subscribe()
    }

    /// Broadcast an event to all current subscribers.
    ///
    /// Returns the number of subscribers that received the event.
    /// Returns 0 when there are no active subscribers.
    pub fn broadcast(&self, event: &AgentEvent) -> usize {
        // `send` returns Err only when there are zero receivers.
        self.tx.send(event.clone()).unwrap_or(0)
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
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
    fn fanout_single_subscriber() {
        let fan = FanOut::new(16);
        let mut rx = fan.add_subscriber();

        let ev = delta("hello");
        let n = fan.broadcast(&ev);
        assert_eq!(n, 1);

        let received = rx.try_recv().unwrap();
        assert!(
            matches!(&received.kind, AgentEventKind::AssistantDelta { text } if text == "hello")
        );
    }

    #[test]
    fn fanout_multiple_subscribers() {
        let fan = FanOut::new(16);
        let mut rx1 = fan.add_subscriber();
        let mut rx2 = fan.add_subscriber();
        let mut rx3 = fan.add_subscriber();

        assert_eq!(fan.subscriber_count(), 3);

        let ev = delta("multi");
        fan.broadcast(&ev);

        for rx in [&mut rx1, &mut rx2, &mut rx3] {
            let received = rx.try_recv().unwrap();
            assert!(
                matches!(&received.kind, AgentEventKind::AssistantDelta { text } if text == "multi")
            );
        }
    }

    #[test]
    fn fanout_no_subscribers() {
        let fan = FanOut::new(16);
        let n = fan.broadcast(&delta("nobody"));
        assert_eq!(n, 0);
    }

    #[test]
    fn fanout_dropped_subscriber() {
        let fan = FanOut::new(16);
        let rx = fan.add_subscriber();
        assert_eq!(fan.subscriber_count(), 1);
        drop(rx);
        assert_eq!(fan.subscriber_count(), 0);
        let n = fan.broadcast(&delta("gone"));
        assert_eq!(n, 0);
    }

    #[test]
    fn fanout_broadcast_sequence() {
        let fan = FanOut::new(16);
        let mut rx = fan.add_subscriber();

        for i in 0..5 {
            fan.broadcast(&delta(&format!("msg-{i}")));
        }

        for i in 0..5 {
            let received = rx.try_recv().unwrap();
            let expected = format!("msg-{i}");
            assert!(
                matches!(&received.kind, AgentEventKind::AssistantDelta { text } if text == &expected)
            );
        }
    }

    #[test]
    fn fanout_late_subscriber_misses_past() {
        let fan = FanOut::new(16);
        fan.broadcast(&delta("before"));

        let mut rx = fan.add_subscriber();
        fan.broadcast(&delta("after"));

        let received = rx.try_recv().unwrap();
        assert!(
            matches!(&received.kind, AgentEventKind::AssistantDelta { text } if text == "after")
        );
    }

    #[test]
    fn fanout_different_event_types() {
        let fan = FanOut::new(16);
        let mut rx = fan.add_subscriber();

        fan.broadcast(&delta("text"));
        fan.broadcast(&error_ev("oops"));

        let r1 = rx.try_recv().unwrap();
        let r2 = rx.try_recv().unwrap();
        assert!(matches!(r1.kind, AgentEventKind::AssistantDelta { .. }));
        assert!(matches!(r2.kind, AgentEventKind::Error { .. }));
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn fanout_zero_capacity_panics() {
        let _ = FanOut::new(0);
    }

    #[test]
    fn fanout_subscriber_count_tracks() {
        let fan = FanOut::new(16);
        assert_eq!(fan.subscriber_count(), 0);

        let rx1 = fan.add_subscriber();
        assert_eq!(fan.subscriber_count(), 1);

        let rx2 = fan.add_subscriber();
        assert_eq!(fan.subscriber_count(), 2);

        drop(rx1);
        assert_eq!(fan.subscriber_count(), 1);

        drop(rx2);
        assert_eq!(fan.subscriber_count(), 0);
    }
}
