// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming multiplexer for fan-out event distribution.
#![allow(dead_code, unused_imports)]

use abp_core::AgentEvent;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

/// Unique identifier for a subscriber.
pub type SubscriberId = u64;

/// A streaming multiplexer that fans out events from a single source to
/// multiple subscribers.
///
/// Each subscriber gets its own bounded channel. When a subscriber's channel
/// is full (backpressure), events are dropped for that subscriber up to the
/// configured buffer limit.
#[derive(Debug)]
pub struct StreamMultiplexer {
    subscribers: Arc<Mutex<HashMap<SubscriberId, mpsc::Sender<AgentEvent>>>>,
    next_id: AtomicU64,
    buffer_size: usize,
}

impl StreamMultiplexer {
    /// Create a new multiplexer. `buffer_size` controls the per-subscriber
    /// channel capacity.
    pub fn new(buffer_size: usize) -> Self {
        assert!(buffer_size > 0, "buffer_size must be > 0");
        Self {
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            buffer_size,
        }
    }

    /// Add a new subscriber, returning its ID and a receiver for events.
    pub async fn subscribe(&self) -> (SubscriberId, mpsc::Receiver<AgentEvent>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(self.buffer_size);
        self.subscribers.lock().await.insert(id, tx);
        (id, rx)
    }

    /// Remove a subscriber by ID. Returns `true` if the subscriber existed.
    pub async fn unsubscribe(&self, id: SubscriberId) -> bool {
        self.subscribers.lock().await.remove(&id).is_some()
    }

