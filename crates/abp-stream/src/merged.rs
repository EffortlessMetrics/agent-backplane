// SPDX-License-Identifier: MIT OR Apache-2.0
//! [`MergedStream`] — merges multiple event streams for parallel tool execution.

use std::pin::Pin;
use std::task::{Context, Poll};

use abp_core::AgentEvent;
use futures_core::Stream;

/// Merges multiple boxed event streams into a single stream, polling them
/// round-robin so that events from any source are interleaved fairly.
pub struct MergedStream {
    streams: Vec<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>>,
    /// Index of the next stream to poll (round-robin).
    next: usize,
}

impl MergedStream {
    /// Create a merged stream from a vec of boxed streams.
    pub fn new(streams: Vec<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>>) -> Self {
        Self { streams, next: 0 }
    }

    /// Number of sub-streams that have not yet completed.
    pub fn remaining(&self) -> usize {
        self.streams.len()
    }
}

impl Stream for MergedStream {
    type Item = AgentEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.streams.is_empty() {
            return Poll::Ready(None);
        }

        let len = self.streams.len();
        let start = self.next % len;

        // Try each stream starting at `start`, wrapping around.
        for offset in 0..len {
            let idx = (start + offset) % len;
            match self.streams[idx].as_mut().poll_next(cx) {
                Poll::Ready(Some(item)) => {
                    self.next = idx + 1;
                    return Poll::Ready(Some(item));
                }
                Poll::Ready(None) => {
                    // Stream exhausted — remove it.
                    let _ = self.streams.remove(idx);
                    if self.streams.is_empty() {
                        return Poll::Ready(None);
                    }
                    // Adjust next index after removal.
                    if self.next > idx {
                        self.next = self.next.saturating_sub(1);
                    }
                    self.next %= self.streams.len();
                    // Restart scan since indices shifted.
                    return self.poll_next(cx);
                }
                Poll::Pending => {}
            }
        }

        // All streams are pending.
        self.next = (start + 1) % self.streams.len();
        Poll::Pending
    }
}
