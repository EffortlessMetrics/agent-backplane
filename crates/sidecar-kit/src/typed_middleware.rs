// SPDX-License-Identifier: MIT OR Apache-2.0
//! Typed middleware system for sidecar event processing.
//!
//! Unlike the value-based [`crate::middleware`] module, this module operates
//! on strongly-typed [`AgentEvent`] values from `abp-core`.

use abp_core::{AgentEvent, AgentEventKind};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

// ── Action ──────────────────────────────────────────────────────────

/// Action returned by a [`SidecarMiddleware`] to control event flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiddlewareAction {
    /// Continue processing with the (possibly mutated) event.
    Continue,
    /// Skip this event — do not pass it downstream.
    Skip,
    /// An error occurred while processing this event.
    Error(String),
}

// ── Trait ────────────────────────────────────────────────────────────

/// A single typed processing step that may inspect, transform, or suppress
/// an [`AgentEvent`].
///
/// Implementations are `Send + Sync` so chains can be shared across threads.
pub trait SidecarMiddleware: Send + Sync {
    /// Process an event, optionally mutating it in place.
    fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction;
}

// ── Chain ────────────────────────────────────────────────────────────

/// Ordered chain of [`SidecarMiddleware`]s.
///
/// Events flow through each middleware in order. If any returns [`MiddlewareAction::Skip`]
/// or [`MiddlewareAction::Error`] the chain short-circuits.
pub struct SidecarMiddlewareChain {
    layers: Vec<Box<dyn SidecarMiddleware>>,
}

impl Default for SidecarMiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl SidecarMiddlewareChain {
    /// Create an empty chain (acts as passthrough).
    #[must_use]
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Append a middleware to the end of the chain.
    pub fn push(&mut self, middleware: impl SidecarMiddleware + 'static) {
        self.layers.push(Box::new(middleware));
    }

    /// Builder method — appends a middleware and returns `self`.
    #[must_use]
    pub fn with(mut self, middleware: impl SidecarMiddleware + 'static) -> Self {
        self.push(middleware);
        self
    }

    /// Run `event` through every middleware in order.
    pub fn process(&self, event: &mut AgentEvent) -> MiddlewareAction {
        for layer in &self.layers {
            let action = layer.on_event(event);
            match action {
                MiddlewareAction::Continue => {}
                other => return other,
            }
        }
        MiddlewareAction::Continue
    }

    /// Number of middlewares in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Returns `true` if the chain contains no middlewares.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Return a static string label for an [`AgentEventKind`] variant.
fn event_kind_name(kind: &AgentEventKind) -> &'static str {
    match kind {
        AgentEventKind::RunStarted { .. } => "run_started",
        AgentEventKind::RunCompleted { .. } => "run_completed",
        AgentEventKind::AssistantDelta { .. } => "assistant_delta",
        AgentEventKind::AssistantMessage { .. } => "assistant_message",
        AgentEventKind::ToolCall { .. } => "tool_call",
        AgentEventKind::ToolResult { .. } => "tool_result",
        AgentEventKind::FileChanged { .. } => "file_changed",
        AgentEventKind::CommandExecuted { .. } => "command_executed",
        AgentEventKind::Warning { .. } => "warning",
        AgentEventKind::Error { .. } => "error",
    }
}

// ── LoggingMiddleware ───────────────────────────────────────────────

/// Traces every event via [`tracing`] at debug level and passes it through.
#[derive(Debug, Clone, Default)]
pub struct LoggingMiddleware;

impl LoggingMiddleware {
    /// Create a new `LoggingMiddleware`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl SidecarMiddleware for LoggingMiddleware {
    fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
        debug!(
            target: "sidecar_kit.typed_middleware",
            kind = event_kind_name(&event.kind),
            "typed middleware saw event"
        );
        MiddlewareAction::Continue
    }
}

// ── MetricsMiddleware ───────────────────────────────────────────────