    /// Broadcast an event to all active subscribers.
    ///
    /// Uses `try_send` to avoid blocking on slow subscribers — if a
    /// subscriber's buffer is full, the event is dropped for that subscriber.
    /// Closed subscribers are automatically removed.
    pub async fn broadcast(&self, event: &AgentEvent) {
        let mut subs = self.subscribers.lock().await;
        let mut closed = Vec::new();
        for (&id, tx) in subs.iter() {
            match tx.try_send(event.clone()) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // Backpressure: drop event for this slow subscriber
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    closed.push(id);
                }
            }
        }
        for id in closed {
            subs.remove(&id);
        }
    }

    /// Number of currently active subscribers.
    pub async fn subscriber_count(&self) -> usize {
        self.subscribers.lock().await.len()
    }

    /// Graceful shutdown: close all subscriber channels and drain.
    ///
    /// After this call, all subscriber receivers will see the channel close.
    pub async fn shutdown(self) {
        let subs = self.subscribers.lock().await;
        // Dropping all senders closes the channels.
        drop(subs);
    }

    /// Run the multiplexer, reading from the given source receiver and
    /// broadcasting each event to all subscribers. Returns when the source
    /// is closed, after draining all remaining events.
    pub async fn run(&self, mut source: mpsc::Receiver<AgentEvent>) {
        while let Some(event) = source.recv().await {
            self.broadcast(&event).await;
        }
        // Source closed — drop all subscriber senders for graceful close
        self.subscribers.lock().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_event(text: &str) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind: abp_core::AgentEventKind::AssistantDelta {
                text: text.to_string(),
            },
            ext: None,
        }
    }

    fn error_event(msg: &str) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind: abp_core::AgentEventKind::Error {
                message: msg.to_string(),
                error_code: None,
            },
            ext: None,
        }
    }

    #[tokio::test]
    async fn fan_out_to_multiple_subscribers() {
        let mux = StreamMultiplexer::new(16);

        let (_id1, mut rx1) = mux.subscribe().await;
        let (_id2, mut rx2) = mux.subscribe().await;
        let (_id3, mut rx3) = mux.subscribe().await;

        assert_eq!(mux.subscriber_count().await, 3);

        let ev = make_event("hello");
        mux.broadcast(&ev).await;

        let r1 = rx1.recv().await.unwrap();
        let r2 = rx2.recv().await.unwrap();
        let r3 = rx3.recv().await.unwrap();

        assert!(
            matches!(&r1.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "hello")
        );
        assert!(
            matches!(&r2.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "hello")
        );
        assert!(
            matches!(&r3.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "hello")
        );
    }

    #[tokio::test]
    async fn subscriber_removal_during_stream() {
        let mux = StreamMultiplexer::new(16);

        let (id1, mut rx1) = mux.subscribe().await;
        let (_id2, mut rx2) = mux.subscribe().await;

        mux.broadcast(&make_event("first")).await;

        // Remove subscriber 1
        assert!(mux.unsubscribe(id1).await);
        assert_eq!(mux.subscriber_count().await, 1);

        mux.broadcast(&make_event("second")).await;

        // rx1 got only the first event
        let e1 = rx1.recv().await.unwrap();
        assert!(
            matches!(&e1.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "first")
        );
        // Channel closed after unsubscribe + no more sends
        assert!(rx1.try_recv().is_err());

        // rx2 got both
        let e2a = rx2.recv().await.unwrap();
        let e2b = rx2.recv().await.unwrap();
        assert!(
            matches!(&e2a.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "first")
        );
        assert!(
            matches!(&e2b.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "second")
        );
    }

    #[tokio::test]
    async fn unsubscribe_nonexistent_returns_false() {
        let mux = StreamMultiplexer::new(16);
        assert!(!mux.unsubscribe(999).await);
    }

    #[tokio::test]
    async fn backpressure_drops_events_for_slow_subscriber() {
        // Buffer size of 2: subscriber can hold at most 2 events
        let mux = StreamMultiplexer::new(2);

        let (_id1, mut rx1) = mux.subscribe().await;

        // Send 5 events without reading — only 2 fit in the buffer
        for i in 0..5 {
            mux.broadcast(&make_event(&format!("msg-{i}"))).await;
        }

        // Should receive the first 2 (buffer was full for the rest)
        let e1 = rx1.recv().await.unwrap();
        let e2 = rx1.recv().await.unwrap();
        assert!(
            matches!(&e1.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "msg-0")
        );
        assert!(
            matches!(&e2.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "msg-1")
        );
    }

    #[tokio::test]
    async fn closed_subscriber_is_auto_removed() {
        let mux = StreamMultiplexer::new(16);

        let (_id1, rx1) = mux.subscribe().await;
        let (_id2, mut rx2) = mux.subscribe().await;
        assert_eq!(mux.subscriber_count().await, 2);

        // Drop rx1 to close its channel
        drop(rx1);

        mux.broadcast(&make_event("after-close")).await;

        // Closed subscriber should be auto-removed
        assert_eq!(mux.subscriber_count().await, 1);

        let ev = rx2.recv().await.unwrap();
        assert!(
            matches!(&ev.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "after-close")
        );
    }

    #[tokio::test]
    async fn run_broadcasts_and_closes_on_source_end() {
        let mux = Arc::new(StreamMultiplexer::new(16));
        let (source_tx, source_rx) = mpsc::channel(16);

        let (_id1, mut rx1) = mux.subscribe().await;
        let (_id2, mut rx2) = mux.subscribe().await;

        let mux_clone = Arc::clone(&mux);
        let handle = tokio::spawn(async move {
            mux_clone.run(source_rx).await;
        });

        source_tx.send(make_event("a")).await.unwrap();
        source_tx.send(make_event("b")).await.unwrap();
        drop(source_tx); // close source

        handle.await.unwrap();

        let mut events1 = Vec::new();
        while let Ok(ev) = rx1.try_recv() {
            events1.push(ev);
        }
        let mut events2 = Vec::new();
        while let Ok(ev) = rx2.try_recv() {
            events2.push(ev);
        }

        assert_eq!(events1.len(), 2);
        assert_eq!(events2.len(), 2);
    }

    #[tokio::test]
    async fn concurrent_subscribe_and_broadcast() {
        let mux = Arc::new(StreamMultiplexer::new(64));

        // Spawn multiple tasks subscribing concurrently
        let mut handles = Vec::new();
        for i in 0..10 {
            let mux = Arc::clone(&mux);
            handles.push(tokio::spawn(async move {
                let (_id, mut rx) = mux.subscribe().await;
                // Wait for one event
                let ev = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await;
                (i, ev)
            }));
        }

        // Small delay to let all subscribers register
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(mux.subscriber_count().await, 10);
        mux.broadcast(&make_event("concurrent")).await;

        for handle in handles {
            let (_, result) = handle.await.unwrap();
            let ev = result.unwrap().unwrap();
            assert!(
                matches!(&ev.kind, abp_core::AgentEventKind::AssistantDelta { text } if text == "concurrent")
            );
        }
    }

    #[tokio::test]
    async fn graceful_shutdown_closes_all_receivers() {
        let mux = StreamMultiplexer::new(16);

        let (_id1, mut rx1) = mux.subscribe().await;
        let (_id2, mut rx2) = mux.subscribe().await;

        mux.shutdown().await;

        // After shutdown, receivers should eventually get None
        assert!(rx1.recv().await.is_none());
        assert!(rx2.recv().await.is_none());
    }
}
