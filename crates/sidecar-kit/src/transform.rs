// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reusable event transformers for the sidecar event middleware system.
//!
//! Provides the [`EventTransformer`] trait and built-in implementations for
//! common event processing tasks: redaction, throttling, enrichment,
//! filtering, and timestamp management.

use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

// ── Trait ────────────────────────────────────────────────────────────

/// A single transformation step that may transform or suppress an event.
///
/// Returning `None` filters the event out of the pipeline.
pub trait EventTransformer: Send + Sync {
    /// Transform an event, optionally filtering it out.
    fn transform(&self, event: AgentEvent) -> Option<AgentEvent>;

    /// Human-readable name for diagnostics.
    fn name(&self) -> &str;
}

// ── helpers ──────────────────────────────────────────────────────────

/// Extract the snake_case name of an [`AgentEventKind`] variant.
fn kind_name(kind: &AgentEventKind) -> &'static str {
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

// ── RedactTransformer ────────────────────────────────────────────────

/// Redacts sensitive patterns (API keys, passwords) from text content.
///
/// Replaces all occurrences of each pattern with `[REDACTED]`.
pub struct RedactTransformer {
    patterns: Vec<String>,
}

impl RedactTransformer {
    /// Create a new `RedactTransformer` with the given literal patterns.
    #[must_use]
    pub fn new(patterns: Vec<String>) -> Self {
        Self { patterns }
    }

    fn redact_string(&self, s: &str) -> String {
        let mut result = s.to_string();
        for pattern in &self.patterns {
            if !pattern.is_empty() {
                result = result.replace(pattern.as_str(), "[REDACTED]");
            }
        }
        result
    }

    fn redact_value(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                *s = self.redact_string(s);
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    self.redact_value(item);
                }
            }
            serde_json::Value::Object(obj) => {
                for (_, v) in obj {
                    self.redact_value(v);
                }
            }
            _ => {}
        }
    }
}

impl EventTransformer for RedactTransformer {
    fn name(&self) -> &str {
        "redact"
    }

    fn transform(&self, mut event: AgentEvent) -> Option<AgentEvent> {
        event.kind = match event.kind {
            AgentEventKind::RunStarted { message } => {
                AgentEventKind::RunStarted { message: self.redact_string(&message) }
            }
            AgentEventKind::RunCompleted { message } => {
                AgentEventKind::RunCompleted { message: self.redact_string(&message) }
            }
            AgentEventKind::AssistantDelta { text } => {
                AgentEventKind::AssistantDelta { text: self.redact_string(&text) }
            }
            AgentEventKind::AssistantMessage { text } => {
                AgentEventKind::AssistantMessage { text: self.redact_string(&text) }
            }
            AgentEventKind::ToolCall { tool_name, tool_use_id, parent_tool_use_id, mut input } => {
                self.redact_value(&mut input);
                AgentEventKind::ToolCall { tool_name, tool_use_id, parent_tool_use_id, input }
            }
            AgentEventKind::ToolResult { tool_name, tool_use_id, mut output, is_error } => {
                self.redact_value(&mut output);
                AgentEventKind::ToolResult { tool_name, tool_use_id, output, is_error }
            }
            AgentEventKind::FileChanged { path, summary } => {
                AgentEventKind::FileChanged { path, summary: self.redact_string(&summary) }
            }
            AgentEventKind::CommandExecuted { command, exit_code, output_preview } => {
                AgentEventKind::CommandExecuted {
                    command: self.redact_string(&command),
                    exit_code,
                    output_preview: output_preview.map(|s| self.redact_string(&s)),
                }
            }
            AgentEventKind::Warning { message } => {
                AgentEventKind::Warning { message: self.redact_string(&message) }
            }
            AgentEventKind::Error { message } => {
                AgentEventKind::Error { message: self.redact_string(&message) }
            }
        };
        Some(event)
    }
}

// ── ThrottleTransformer ──────────────────────────────────────────────

/// Rate-limits events by kind, filtering out events once the maximum
/// count per kind has been reached.
pub struct ThrottleTransformer {
    max_per_kind: usize,
    counts: Mutex<HashMap<String, usize>>,
}