/// Counts events by kind and tracks per-event processing duration.
///
/// All counters are behind a [`Mutex`] for thread-safe interior mutability.
pub struct MetricsMiddleware {
    counts: Mutex<HashMap<String, u64>>,
    timings: Mutex<Vec<Duration>>,
}

impl Default for MetricsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsMiddleware {
    /// Create a new `MetricsMiddleware` with zeroed counters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
            timings: Mutex::new(Vec::new()),
        }
    }

    /// Snapshot of event counts by kind.
    #[must_use]
    pub fn counts(&self) -> HashMap<String, u64> {
        self.counts.lock().unwrap().clone()
    }

    /// Snapshot of recorded per-event timings.
    #[must_use]
    pub fn timings(&self) -> Vec<Duration> {
        self.timings.lock().unwrap().clone()
    }

    /// Total number of events observed.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.counts.lock().unwrap().values().sum()
    }
}

impl SidecarMiddleware for MetricsMiddleware {
    fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
        let start = Instant::now();
        let name = event_kind_name(&event.kind).to_string();
        *self.counts.lock().unwrap().entry(name).or_insert(0) += 1;
        self.timings.lock().unwrap().push(start.elapsed());
        MiddlewareAction::Continue
    }
}

// ── FilterMiddleware ────────────────────────────────────────────────

/// Drops events for which the predicate returns `true`.
pub struct FilterMiddleware {
    predicate: Box<dyn Fn(&AgentEvent) -> bool + Send + Sync>,
}

impl FilterMiddleware {
    /// Create a filter that **drops** events matching `predicate`.
    pub fn new(predicate: impl Fn(&AgentEvent) -> bool + Send + Sync + 'static) -> Self {
        Self {
            predicate: Box::new(predicate),
        }
    }
}

impl SidecarMiddleware for FilterMiddleware {
    fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
        if (self.predicate)(event) {
            MiddlewareAction::Skip
        } else {
            MiddlewareAction::Continue
        }
    }
}

// ── RateLimitMiddleware ─────────────────────────────────────────────

struct RateLimitState {
    /// Timestamps of events in the current window.
    window: Vec<Instant>,
}

/// Limits events to at most `max_per_second` per wall-clock second.
///
/// Events exceeding the limit receive [`MiddlewareAction::Skip`].
pub struct RateLimitMiddleware {
    max_per_second: u32,
    state: Mutex<RateLimitState>,
}

impl RateLimitMiddleware {
    /// Create a rate limiter allowing `max_per_second` events per second.
    #[must_use]
    pub fn new(max_per_second: u32) -> Self {
        Self {
            max_per_second,
            state: Mutex::new(RateLimitState { window: Vec::new() }),
        }
    }
}

impl SidecarMiddleware for RateLimitMiddleware {
    fn on_event(&self, _event: &mut AgentEvent) -> MiddlewareAction {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let cutoff = now - Duration::from_secs(1);
        state.window.retain(|t| *t > cutoff);
        if state.window.len() >= self.max_per_second as usize {
            MiddlewareAction::Skip
        } else {
            state.window.push(now);
            MiddlewareAction::Continue
        }
    }
}

// ── ErrorRecoveryMiddleware ─────────────────────────────────────────

/// Wraps an inner [`SidecarMiddleware`] and catches panics, converting
/// them into [`MiddlewareAction::Error`].
pub struct ErrorRecoveryMiddleware {
    inner: Box<dyn SidecarMiddleware>,
}

impl ErrorRecoveryMiddleware {
    /// Wrap `inner` so that any panic it produces becomes an error action.
    pub fn wrap(inner: impl SidecarMiddleware + 'static) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }
}

impl SidecarMiddleware for ErrorRecoveryMiddleware {
    fn on_event(&self, event: &mut AgentEvent) -> MiddlewareAction {
        let result = catch_unwind(AssertUnwindSafe(|| self.inner.on_event(event)));
        match result {
            Ok(action) => action,
            Err(payload) => {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                warn!(
                    target: "sidecar_kit.typed_middleware",
                    error = %msg,
                    "middleware panic recovered"
                );
                MiddlewareAction::Error(msg)
            }
        }
    }
}
