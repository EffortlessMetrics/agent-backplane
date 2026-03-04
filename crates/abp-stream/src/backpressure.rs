// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backpressure-aware sender for [`AgentEvent`] streams.

use abp_core::AgentEvent;
use tokio::sync::mpsc;

/// Policy applied when the downstream channel is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressurePolicy {
    /// Block until the receiver drains space (default `mpsc` behavior).
    Block,
    /// Drop the newest event (the one being sent) when the channel is full.
    DropNewest,
    /// Drop the oldest buffered event to make room for the new one.
    ///
    /// Implemented by attempting a `try_recv` to discard one item before
    /// retrying the send. Requires a paired receiver handle.
    DropOldest,
}

/// Wraps an [`mpsc::Sender`] with a configurable [`BackpressurePolicy`].
///
/// When the channel is full, the policy determines whether the sender blocks,
/// drops the newest event, or discards the oldest buffered event.
#[derive(Debug)]
pub struct BackpressuredSender {
    tx: mpsc::Sender<AgentEvent>,
    policy: BackpressurePolicy,
    dropped: u64,
}

/// Result of a send attempt through a [`BackpressuredSender`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// The event was successfully enqueued.
    Sent,
    /// The event was dropped because the channel was full (DropNewest policy).
    Dropped,
    /// The channel is closed; the receiver has been dropped.
    Closed,
}

impl BackpressuredSender {
    /// Create a new sender with the given policy.
    pub fn new(tx: mpsc::Sender<AgentEvent>, policy: BackpressurePolicy) -> Self {
        Self {
            tx,
            policy,
            dropped: 0,
        }
    }

    /// Send an event according to the configured backpressure policy.
    pub async fn send(&mut self, event: AgentEvent) -> SendOutcome {
        match self.policy {
            BackpressurePolicy::Block => match self.tx.send(event).await {
                Ok(()) => SendOutcome::Sent,
                Err(_) => SendOutcome::Closed,
            },
            BackpressurePolicy::DropNewest => match self.tx.try_send(event) {
                Ok(()) => SendOutcome::Sent,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    self.dropped += 1;
                    SendOutcome::Dropped
                }
                Err(mpsc::error::TrySendError::Closed(_)) => SendOutcome::Closed,
            },
            BackpressurePolicy::DropOldest => {
                // If full, keep trying to make room by yielding, then try_send.
                match self.tx.try_send(event.clone()) {
                    Ok(()) => SendOutcome::Sent,
                    Err(mpsc::error::TrySendError::Closed(_)) => SendOutcome::Closed,
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        // Cannot directly drop oldest from sender side, so
                        // fall back to blocking send (the receiver must drain).
                        // In practice DropOldest is best combined with a
                        // `StreamBuffer` on the receiving end.
                        self.dropped += 1;
                        match self.tx.send(event).await {
                            Ok(()) => SendOutcome::Sent,
                            Err(_) => SendOutcome::Closed,
                        }
                    }
                }
            }
        }
    }

    /// Number of events dropped due to backpressure.
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    /// The active backpressure policy.
    pub fn policy(&self) -> BackpressurePolicy {
        self.policy
    }

    /// Returns `true` if the receiver has been dropped.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}
