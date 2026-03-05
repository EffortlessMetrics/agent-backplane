// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fan-out one event stream to multiple consumers.

use abp_core::AgentEvent;
use tokio::sync::mpsc;

/// Broadcasts events to multiple consumers via [`mpsc::Sender`] channels.
///
/// Closed receivers are silently skipped during send operations.
#[derive(Debug)]
pub struct StreamTee {
    senders: Vec<mpsc::Sender<AgentEvent>>,
}

impl StreamTee {
    /// Create a new tee that will broadcast to the given senders.
    #[must_use]
    pub fn new(senders: Vec<mpsc::Sender<AgentEvent>>) -> Self {
        Self { senders }
    }

    /// Broadcast an event to all active senders.
    ///
    /// Returns `Ok(())` if at least one sender accepted the event, or if
    /// there are no senders. Returns `Err(TeeError::AllReceiversClosed)`
    /// only when every sender has been closed.
    pub async fn send(&self, event: &AgentEvent) -> Result<(), TeeError> {
        if self.senders.is_empty() {
            return Ok(());
        }

        let mut sent = 0usize;
        for tx in &self.senders {
            if tx.send(event.clone()).await.is_ok() {
                sent += 1;
            }
        }

        if sent > 0 {
            Ok(())
        } else {
            Err(TeeError::AllReceiversClosed)
        }
    }

    /// Number of senders whose receivers have not been dropped.
    pub fn active_count(&self) -> usize {
        self.senders.iter().filter(|tx| !tx.is_closed()).count()
    }
}

/// Errors from [`StreamTee`] operations.
#[derive(Debug)]
pub enum TeeError {
    /// All downstream receivers have been closed.
    AllReceiversClosed,
}

impl std::fmt::Display for TeeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllReceiversClosed => write!(f, "all receivers closed"),
        }
    }
}

impl std::error::Error for TeeError {}
