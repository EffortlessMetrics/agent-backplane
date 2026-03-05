// SPDX-License-Identifier: MIT OR Apache-2.0
//! [`TimeoutStream`] — wraps a stream with per-item timeout.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use abp_core::AgentEvent;
use futures_core::Stream;
use pin_project_lite::pin_project;
use tokio::time::{sleep, Sleep};

pin_project! {
    /// Wraps an event stream and yields a timeout error if no item arrives
    /// within `timeout` duration for each successive poll cycle.
    pub struct TimeoutStream<S> {
        #[pin]
        inner: S,
        timeout: Duration,
        #[pin]
        deadline: Sleep,
    }
}

/// Result type yielded by [`TimeoutStream`].
pub type TimeoutItem = Result<AgentEvent, StreamTimeout>;

/// Error indicating that a per-item timeout elapsed.
#[derive(Debug, Clone)]
pub struct StreamTimeout {
    /// The timeout duration that was exceeded.
    pub duration: Duration,
}

impl std::fmt::Display for StreamTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stream timeout after {:?}", self.duration)
    }
}

impl std::error::Error for StreamTimeout {}

impl<S> TimeoutStream<S> {
    /// Wrap `inner` with a per-item `timeout`.
    pub fn new(inner: S, timeout: Duration) -> Self {
        Self {
            inner,
            timeout,
            deadline: sleep(timeout),
        }
    }
}

impl<S: Stream<Item = AgentEvent>> Stream for TimeoutStream<S> {
    type Item = TimeoutItem;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // Check the inner stream first.
        match this.inner.poll_next(cx) {
            Poll::Ready(Some(item)) => {
                // Reset deadline for the next item.
                this.deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + *this.timeout);
                return Poll::Ready(Some(Ok(item)));
            }
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => {}
        }

        // Inner is pending — check timeout.
        match this.deadline.as_mut().poll(cx) {
            Poll::Ready(()) => {
                // Timeout fired. Reset for next call and yield error.
                this.deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + *this.timeout);
                Poll::Ready(Some(Err(StreamTimeout {
                    duration: *this.timeout,
                })))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
