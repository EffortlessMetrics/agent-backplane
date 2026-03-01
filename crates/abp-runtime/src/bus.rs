// SPDX-License-Identifier: MIT OR Apache-2.0
//! Broadcast-based event bus for decoupled [`AgentEvent`] distribution.
//!
//! Unlike [`super::multiplex::EventMultiplexer`], the event bus tracks
//! publishing statistics and supports filtered subscriptions.

use abp_core::AgentEvent;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

/// Default channel capacity for the event bus.
const DEFAULT_CAPACITY: usize = 256;

/// Shared statistics counters for an [`EventBus`].
#[derive(Debug, Default)]
struct StatsInner {
    total_published: AtomicU64,
    dropped_events: AtomicU64,
}

/// Broadcast-based event bus for distributing [`AgentEvent`]s to multiple
/// subscribers with built-in statistics tracking.
pub struct EventBus {
    tx: broadcast::Sender<AgentEvent>,
    stats: Arc<StatsInner>,
}

impl EventBus {
    /// Create a new event bus with the default channel capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Create a new event bus with the given channel capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            stats: Arc::new(StatsInner::default()),
        }
    }

    /// Create a new [`EventSubscription`] that receives future events.
    #[must_use]
    pub fn subscribe(&self) -> EventSubscription {
        EventSubscription {
            rx: self.tx.subscribe(),
            stats: Arc::clone(&self.stats),
        }
    }

    /// Publish an event to all current subscribers.
    ///
    /// If no subscribers are listening the event is silently dropped and
    /// counted in [`EventBusStats::dropped_events`].
    pub fn publish(&self, event: AgentEvent) {
        self.stats.total_published.fetch_add(1, Ordering::Relaxed);
        if self.tx.send(event).is_err() {
            self.stats.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Return the number of active subscribers.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Return a snapshot of the current bus statistics.
    #[must_use]
    pub fn stats(&self) -> EventBusStats {
        EventBusStats {
            total_published: self.stats.total_published.load(Ordering::Relaxed),
            active_subscribers: self.tx.receiver_count(),
            dropped_events: self.stats.dropped_events.load(Ordering::Relaxed),
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// A handle for receiving events from an [`EventBus`].
pub struct EventSubscription {
    rx: broadcast::Receiver<AgentEvent>,
    stats: Arc<StatsInner>,
}

/// Receive the next event asynchronously.
///
/// Returns `None` when the bus is dropped (channel closed) or
/// the subscriber has lagged behind. Lagged events are counted
/// in [`EventBusStats::dropped_events`].
impl EventSubscription {
    /// Receive the next event, waiting asynchronously.
    ///
    /// Returns `None` when the bus is closed or the subscriber lagged.
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        loop {
            match self.rx.recv().await {
                Ok(ev) => return Some(ev),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    self.stats.dropped_events.fetch_add(n, Ordering::Relaxed);
                    // Continue to try reading the next available event.
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Try to receive an event without blocking.
    ///
    /// Returns `None` if no event is available, the channel is closed,
    /// or the subscriber lagged.
    pub fn try_recv(&mut self) -> Option<AgentEvent> {
        match self.rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                self.stats.dropped_events.fetch_add(n, Ordering::Relaxed);
                // After recovering from lag, try once more.
                self.rx.try_recv().ok()
            }
            Err(_) => None,
        }
    }
}

/// Wraps an [`EventSubscription`] with a predicate filter so that only
/// matching events are yielded.
pub struct FilteredSubscription {
    inner: EventSubscription,
    filter: Box<dyn Fn(&AgentEvent) -> bool + Send + Sync>,
}

impl FilteredSubscription {
    /// Create a filtered subscription that only yields events where
    /// `filter` returns `true`.
    pub fn new(
        sub: EventSubscription,
        filter: Box<dyn Fn(&AgentEvent) -> bool + Send + Sync>,
    ) -> Self {
        Self { inner: sub, filter }
    }

    /// Receive the next matching event, waiting asynchronously.
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        loop {
            match self.inner.recv().await {
                Some(ev) if (self.filter)(&ev) => return Some(ev),
                Some(_) => continue,
                None => return None,
            }
        }
    }

    /// Try to receive the next matching event without blocking.
    pub fn try_recv(&mut self) -> Option<AgentEvent> {
        loop {
            match self.inner.try_recv() {
                Some(ev) if (self.filter)(&ev) => return Some(ev),
                Some(_) => continue,
                None => return None,
            }
        }
    }
}

/// Snapshot of event bus statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventBusStats {
    /// Total number of events published to the bus.
    pub total_published: u64,
    /// Number of subscribers currently listening.
    pub active_subscribers: usize,
    /// Events lost because no subscribers were listening or a subscriber lagged.
    pub dropped_events: u64,
}
