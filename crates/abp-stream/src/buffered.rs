// SPDX-License-Identifier: MIT OR Apache-2.0
//! [`BufferedStream`] — buffers events and emits them in batches.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use abp_core::AgentEvent;
use futures_core::Stream;
use pin_project_lite::pin_project;
use tokio::time::{Sleep, sleep};

pin_project! {
    /// Buffers events from an inner stream and emits them in `Vec` batches.
    ///
    /// A batch is emitted when either:
    /// - `batch_size` events have accumulated, or
    /// - `flush_interval` has elapsed since the first buffered event (if set), or
    /// - the inner stream ends (any remaining buffered events are flushed).
    pub struct BufferedStream<S> {
        #[pin]
        inner: S,
        batch_size: usize,
        buf: Vec<AgentEvent>,
        done: bool,
        flush_interval: Option<Duration>,
        #[pin]
        flush_deadline: Sleep,
        deadline_active: bool,
    }
}

impl<S> BufferedStream<S> {
    /// Create a new buffered stream with the given batch size.
    pub fn new(inner: S, batch_size: usize) -> Self {
        assert!(batch_size > 0, "batch_size must be > 0");
        Self {
            inner,
            batch_size,
            buf: Vec::with_capacity(batch_size),
            done: false,
            flush_interval: None,
            flush_deadline: sleep(Duration::from_secs(86400)),
            deadline_active: false,
        }
    }

    /// Set an optional flush interval. If the buffer is non-empty and this
    /// duration elapses since the first buffered event, the batch is emitted
    /// even if `batch_size` hasn't been reached.
    pub fn with_flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = Some(interval);
        self
    }
}

impl<S: Stream<Item = AgentEvent>> Stream for BufferedStream<S> {
    type Item = Vec<AgentEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            if *this.done {
                // Inner exhausted — flush remaining.
                if this.buf.is_empty() {
                    return Poll::Ready(None);
                }
                let batch = std::mem::take(this.buf);
                return Poll::Ready(Some(batch));
            }

            // Try to fill the buffer from the inner stream.
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(event)) => {
                    // Start flush deadline on first buffered item.
                    if this.buf.is_empty() {
                        if let Some(interval) = this.flush_interval {
                            this.flush_deadline
                                .as_mut()
                                .reset(tokio::time::Instant::now() + *interval);
                            *this.deadline_active = true;
                        }
                    }
                    this.buf.push(event);
                    if this.buf.len() >= *this.batch_size {
                        *this.deadline_active = false;
                        let batch = std::mem::take(this.buf);
                        return Poll::Ready(Some(batch));
                    }
                    // Continue polling for more items.
                }
                Poll::Ready(None) => {
                    *this.done = true;
                    // Loop will flush remaining.
                }
                Poll::Pending => {
                    // Check flush deadline if active.
                    if *this.deadline_active && !this.buf.is_empty() {
                        if let Poll::Ready(()) = this.flush_deadline.as_mut().poll(cx) {
                            *this.deadline_active = false;
                            let batch = std::mem::take(this.buf);
                            return Poll::Ready(Some(batch));
                        }
                    }
                    return Poll::Pending;
                }
            }
        }
    }
}
