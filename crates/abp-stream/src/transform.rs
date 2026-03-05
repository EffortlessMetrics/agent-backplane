// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stream transformers: [`MapStream`], [`FilterStream`], [`TakeUntilStream`],
//! [`ThrottleStream`], and [`BatchStream`].

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use abp_core::AgentEvent;
use futures_core::Stream;
use pin_project_lite::pin_project;
use tokio::time::{sleep, Instant, Sleep};

// ---------------------------------------------------------------------------
// MapStream
// ---------------------------------------------------------------------------

pin_project! {
    /// Applies a function to each event yielded by the inner stream.
    pub struct MapStream<S, F> {
        #[pin]
        inner: S,
        f: F,
    }
}

impl<S, F> MapStream<S, F> {
    /// Wrap `inner` with a mapping function.
    pub fn new(inner: S, f: F) -> Self {
        Self { inner, f }
    }
}

impl<S, F> Stream for MapStream<S, F>
where
    S: Stream<Item = AgentEvent>,
    F: FnMut(AgentEvent) -> AgentEvent,
{
    type Item = AgentEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match this.inner.poll_next(cx) {
            Poll::Ready(Some(ev)) => Poll::Ready(Some((this.f)(ev))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

// ---------------------------------------------------------------------------
// FilterStream
// ---------------------------------------------------------------------------

pin_project! {
    /// Drops events that do not satisfy the predicate.
    pub struct FilterStream<S, P> {
        #[pin]
        inner: S,
        predicate: P,
    }
}

impl<S, P> FilterStream<S, P> {
    /// Wrap `inner` with a filtering predicate.
    pub fn new(inner: S, predicate: P) -> Self {
        Self { inner, predicate }
    }
}

impl<S, P> Stream for FilterStream<S, P>
where
    S: Stream<Item = AgentEvent>,
    P: FnMut(&AgentEvent) -> bool,
{
    type Item = AgentEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(ev)) => {
                    if (this.predicate)(&ev) {
                        return Poll::Ready(Some(ev));
                    }
                    // Drop and poll again.
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TakeUntilStream
// ---------------------------------------------------------------------------

pin_project! {
    /// Yields events until the condition returns `true`, then terminates.
    ///
    /// The event that triggers the condition is **not** yielded.
    pub struct TakeUntilStream<S, C> {
        #[pin]
        inner: S,
        condition: C,
        done: bool,
    }
}

impl<S, C> TakeUntilStream<S, C> {
    /// Wrap `inner`; the stream ends when `condition` returns `true`.
    pub fn new(inner: S, condition: C) -> Self {
        Self {
            inner,
            condition,
            done: false,
        }
    }
}

impl<S, C> Stream for TakeUntilStream<S, C>
where
    S: Stream<Item = AgentEvent>,
    C: FnMut(&AgentEvent) -> bool,
{
    type Item = AgentEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if *this.done {
            return Poll::Ready(None);
        }
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(ev)) => {
                if (this.condition)(&ev) {
                    *this.done = true;
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(ev))
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

// ---------------------------------------------------------------------------
// ThrottleStream
// ---------------------------------------------------------------------------

pin_project! {
    /// Rate-limits events to at most one per `interval`.
    ///
    /// When an event arrives before the interval has elapsed it is dropped.
    pub struct ThrottleStream<S> {
        #[pin]
        inner: S,
        interval: Duration,
        #[pin]
        delay: Sleep,
        ready: bool,
    }
}

impl<S> ThrottleStream<S> {
    /// Wrap `inner` with a minimum inter-event `interval`.
    pub fn new(inner: S, interval: Duration) -> Self {
        Self {
            inner,
            interval,
            delay: sleep(Duration::ZERO),
            ready: true,
        }
    }
}

impl<S: Stream<Item = AgentEvent>> Stream for ThrottleStream<S> {
    type Item = AgentEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // If we are waiting for the throttle interval, check the timer.
        if !*this.ready {
            match this.delay.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    *this.ready = true;
                }
                Poll::Pending => {
                    // Still throttled — drain inner to keep it polled but drop events.
                    loop {
                        match this.inner.as_mut().poll_next(cx) {
                            Poll::Ready(Some(_)) => {
                                // drop the event
                            }
                            Poll::Ready(None) => return Poll::Ready(None),
                            Poll::Pending => return Poll::Pending,
                        }
                    }
                }
            }
        }

        // Ready to emit.
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(ev)) => {
                *this.ready = false;
                this.delay.as_mut().reset(Instant::now() + *this.interval);
                Poll::Ready(Some(ev))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

// ---------------------------------------------------------------------------
// BatchStream
// ---------------------------------------------------------------------------

pin_project! {
    /// Collects up to `batch_size` events before emitting them as a `Vec`.
    ///
    /// Any remaining events are flushed when the inner stream ends.
    pub struct BatchStream<S> {
        #[pin]
        inner: S,
        batch_size: usize,
        buf: Vec<AgentEvent>,
        done: bool,
    }
}

impl<S> BatchStream<S> {
    /// Create a new batch stream that emits batches of `batch_size`.
    pub fn new(inner: S, batch_size: usize) -> Self {
        assert!(batch_size > 0, "batch_size must be > 0");
        Self {
            inner,
            batch_size,
            buf: Vec::with_capacity(batch_size),
            done: false,
        }
    }
}

impl<S: Stream<Item = AgentEvent>> Stream for BatchStream<S> {
    type Item = Vec<AgentEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            if *this.done {
                if this.buf.is_empty() {
                    return Poll::Ready(None);
                }
                let batch = std::mem::take(this.buf);
                return Poll::Ready(Some(batch));
            }

            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(ev)) => {
                    this.buf.push(ev);
                    if this.buf.len() >= *this.batch_size {
                        let batch = std::mem::take(this.buf);
                        return Poll::Ready(Some(batch));
                    }
                }
                Poll::Ready(None) => {
                    *this.done = true;
                    // Loop will flush remaining.
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEventKind;
    use chrono::Utc;
    use tokio_stream::StreamExt;

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    fn delta(text: &str) -> AgentEvent {
        make_event(AgentEventKind::AssistantDelta {
            text: text.to_string(),
        })
    }

    fn error_ev(msg: &str) -> AgentEvent {
        make_event(AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        })
    }

    fn run_started() -> AgentEvent {
        make_event(AgentEventKind::RunStarted {
            message: "go".to_string(),
        })
    }

    fn run_completed() -> AgentEvent {
        make_event(AgentEventKind::RunCompleted {
            message: "done".to_string(),
        })
    }

    // -- MapStream tests --

    #[tokio::test]
    async fn map_stream_transforms_events() {
        let src = tokio_stream::iter(vec![delta("a"), delta("b")]);
        let mapped = MapStream::new(src, |mut ev: AgentEvent| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = text.to_uppercase();
            }
            ev
        });
        let out: Vec<_> = mapped.collect().await;
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0].kind, AgentEventKind::AssistantDelta { text } if text == "A"));
        assert!(matches!(&out[1].kind, AgentEventKind::AssistantDelta { text } if text == "B"));
    }

    #[tokio::test]
    async fn map_stream_empty() {
        let src = tokio_stream::iter(Vec::<AgentEvent>::new());
        let mapped = MapStream::new(src, |ev: AgentEvent| ev);
        let out: Vec<_> = mapped.collect().await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn map_stream_preserves_order() {
        let events: Vec<_> = (0..10).map(|i| delta(&format!("{i}"))).collect();
        let src = tokio_stream::iter(events);
        let mapped = MapStream::new(src, |ev: AgentEvent| ev);
        let out: Vec<_> = mapped.collect().await;
        assert_eq!(out.len(), 10);
    }

    #[tokio::test]
    async fn map_stream_size_hint() {
        let src = tokio_stream::iter(vec![delta("a"), delta("b"), delta("c")]);
        let mapped = MapStream::new(src, |ev: AgentEvent| ev);
        let (lo, hi) = mapped.size_hint();
        assert_eq!(lo, 3);
        assert_eq!(hi, Some(3));
    }

    // -- FilterStream tests --

    #[tokio::test]
    async fn filter_stream_keeps_matching() {
        let src = tokio_stream::iter(vec![delta("keep"), error_ev("drop"), delta("keep2")]);
        let filtered = FilterStream::new(src, |ev: &AgentEvent| {
            matches!(ev.kind, AgentEventKind::AssistantDelta { .. })
        });
        let out: Vec<_> = filtered.collect().await;
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0].kind, AgentEventKind::AssistantDelta { text } if text == "keep"));
        assert!(matches!(&out[1].kind, AgentEventKind::AssistantDelta { text } if text == "keep2"));
    }

    #[tokio::test]
    async fn filter_stream_drops_all() {
        let src = tokio_stream::iter(vec![delta("a"), delta("b")]);
        let filtered = FilterStream::new(src, |_: &AgentEvent| false);
        let out: Vec<_> = filtered.collect().await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn filter_stream_keeps_all() {
        let src = tokio_stream::iter(vec![delta("a"), delta("b"), delta("c")]);
        let filtered = FilterStream::new(src, |_: &AgentEvent| true);
        let out: Vec<_> = filtered.collect().await;
        assert_eq!(out.len(), 3);
    }

    #[tokio::test]
    async fn filter_stream_empty() {
        let src = tokio_stream::iter(Vec::<AgentEvent>::new());
        let filtered = FilterStream::new(src, |_: &AgentEvent| true);
        let out: Vec<_> = filtered.collect().await;
        assert!(out.is_empty());
    }

    // -- TakeUntilStream tests --

    #[tokio::test]
    async fn take_until_stops_on_condition() {
        let src = tokio_stream::iter(vec![delta("a"), error_ev("stop"), delta("c")]);
        let stream = TakeUntilStream::new(src, |ev: &AgentEvent| {
            matches!(ev.kind, AgentEventKind::Error { .. })
        });
        let out: Vec<_> = stream.collect().await;
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0].kind, AgentEventKind::AssistantDelta { text } if text == "a"));
    }

    #[tokio::test]
    async fn take_until_no_trigger() {
        let src = tokio_stream::iter(vec![delta("a"), delta("b")]);
        let stream = TakeUntilStream::new(src, |ev: &AgentEvent| {
            matches!(ev.kind, AgentEventKind::Error { .. })
        });
        let out: Vec<_> = stream.collect().await;
        assert_eq!(out.len(), 2);
    }

    #[tokio::test]
    async fn take_until_immediate_trigger() {
        let src = tokio_stream::iter(vec![error_ev("stop"), delta("a")]);
        let stream = TakeUntilStream::new(src, |ev: &AgentEvent| {
            matches!(ev.kind, AgentEventKind::Error { .. })
        });
        let out: Vec<_> = stream.collect().await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn take_until_empty() {
        let src = tokio_stream::iter(Vec::<AgentEvent>::new());
        let stream = TakeUntilStream::new(src, |_: &AgentEvent| true);
        let out: Vec<_> = stream.collect().await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn take_until_run_completed() {
        let events = vec![
            run_started(),
            delta("hello"),
            delta("world"),
            run_completed(),
            delta("after"),
        ];
        let stream = TakeUntilStream::new(tokio_stream::iter(events), |ev: &AgentEvent| {
            matches!(ev.kind, AgentEventKind::RunCompleted { .. })
        });
        let out: Vec<_> = stream.collect().await;
        assert_eq!(out.len(), 3);
    }

    // -- ThrottleStream tests --

    #[tokio::test]
    async fn throttle_emits_first_immediately() {
        let src = tokio_stream::iter(vec![delta("first")]);
        let stream = ThrottleStream::new(src, Duration::from_secs(10));
        let out: Vec<_> = stream.collect().await;
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0].kind, AgentEventKind::AssistantDelta { text } if text == "first"));
    }

    #[tokio::test]
    async fn throttle_empty_stream() {
        let src = tokio_stream::iter(Vec::<AgentEvent>::new());
        let stream = ThrottleStream::new(src, Duration::from_millis(10));
        let out: Vec<_> = stream.collect().await;
        assert!(out.is_empty());
    }

    // -- BatchStream tests --

    #[tokio::test]
    async fn batch_stream_exact_batches() {
        let events: Vec<_> = (0..6).map(|i| delta(&format!("{i}"))).collect();
        let stream = BatchStream::new(tokio_stream::iter(events), 3);
        let batches: Vec<_> = stream.collect().await;
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[1].len(), 3);
    }

    #[tokio::test]
    async fn batch_stream_remainder() {
        let events: Vec<_> = (0..5).map(|i| delta(&format!("{i}"))).collect();
        let stream = BatchStream::new(tokio_stream::iter(events), 3);
        let batches: Vec<_> = stream.collect().await;
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[1].len(), 2);
    }

    #[tokio::test]
    async fn batch_stream_single_element() {
        let stream = BatchStream::new(tokio_stream::iter(vec![delta("x")]), 5);
        let batches: Vec<_> = stream.collect().await;
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 1);
    }

    #[tokio::test]
    async fn batch_stream_empty() {
        let stream = BatchStream::new(tokio_stream::iter(Vec::<AgentEvent>::new()), 3);
        let batches: Vec<_> = stream.collect().await;
        assert!(batches.is_empty());
    }

    #[tokio::test]
    async fn batch_stream_size_one() {
        let events: Vec<_> = (0..3).map(|i| delta(&format!("{i}"))).collect();
        let stream = BatchStream::new(tokio_stream::iter(events), 1);
        let batches: Vec<_> = stream.collect().await;
        assert_eq!(batches.len(), 3);
        for batch in &batches {
            assert_eq!(batch.len(), 1);
        }
    }

    #[test]
    #[should_panic(expected = "batch_size must be > 0")]
    fn batch_stream_zero_size_panics() {
        let _ = BatchStream::new(tokio_stream::iter(Vec::<AgentEvent>::new()), 0);
    }
}
