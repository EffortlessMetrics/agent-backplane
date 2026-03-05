// SPDX-License-Identifier: MIT OR Apache-2.0
//! Middleware / interceptor pattern for sidecar event processing.
//!
//! All payloads are [`serde_json::Value`] to stay independent of `abp-core`
//! types, consistent with the rest of `sidecar-kit`.

use serde_json::Value;
use std::time::Instant;
use tracing::debug;

// ── Trait ────────────────────────────────────────────────────────────

/// A single processing step that may transform or suppress an event.
///
/// Returning `None` drops the event from the pipeline.
pub trait EventMiddleware: Send + Sync {
    /// Process an event, optionally transforming or dropping it.
    fn process(&self, event: &Value) -> Option<Value>;
}

// ── LoggingMiddleware ────────────────────────────────────────────────

/// Middleware that logs every event via [`tracing`] and passes it through
/// unchanged.
#[derive(Debug, Clone, Default)]
pub struct LoggingMiddleware;

impl LoggingMiddleware {
    /// Create a new `LoggingMiddleware`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl EventMiddleware for LoggingMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        debug!(target: "sidecar_kit.middleware", %event, "middleware saw event");
        Some(event.clone())
    }
}

// ── FilterMiddleware ─────────────────────────────────────────────────

/// Include/exclude filter operating on the `"type"` field of a JSON event
/// value, analogous to `abp-core`'s `EventFilter` but fully value-based.
#[derive(Debug, Clone)]
pub struct FilterMiddleware {
    mode: FilterMode,
    /// Stored lowercase for case-insensitive comparison.
    kinds: Vec<String>,
}

#[derive(Debug, Clone)]
enum FilterMode {
    Include,
    Exclude,
}

impl FilterMiddleware {
    /// Create a filter that only passes events whose `"type"` is in `kinds`.
    /// An empty list means nothing passes.
    #[must_use]
    pub fn include_kinds(kinds: &[&str]) -> Self {
        Self {
            mode: FilterMode::Include,
            kinds: kinds.iter().map(|k| k.to_ascii_lowercase()).collect(),
        }
    }

    /// Create a filter that passes everything *except* events whose `"type"`
    /// is in `kinds`. An empty list means everything passes.
    #[must_use]
    pub fn exclude_kinds(kinds: &[&str]) -> Self {
        Self {
            mode: FilterMode::Exclude,
            kinds: kinds.iter().map(|k| k.to_ascii_lowercase()).collect(),
        }
    }
}

impl EventMiddleware for FilterMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        let type_name = event
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();

        let in_set = self.kinds.contains(&type_name);
        let passes = match self.mode {
            FilterMode::Include => in_set,
            FilterMode::Exclude => !in_set,
        };

        if passes { Some(event.clone()) } else { None }
    }
}

// ── MiddlewareChain ──────────────────────────────────────────────────

/// Ordered chain of [`EventMiddleware`]s. Events flow through each
/// middleware in sequence; if any returns `None` the chain short-circuits.
pub struct MiddlewareChain {
    layers: Vec<Box<dyn EventMiddleware>>,
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareChain {
    /// Create an empty chain (acts as passthrough).
    #[must_use]
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Append a middleware to the end of the chain.
    pub fn push(&mut self, middleware: impl EventMiddleware + 'static) {
        self.layers.push(Box::new(middleware));
    }

    /// Convenience builder that appends a middleware and returns `self`.
    #[must_use]
    pub fn with(mut self, middleware: impl EventMiddleware + 'static) -> Self {
        self.push(middleware);
        self
    }

    /// Run `event` through every middleware in order.
    ///
    /// Returns `None` if any middleware drops the event.
    pub fn process(&self, event: &Value) -> Option<Value> {
        let mut current = event.clone();
        for layer in &self.layers {
            current = layer.process(&current)?;
        }
        Some(current)
    }

    /// Returns the number of middlewares in the chain.
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

// ── TimingMiddleware ─────────────────────────────────────────────────

/// Middleware that injects a `"_processing_us"` field recording how many
/// microseconds each event spent traversing this middleware.
///
/// Useful for diagnosing pipeline latency; the field is added to JSON
/// objects only and ignored for non-object events.
#[derive(Debug, Clone, Default)]
pub struct TimingMiddleware;

impl TimingMiddleware {
    /// Create a new `TimingMiddleware`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl EventMiddleware for TimingMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        let start = Instant::now();
        let mut out = event.clone();
        let elapsed = start.elapsed().as_micros() as u64;
        if let Some(obj) = out.as_object_mut() {
            obj.insert("_processing_us".to_string(), Value::Number(elapsed.into()));
        }
        Some(out)
    }
}

// ── ErrorWrapMiddleware ──────────────────────────────────────────────

/// Middleware that wraps any malformed event (non-object) into an error
/// event, ensuring downstream stages always receive JSON objects.
#[derive(Debug, Clone, Default)]
pub struct ErrorWrapMiddleware;

impl ErrorWrapMiddleware {
    /// Create a new `ErrorWrapMiddleware`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl EventMiddleware for ErrorWrapMiddleware {
    fn process(&self, event: &Value) -> Option<Value> {
        if event.is_object() {
            Some(event.clone())
        } else {
            Some(serde_json::json!({
                "type": "error",
                "message": format!("non-object event replaced: {event}"),
                "_original": event.clone(),
            }))
        }
    }
}
