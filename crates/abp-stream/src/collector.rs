// SPDX-License-Identifier: MIT OR Apache-2.0
//! [`EventCollector`] — collects events into a `Vec` while forwarding them through a stream.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use abp_core::AgentEvent;
use futures_core::Stream;
use pin_project_lite::pin_project;

pin_project! {
    /// A stream wrapper that records every event it yields into an internal
    /// buffer while transparently forwarding them to the consumer.
    pub struct EventCollector<S> {
        #[pin]
        inner: S,
        collected: Arc<Mutex<Vec<AgentEvent>>>,
    }
}

impl<S> EventCollector<S> {
    /// Wrap `inner` in a collector that records all yielded events.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            collected: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return a handle to the collected events (can be cloned and shared).
    pub fn collected(&self) -> Arc<Mutex<Vec<AgentEvent>>> {
        Arc::clone(&self.collected)
    }

    /// Snapshot the events collected so far.
    pub fn snapshot(&self) -> Vec<AgentEvent> {
        self.collected.lock().expect("collector lock").clone()
    }

    /// Number of events collected so far.
    pub fn len(&self) -> usize {
        self.collected.lock().expect("collector lock").len()
    }

    /// Whether no events have been collected.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<S: Stream<Item = AgentEvent>> Stream for EventCollector<S> {
    type Item = AgentEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match this.inner.poll_next(cx) {
            Poll::Ready(Some(event)) => {
                this.collected
                    .lock()
                    .expect("collector lock")
                    .push(event.clone());
                Poll::Ready(Some(event))
            }
            other => other,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
