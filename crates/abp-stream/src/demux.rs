// SPDX-License-Identifier: MIT OR Apache-2.0
//! Demultiplexer that routes events to different channels by type.
#![allow(dead_code, unused_imports)]

use abp_core::AgentEvent;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A route entry: a predicate and destination sender.
struct Route {
    predicate: Arc<dyn Fn(&AgentEvent) -> bool + Send + Sync>,
    sender: mpsc::Sender<AgentEvent>,
}

impl std::fmt::Debug for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Route")
            .field("sender_closed", &self.sender.is_closed())
            .finish_non_exhaustive()
    }
}

/// Routes events to different channels based on predicates.
///
/// Events are matched against routes in insertion order; the first matching
/// route receives the event. Unmatched events go to the default route if one
/// is configured.
#[derive(Debug)]
pub struct StreamDemux {
    routes: Vec<Route>,
    default: Option<mpsc::Sender<AgentEvent>>,
}

impl StreamDemux {
    /// Create a new demultiplexer with no routes.
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            default: None,
        }
    }

    /// Add a route: events matching `predicate` will be sent to `sender`.
    pub fn add_route<F>(&mut self, predicate: F, sender: mpsc::Sender<AgentEvent>)
    where
        F: Fn(&AgentEvent) -> bool + Send + Sync + 'static,
    {
        self.routes.push(Route {
            predicate: Arc::new(predicate),
            sender,
        });
    }

    /// Set a default route for events that don't match any predicate.
    pub fn set_default(&mut self, sender: mpsc::Sender<AgentEvent>) {
        self.default = Some(sender);
    }

    /// Route a single event. Returns `true` if the event was sent to some
    /// channel, `false` if it was unmatched and no default is set (or the
    /// target channel is closed).
    pub async fn route(&self, event: &AgentEvent) -> bool {
        for r in &self.routes {
            if (r.predicate)(event) {
                return r.sender.send(event.clone()).await.is_ok();
            }
        }
        if let Some(ref default) = self.default {
            return default.send(event.clone()).await.is_ok();
        }
        false
    }

    /// Run the demuxer, reading from a source and routing until the source
    /// closes.
    pub async fn run(&self, mut source: mpsc::Receiver<AgentEvent>) {
        while let Some(event) = source.recv().await {
            self.route(&event).await;
        }
    }

    /// Number of configured routes (excluding the default).
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// Whether a default route has been configured.
    pub fn has_default(&self) -> bool {
        self.default.is_some()
    }
}

impl Default for StreamDemux {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    fn delta_event(text: &str) -> AgentEvent {
        make_event(AgentEventKind::AssistantDelta {
            text: text.to_string(),
        })
    }

    fn tool_call_event(name: &str) -> AgentEvent {
        make_event(AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        })
    }

    fn error_event(msg: &str) -> AgentEvent {
        make_event(AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        })
    }

    fn is_text_event(ev: &AgentEvent) -> bool {
        matches!(
            ev.kind,
            AgentEventKind::AssistantDelta { .. } | AgentEventKind::AssistantMessage { .. }
        )
    }

    fn is_tool_event(ev: &AgentEvent) -> bool {
        matches!(
            ev.kind,
            AgentEventKind::ToolCall { .. } | AgentEventKind::ToolResult { .. }
        )
    }

    #[tokio::test]
    async fn routes_text_and_tool_events_separately() {
        let (text_tx, mut text_rx) = mpsc::channel(16);
        let (tool_tx, mut tool_rx) = mpsc::channel(16);

        let mut demux = StreamDemux::new();
        demux.add_route(is_text_event, text_tx);
        demux.add_route(is_tool_event, tool_tx);

        assert!(demux.route(&delta_event("hello")).await);
        assert!(demux.route(&tool_call_event("bash")).await);

        let text_ev = text_rx.recv().await.unwrap();
        assert!(
            matches!(&text_ev.kind, AgentEventKind::AssistantDelta { text } if text == "hello")
        );

        let tool_ev = tool_rx.recv().await.unwrap();
        assert!(
            matches!(&tool_ev.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "bash")
        );
    }

    #[tokio::test]
    async fn unmatched_event_goes_to_default() {
        let (text_tx, _text_rx) = mpsc::channel(16);
        let (default_tx, mut default_rx) = mpsc::channel(16);

        let mut demux = StreamDemux::new();
        demux.add_route(is_text_event, text_tx);
        demux.set_default(default_tx);

        // Error event doesn't match text route, goes to default
        assert!(demux.route(&error_event("oops")).await);

        let ev = default_rx.recv().await.unwrap();
        assert!(matches!(&ev.kind, AgentEventKind::Error { message, .. } if message == "oops"));
    }

    #[tokio::test]
    async fn unmatched_event_no_default_returns_false() {
        let (text_tx, _text_rx) = mpsc::channel(16);

        let mut demux = StreamDemux::new();
        demux.add_route(is_text_event, text_tx);

        // No default — unmatched event returns false
        assert!(!demux.route(&error_event("oops")).await);
    }

    #[tokio::test]
    async fn first_matching_route_wins() {
        let (first_tx, mut first_rx) = mpsc::channel(16);
        let (second_tx, mut second_rx) = mpsc::channel(16);

        let mut demux = StreamDemux::new();
        // Both routes match text events, but first should win
        demux.add_route(is_text_event, first_tx);
        demux.add_route(is_text_event, second_tx);

        assert!(demux.route(&delta_event("hi")).await);

        let ev = first_rx.recv().await.unwrap();
        assert!(matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text == "hi"));

        // Second route should not have received anything
        assert!(second_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn run_routes_until_source_closes() {
        let (source_tx, source_rx) = mpsc::channel(16);
        let (text_tx, mut text_rx) = mpsc::channel(16);
        let (default_tx, mut default_rx) = mpsc::channel(16);

        let mut demux = StreamDemux::new();
        demux.add_route(is_text_event, text_tx);
        demux.set_default(default_tx);

        let handle = tokio::spawn(async move {
            demux.run(source_rx).await;
        });

        source_tx.send(delta_event("a")).await.unwrap();
        source_tx.send(error_event("b")).await.unwrap();
        source_tx.send(delta_event("c")).await.unwrap();
        drop(source_tx);

        handle.await.unwrap();

        let mut texts = Vec::new();
        while let Ok(ev) = text_rx.try_recv() {
            texts.push(ev);
        }
        let mut defaults = Vec::new();
        while let Ok(ev) = default_rx.try_recv() {
            defaults.push(ev);
        }

        assert_eq!(texts.len(), 2);
        assert_eq!(defaults.len(), 1);
    }

    #[tokio::test]
    async fn closed_route_returns_false() {
        let (text_tx, text_rx) = mpsc::channel(16);

        let mut demux = StreamDemux::new();
        demux.add_route(is_text_event, text_tx);

        // Drop receiver to close the channel
        drop(text_rx);

        assert!(!demux.route(&delta_event("dropped")).await);
    }

    #[tokio::test]
    async fn route_count_and_has_default() {
        let mut demux = StreamDemux::new();
        assert_eq!(demux.route_count(), 0);
        assert!(!demux.has_default());

        let (tx1, _rx1) = mpsc::channel(16);
        let (tx2, _rx2) = mpsc::channel(16);
        let (dtx, _drx) = mpsc::channel(16);

        demux.add_route(is_text_event, tx1);
        demux.add_route(is_tool_event, tx2);
        demux.set_default(dtx);

        assert_eq!(demux.route_count(), 2);
        assert!(demux.has_default());
    }
}
