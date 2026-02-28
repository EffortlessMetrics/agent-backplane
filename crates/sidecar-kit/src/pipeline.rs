// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stage-based event processing pipeline with built-in stages.
//!
//! Unlike [`crate::middleware::MiddlewareChain`] which is a simple
//! pass-through/filter chain, [`EventPipeline`] provides richer error
//! reporting via [`PipelineError`] and ships with several ready-made stages.

use serde_json::Value;
use std::fmt;

// ── Error ────────────────────────────────────────────────────────────

/// Errors produced by the event pipeline.
#[derive(Debug)]
pub enum PipelineError {
    /// A named stage encountered an error.
    StageError {
        /// Name of the stage that failed.
        stage: String,
        /// Human-readable description of what went wrong.
        message: String,
    },
    /// The event value is not a valid JSON object.
    InvalidEvent,
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StageError { stage, message } => {
                write!(f, "stage '{stage}' failed: {message}")
            }
            Self::InvalidEvent => write!(f, "event is not a valid JSON object"),
        }
    }
}

impl std::error::Error for PipelineError {}

// ── Trait ────────────────────────────────────────────────────────────

/// A single stage in an [`EventPipeline`].
///
/// Returning `Ok(None)` filters the event out. Returning `Ok(Some(v))`
/// passes the (possibly transformed) event to the next stage.
pub trait PipelineStage: Send + Sync {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &str;

    /// Process one event.
    fn process(&self, event: Value) -> Result<Option<Value>, PipelineError>;
}

// ── EventPipeline ────────────────────────────────────────────────────

/// Ordered sequence of [`PipelineStage`]s that an event flows through.
pub struct EventPipeline {
    stages: Vec<Box<dyn PipelineStage + Send + Sync>>,
}

impl Default for EventPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl EventPipeline {
    /// Create an empty pipeline (acts as passthrough).
    #[must_use]
    pub fn new() -> Self {
        Self {
            stages: Vec::new(),
        }
    }

    /// Append a stage to the end of the pipeline.
    pub fn add_stage(&mut self, stage: Box<dyn PipelineStage + Send + Sync>) {
        self.stages.push(stage);
    }

    /// Run `event` through every stage in order.
    ///
    /// Returns `Ok(None)` if any stage filters the event out, or the final
    /// transformed event on success.
    pub fn process(&self, event: Value) -> Result<Option<Value>, PipelineError> {
        let mut current = event;
        for stage in &self.stages {
            match stage.process(current)? {
                Some(v) => current = v,
                None => return Ok(None),
            }
        }
        Ok(Some(current))
    }

    /// Returns the number of stages in the pipeline.
    #[must_use]
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }
}

// ── TimestampStage ───────────────────────────────────────────────────

/// Adds or overwrites a `processed_at` field with the current UTC timestamp.
#[derive(Debug, Clone, Default)]
pub struct TimestampStage;

impl TimestampStage {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl PipelineStage for TimestampStage {
    fn name(&self) -> &str {
        "timestamp"
    }

    fn process(&self, mut event: Value) -> Result<Option<Value>, PipelineError> {
        let obj = event
            .as_object_mut()
            .ok_or(PipelineError::InvalidEvent)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_millis();

        obj.insert(
            "processed_at".to_string(),
            Value::Number(serde_json::Number::from(now as u64)),
        );
        Ok(Some(event))
    }
}

// ── RedactStage ──────────────────────────────────────────────────────

/// Removes the specified top-level fields from every event.
#[derive(Debug, Clone)]
pub struct RedactStage {
    fields: Vec<String>,
}

impl RedactStage {
    /// Create a new `RedactStage` that removes the given fields.
    #[must_use]
    pub fn new(fields: Vec<String>) -> Self {
        Self { fields }
    }
}

impl PipelineStage for RedactStage {
    fn name(&self) -> &str {
        "redact"
    }

    fn process(&self, mut event: Value) -> Result<Option<Value>, PipelineError> {
        let obj = event
            .as_object_mut()
            .ok_or(PipelineError::InvalidEvent)?;

        for field in &self.fields {
            obj.remove(field);
        }
        Ok(Some(event))
    }
}

// ── ValidateStage ────────────────────────────────────────────────────

/// Checks that all specified fields are present in the event object.
#[derive(Debug, Clone)]
pub struct ValidateStage {
    required_fields: Vec<String>,
}

impl ValidateStage {
    /// Create a new `ValidateStage` that requires the given fields.
    #[must_use]
    pub fn new(required_fields: Vec<String>) -> Self {
        Self { required_fields }
    }
}

impl PipelineStage for ValidateStage {
    fn name(&self) -> &str {
        "validate"
    }

    fn process(&self, event: Value) -> Result<Option<Value>, PipelineError> {
        let obj = event.as_object().ok_or(PipelineError::InvalidEvent)?;

        for field in &self.required_fields {
            if !obj.contains_key(field) {
                return Err(PipelineError::StageError {
                    stage: self.name().to_string(),
                    message: format!("missing required field: {field}"),
                });
            }
        }
        Ok(Some(event))
    }
}