impl ThrottleTransformer {
    /// Create a new `ThrottleTransformer` that allows at most `max_per_kind`
    /// events of each kind.
    #[must_use]
    pub fn new(max_per_kind: usize) -> Self {
        Self {
            max_per_kind,
            counts: Mutex::new(HashMap::new()),
        }
    }
}

impl EventTransformer for ThrottleTransformer {
    fn name(&self) -> &str {
        "throttle"
    }

    fn transform(&self, event: AgentEvent) -> Option<AgentEvent> {
        let kind = kind_name(&event.kind).to_string();
        let mut counts = self.counts.lock().unwrap();
        let count = counts.entry(kind).or_insert(0);
        *count += 1;
        if *count > self.max_per_kind {
            None
        } else {
            Some(event)
        }
    }
}

// ── EnrichTransformer ────────────────────────────────────────────────

/// Adds metadata key-value pairs to every event's `ext` field.
pub struct EnrichTransformer {
    metadata: BTreeMap<String, String>,
}

impl EnrichTransformer {
    /// Create a new `EnrichTransformer` with the given metadata.
    #[must_use]
    pub fn new(metadata: BTreeMap<String, String>) -> Self {
        Self { metadata }
    }
}

impl EventTransformer for EnrichTransformer {
    fn name(&self) -> &str {
        "enrich"
    }

    fn transform(&self, mut event: AgentEvent) -> Option<AgentEvent> {
        let ext = event.ext.get_or_insert_with(BTreeMap::new);
        for (k, v) in &self.metadata {
            ext.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        Some(event)
    }
}

// ── FilterTransformer ────────────────────────────────────────────────

/// Filters events using a user-supplied predicate.
///
/// Events for which the predicate returns `false` are dropped.
pub struct FilterTransformer {
    predicate: Box<dyn Fn(&AgentEvent) -> bool + Send + Sync>,
}

impl FilterTransformer {
    /// Create a new `FilterTransformer` with the given predicate.
    pub fn new(predicate: Box<dyn Fn(&AgentEvent) -> bool + Send + Sync>) -> Self {
        Self { predicate }
    }
}

impl EventTransformer for FilterTransformer {
    fn name(&self) -> &str {
        "filter"
    }

    fn transform(&self, event: AgentEvent) -> Option<AgentEvent> {
        if (self.predicate)(&event) {
            Some(event)
        } else {
            None
        }
    }
}

// ── TimestampTransformer ─────────────────────────────────────────────

/// Ensures all events have a current timestamp.
///
/// If an event's `ts` field is at or before the Unix epoch, it is
/// replaced with the current UTC time.
#[derive(Debug, Clone, Default)]
pub struct TimestampTransformer;

impl TimestampTransformer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl EventTransformer for TimestampTransformer {
    fn name(&self) -> &str {
        "timestamp"
    }

    fn transform(&self, mut event: AgentEvent) -> Option<AgentEvent> {
        if event.ts.timestamp() <= 0 {
            event.ts = Utc::now();
        }
        Some(event)
    }
}

// ── TransformerChain ─────────────────────────────────────────────────

/// Ordered chain of [`EventTransformer`]s. Events flow through each
/// transformer in sequence; if any returns `None` the chain short-circuits.
pub struct TransformerChain {
    transformers: Vec<Box<dyn EventTransformer>>,
}

impl Default for TransformerChain {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformerChain {
    /// Create an empty chain (acts as passthrough).
    #[must_use]
    pub fn new() -> Self {
        Self {
            transformers: Vec::new(),
        }
    }

    /// Append a transformer and return `self` for chaining.
    #[must_use]
    pub fn with(mut self, transformer: Box<dyn EventTransformer>) -> Self {
        self.transformers.push(transformer);
        self
    }

    /// Process a single event through all transformers in order.
    ///
    /// Returns `None` if any transformer drops the event.
    pub fn process(&self, event: AgentEvent) -> Option<AgentEvent> {
        let mut current = event;
        for transformer in &self.transformers {
            current = transformer.transform(current)?;
        }
        Some(current)
    }

    /// Process a batch of events, returning only those that survive
    /// all transformers.
    pub fn process_batch(&self, events: Vec<AgentEvent>) -> Vec<AgentEvent> {
        events.into_iter().filter_map(|e| self.process(e)).collect()
    }
}
