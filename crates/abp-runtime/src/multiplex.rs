// SPDX-License-Identifier: MIT OR Apache-2.0
//! Event multiplexer for broadcasting [`AgentEvent`]s to multiple consumers,
//! and an `EventRouter` for dispatching events by kind.

use abp_core::AgentEvent;
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::broadcast;

/// Errors produced by the multiplexer and subscriber operations.
#[derive(Debug, Error)]
pub enum MultiplexError {
    /// No subscribers are listening when a broadcast is attempted.
    #[error("no subscribers")]
    NoSubscribers,

    /// A subscriber fell behind and missed events.
    #[error("subscriber lagged, missed {missed} events")]
    Lagged {
        /// Number of events the subscriber missed.
        missed: u64,
    },

    /// The channel is closed.
    #[error("channel closed")]
    Closed,
}

/// Broadcasts [`AgentEvent`]s to multiple subscribers via `tokio::sync::broadcast`.
pub struct EventMultiplexer {
    tx: broadcast::Sender<AgentEvent>,
}

impl EventMultiplexer {
    /// Create a new multiplexer with the given channel capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Create a new [`EventSubscriber`] that receives future broadcasts.
    #[must_use]
    pub fn subscribe(&self) -> EventSubscriber {
        EventSubscriber {
            rx: self.tx.subscribe(),
        }
    }

    /// Broadcast an event to all current subscribers.
    ///
    /// Returns the number of receivers that will get the event.
    ///
    /// # Errors
    ///
    /// Returns [`MultiplexError::NoSubscribers`] if there are no active subscribers.
    pub fn broadcast(&self, event: AgentEvent) -> Result<usize, MultiplexError> {
        self.tx
            .send(event)
            .map_err(|_| MultiplexError::NoSubscribers)
    }

    /// Return the number of active subscribers.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// A handle for receiving events from an [`EventMultiplexer`].
pub struct EventSubscriber {
    rx: broadcast::Receiver<AgentEvent>,
}

impl EventSubscriber {
    /// Receive the next event, waiting asynchronously.
    ///
    /// # Errors
    ///
    /// Returns [`MultiplexError::Lagged`] if this subscriber fell behind, or
    /// [`MultiplexError::Closed`] if the multiplexer has been dropped.
    pub async fn recv(&mut self) -> Result<AgentEvent, MultiplexError> {
        match self.rx.recv().await {
            Ok(ev) => Ok(ev),
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                Err(MultiplexError::Lagged { missed })
            }
            Err(broadcast::error::RecvError::Closed) => Err(MultiplexError::Closed),
        }
    }

    /// Try to receive an event without waiting.
    ///
    /// Returns `None` if no event is available or the channel is closed/lagged.
    pub fn try_recv(&mut self) -> Option<AgentEvent> {
        self.rx.try_recv().ok()
    }
}

/// Extracts the serde-serialized `"type"` tag from an `AgentEventKind`.
fn event_kind_name(event: &AgentEvent) -> Option<String> {
    let value = serde_json::to_value(&event.kind).ok()?;
    value.get("type").and_then(|v| v.as_str()).map(String::from)
}

type EventHandler = Box<dyn Fn(&AgentEvent) + Send + Sync>;

/// Routes events to handlers registered for specific event kinds.
///
/// Kind strings match the serde `rename_all = "snake_case"` tag on
/// `AgentEventKind` (e.g. `"run_started"`, `"tool_call"`).
pub struct EventRouter {
    routes: HashMap<String, Vec<EventHandler>>,
}

impl EventRouter {
    /// Create an empty router with no handlers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    /// Register a handler for events whose kind matches `kind`.
    pub fn add_route(&mut self, kind: &str, handler: Box<dyn Fn(&AgentEvent) + Send + Sync>) {
        self.routes
            .entry(kind.to_string())
            .or_default()
            .push(handler);
    }

    /// Dispatch `event` to all handlers registered for its kind.
    pub fn route(&self, event: &AgentEvent) {
        if let Some(kind) = event_kind_name(event) {
            for handler in self.routes.get(&kind).into_iter().flatten() {
                handler(event);
            }
        }
    }

    /// Return the total number of registered routes (distinct kinds).
    #[must_use]
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}
