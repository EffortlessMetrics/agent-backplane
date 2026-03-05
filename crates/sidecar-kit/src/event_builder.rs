// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rich event builders for ALL [`AgentEventKind`] variants.
//!
//! Each builder validates required fields before producing an [`AgentEvent`],
//! returning `Result<AgentEvent, EventBuildError>` so callers get clear
//! diagnostics when a field is missing.
//!
//! # Example
//! ```
//! use sidecar_kit::event_builder::{TextDeltaBuilder, ToolCallBuilder};
//! use serde_json::json;
//!
//! let delta = TextDeltaBuilder::new("streaming chunk").build().unwrap();
//! let tc = ToolCallBuilder::new("read_file")
//!     .input(json!({"path": "a.rs"}))
//!     .tool_use_id("tc-1")
//!     .build()
//!     .unwrap();
//! ```

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde_json::Value;

use abp_core::{AgentEvent, AgentEventKind};

/// Error returned when a builder is missing a required field.
#[derive(Debug, Clone)]
pub struct EventBuildError {
    /// Name of the builder that failed.
    pub builder: &'static str,
    /// The missing or invalid field.
    pub field: String,
}

impl fmt::Display for EventBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} builder: missing required field '{}'",
            self.builder, self.field
        )
    }
}

impl std::error::Error for EventBuildError {}

// ── shared helpers ──────────────────────────────────────────────────

fn finish(
    kind: AgentEventKind,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
) -> AgentEvent {
    AgentEvent {
        ts: ts.unwrap_or_else(Utc::now),
        kind,
        ext,
    }
}

// ── TextDeltaBuilder ────────────────────────────────────────────────

/// Builder for `AssistantDelta` events.
#[derive(Debug, Clone)]
pub struct TextDeltaBuilder {
    text: String,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl TextDeltaBuilder {
    /// Create a builder with the required text fragment.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.text.is_empty() {
            return Err(EventBuildError {
                builder: "TextDelta",
                field: "text".into(),
            });
        }
        Ok(finish(
            AgentEventKind::AssistantDelta { text: self.text },
            self.ts,
            self.ext,
        ))
    }
}

// ── ToolCallBuilder ─────────────────────────────────────────────────

/// Builder for `ToolCall` events.
#[derive(Debug, Clone)]
pub struct ToolCallBuilder {
    tool_name: String,
    tool_use_id: Option<String>,
    parent_tool_use_id: Option<String>,
    input: Option<Value>,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl ToolCallBuilder {
    /// Create a builder with the required tool name.
    #[must_use]
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: None,
            ts: None,
            ext: None,
        }
    }

    /// Set the tool-use identifier.
    #[must_use]
    pub fn tool_use_id(mut self, id: impl Into<String>) -> Self {
        self.tool_use_id = Some(id.into());
        self
    }

    /// Set the parent tool-use identifier for nested calls.
    #[must_use]
    pub fn parent_tool_use_id(mut self, id: impl Into<String>) -> Self {
        self.parent_tool_use_id = Some(id.into());
        self
    }

    /// Set the JSON input for the tool.
    #[must_use]
    pub fn input(mut self, input: Value) -> Self {
        self.input = Some(input);
        self
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.tool_name.is_empty() {
            return Err(EventBuildError {
                builder: "ToolCall",
                field: "tool_name".into(),
            });
        }
        let input = self.input.unwrap_or(Value::Object(Default::default()));
        Ok(finish(
            AgentEventKind::ToolCall {
                tool_name: self.tool_name,
                tool_use_id: self.tool_use_id,
                parent_tool_use_id: self.parent_tool_use_id,
                input,
            },
            self.ts,
            self.ext,
        ))
    }
}

// ── ToolResultBuilder ───────────────────────────────────────────────

/// Builder for `ToolResult` events.
#[derive(Debug, Clone)]
pub struct ToolResultBuilder {
    tool_name: String,
    tool_use_id: Option<String>,
    output: Option<Value>,
    is_error: bool,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl ToolResultBuilder {
    /// Create a builder with the required tool name.
    #[must_use]
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            tool_use_id: None,
            output: None,
            is_error: false,
            ts: None,
            ext: None,
        }
    }

    /// Set the tool-use identifier.
    #[must_use]
    pub fn tool_use_id(mut self, id: impl Into<String>) -> Self {
        self.tool_use_id = Some(id.into());
        self
    }

    /// Set the JSON output from the tool.
    #[must_use]
    pub fn output(mut self, output: Value) -> Self {
        self.output = Some(output);
        self
    }

    /// Mark this result as an error result.
    #[must_use]
    pub fn is_error(mut self, err: bool) -> Self {
        self.is_error = err;
        self
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.tool_name.is_empty() {
            return Err(EventBuildError {
                builder: "ToolResult",
                field: "tool_name".into(),
            });
        }
        let output = self.output.unwrap_or(Value::Null);
        Ok(finish(
            AgentEventKind::ToolResult {
                tool_name: self.tool_name,
                tool_use_id: self.tool_use_id,
                output,
                is_error: self.is_error,
            },
            self.ts,
            self.ext,
        ))
    }
}

// ── FileEditBuilder ─────────────────────────────────────────────────

/// Builder for `FileChanged` events.
#[derive(Debug, Clone)]
pub struct FileEditBuilder {
    path: String,
    summary: Option<String>,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl FileEditBuilder {
    /// Create a builder with the required file path.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            summary: None,
            ts: None,
            ext: None,
        }
    }

    /// Set the human-readable change summary.
    #[must_use]
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.path.is_empty() {
            return Err(EventBuildError {
                builder: "FileEdit",
                field: "path".into(),
            });
        }
        Ok(finish(
            AgentEventKind::FileChanged {
                path: self.path,
                summary: self.summary.unwrap_or_default(),
            },
            self.ts,
            self.ext,
        ))
    }
}

// ── CommandRunBuilder ───────────────────────────────────────────────

/// Builder for `CommandExecuted` events.
#[derive(Debug, Clone)]
pub struct CommandRunBuilder {
    command: String,
    exit_code: Option<i32>,
    output_preview: Option<String>,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl CommandRunBuilder {
    /// Create a builder with the required command string.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            exit_code: None,
            output_preview: None,
            ts: None,
            ext: None,
        }
    }

    /// Set the exit code.
    #[must_use]
    pub fn exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Set a truncated output preview.
    #[must_use]
    pub fn output_preview(mut self, preview: impl Into<String>) -> Self {
        self.output_preview = Some(preview.into());
        self
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.command.is_empty() {
            return Err(EventBuildError {
                builder: "CommandRun",
                field: "command".into(),
            });
        }
        Ok(finish(
            AgentEventKind::CommandExecuted {
                command: self.command,
                exit_code: self.exit_code,
                output_preview: self.output_preview,
            },
            self.ts,
            self.ext,
        ))
    }
}

// ── ThinkingBuilder ─────────────────────────────────────────────────

/// Builder for `AssistantMessage` events used as "thinking" markers.
///
/// ABP v0.1 does not have a dedicated thinking variant; this emits an
/// `AssistantMessage` with the thinking text plus an `"_thinking": true`
/// extension so consumers can distinguish it from ordinary messages.
#[derive(Debug, Clone)]
pub struct ThinkingBuilder {
    text: String,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl ThinkingBuilder {
    /// Create a builder with the thinking text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(mut self) -> Result<AgentEvent, EventBuildError> {
        if self.text.is_empty() {
            return Err(EventBuildError {
                builder: "Thinking",
                field: "text".into(),
            });
        }
        // Tag as thinking via extension
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert("_thinking".into(), Value::Bool(true));
        Ok(finish(
            AgentEventKind::AssistantMessage { text: self.text },
            self.ts,
            self.ext,
        ))
    }
}

// ── UsageBuilder ────────────────────────────────────────────────────

/// Builder for a synthetic `RunCompleted` event that carries usage data
/// in the extension map.
///
/// ABP v0.1 does not have a dedicated usage event; this emits a
/// `RunCompleted` carrying normalised token counts in its `ext`.
#[derive(Debug, Clone)]
pub struct UsageBuilder {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    model: Option<String>,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl UsageBuilder {
    /// Create a new usage builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            model: None,
            ts: None,
            ext: None,
        }
    }

    /// Set input token count.
    #[must_use]
    pub fn input_tokens(mut self, n: u64) -> Self {
        self.input_tokens = Some(n);
        self
    }

    /// Set output token count.
    #[must_use]
    pub fn output_tokens(mut self, n: u64) -> Self {
        self.output_tokens = Some(n);
        self
    }

    /// Set the model name.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(mut self) -> Result<AgentEvent, EventBuildError> {
        if self.input_tokens.is_none() && self.output_tokens.is_none() {
            return Err(EventBuildError {
                builder: "Usage",
                field: "input_tokens or output_tokens".into(),
            });
        }
        let ext = self.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("_usage".into(), Value::Bool(true));
        if let Some(n) = self.input_tokens {
            ext.insert("input_tokens".into(), Value::Number(n.into()));
        }
        if let Some(n) = self.output_tokens {
            ext.insert("output_tokens".into(), Value::Number(n.into()));
        }
        if let Some(m) = &self.model {
            ext.insert("model".into(), Value::String(m.clone()));
        }
        Ok(finish(
            AgentEventKind::RunCompleted {
                message: "usage report".into(),
            },
            self.ts,
            self.ext,
        ))
    }
}

impl Default for UsageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ── ErrorBuilder ────────────────────────────────────────────────────

/// Builder for `Error` events.
#[derive(Debug, Clone)]
pub struct ErrorBuilder {
    message: String,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl ErrorBuilder {
    /// Create a builder with the required error message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.message.is_empty() {
            return Err(EventBuildError {
                builder: "Error",
                field: "message".into(),
            });
        }
        Ok(finish(
            AgentEventKind::Error {
                message: self.message,
                error_code: None,
            },
            self.ts,
            self.ext,
        ))
    }
}

// ── WarningBuilder ──────────────────────────────────────────────────

/// Builder for `Warning` events.
#[derive(Debug, Clone)]
pub struct WarningBuilder {
    message: String,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl WarningBuilder {
    /// Create a builder with the required warning message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.message.is_empty() {
            return Err(EventBuildError {
                builder: "Warning",
                field: "message".into(),
            });
        }
        Ok(finish(
            AgentEventKind::Warning {
                message: self.message,
            },
            self.ts,
            self.ext,
        ))
    }
}

// ── RunStartedBuilder / RunCompletedBuilder ─────────────────────────

/// Builder for `RunStarted` events.
#[derive(Debug, Clone)]
pub struct RunStartedBuilder {
    message: String,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl RunStartedBuilder {
    /// Create a builder with the required message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.message.is_empty() {
            return Err(EventBuildError {
                builder: "RunStarted",
                field: "message".into(),
            });
        }
        Ok(finish(
            AgentEventKind::RunStarted {
                message: self.message,
            },
            self.ts,
            self.ext,
        ))
    }
}

/// Builder for `RunCompleted` events.
#[derive(Debug, Clone)]
pub struct RunCompletedBuilder {
    message: String,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl RunCompletedBuilder {
    /// Create a builder with the required message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.message.is_empty() {
            return Err(EventBuildError {
                builder: "RunCompleted",
                field: "message".into(),
            });
        }
        Ok(finish(
            AgentEventKind::RunCompleted {
                message: self.message,
            },
            self.ts,
            self.ext,
        ))
    }
}

/// Builder for `AssistantMessage` events.
#[derive(Debug, Clone)]
pub struct TextMessageBuilder {
    text: String,
    ts: Option<DateTime<Utc>>,
    ext: Option<BTreeMap<String, Value>>,
}

impl TextMessageBuilder {
    /// Create a builder with the required text.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp.
    #[must_use]
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Validate and build the event.
    pub fn build(self) -> Result<AgentEvent, EventBuildError> {
        if self.text.is_empty() {
            return Err(EventBuildError {
                builder: "TextMessage",
                field: "text".into(),
            });
        }
        Ok(finish(
            AgentEventKind::AssistantMessage { text: self.text },
            self.ts,
            self.ext,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── TextDelta ───────────────────────────────────────────────────

    #[test]
    fn text_delta_ok() {
        let e = TextDeltaBuilder::new("hello").build().unwrap();
        assert!(matches!(e.kind, AgentEventKind::AssistantDelta { ref text } if text == "hello"));
    }

    #[test]
    fn text_delta_empty_fails() {
        let err = TextDeltaBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "text");
    }

    #[test]
    fn text_delta_with_ext() {
        let e = TextDeltaBuilder::new("hi")
            .ext("raw", json!(42))
            .build()
            .unwrap();
        assert!(e.ext.unwrap().contains_key("raw"));
    }

    // ── ToolCall ────────────────────────────────────────────────────

    #[test]
    fn tool_call_ok() {
        let e = ToolCallBuilder::new("read_file")
            .input(json!({"path": "a.rs"}))
            .tool_use_id("tc-1")
            .build()
            .unwrap();
        match &e.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
                assert_eq!(input["path"], "a.rs");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn tool_call_empty_name_fails() {
        let err = ToolCallBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "tool_name");
    }

    #[test]
    fn tool_call_defaults_input_to_empty_object() {
        let e = ToolCallBuilder::new("noop").build().unwrap();
        match &e.kind {
            AgentEventKind::ToolCall { input, .. } => assert!(input.is_object()),
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn tool_call_with_parent() {
        let e = ToolCallBuilder::new("sub_tool")
            .parent_tool_use_id("parent-1")
            .build()
            .unwrap();
        match &e.kind {
            AgentEventKind::ToolCall {
                parent_tool_use_id, ..
            } => assert_eq!(parent_tool_use_id.as_deref(), Some("parent-1")),
            _ => panic!("expected ToolCall"),
        }
    }

    // ── ToolResult ──────────────────────────────────────────────────

    #[test]
    fn tool_result_ok() {
        let e = ToolResultBuilder::new("read_file")
            .tool_use_id("tc-1")
            .output(json!("file contents"))
            .build()
            .unwrap();
        match &e.kind {
            AgentEventKind::ToolResult {
                tool_name,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_error_flag() {
        let e = ToolResultBuilder::new("cmd")
            .is_error(true)
            .build()
            .unwrap();
        match &e.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_empty_name_fails() {
        let err = ToolResultBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "tool_name");
    }

    // ── FileEdit ────────────────────────────────────────────────────

    #[test]
    fn file_edit_ok() {
        let e = FileEditBuilder::new("src/main.rs")
            .summary("added function")
            .build()
            .unwrap();
        match &e.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "added function");
            }
            _ => panic!("expected FileChanged"),
        }
    }

    #[test]
    fn file_edit_empty_path_fails() {
        let err = FileEditBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "path");
    }

    #[test]
    fn file_edit_default_summary() {
        let e = FileEditBuilder::new("a.rs").build().unwrap();
        match &e.kind {
            AgentEventKind::FileChanged { summary, .. } => assert_eq!(summary, ""),
            _ => panic!("expected FileChanged"),
        }
    }

    // ── CommandRun ──────────────────────────────────────────────────

    #[test]
    fn command_run_ok() {
        let e = CommandRunBuilder::new("cargo test")
            .exit_code(0)
            .output_preview("all passed")
            .build()
            .unwrap();
        match &e.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(*exit_code, Some(0));
                assert_eq!(output_preview.as_deref(), Some("all passed"));
            }
            _ => panic!("expected CommandExecuted"),
        }
    }

    #[test]
    fn command_run_empty_fails() {
        let err = CommandRunBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "command");
    }

    // ── Thinking ────────────────────────────────────────────────────

    #[test]
    fn thinking_ok() {
        let e = ThinkingBuilder::new("reasoning about problem")
            .build()
            .unwrap();
        match &e.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "reasoning about problem");
            }
            _ => panic!("expected AssistantMessage"),
        }
        assert_eq!(e.ext.as_ref().unwrap()["_thinking"], Value::Bool(true));
    }

    #[test]
    fn thinking_empty_fails() {
        let err = ThinkingBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "text");
    }

    // ── Usage ───────────────────────────────────────────────────────

    #[test]
    fn usage_ok() {
        let e = UsageBuilder::new()
            .input_tokens(100)
            .output_tokens(50)
            .model("gpt-4")
            .build()
            .unwrap();
        let ext = e.ext.as_ref().unwrap();
        assert_eq!(ext["_usage"], Value::Bool(true));
        assert_eq!(ext["input_tokens"], json!(100));
        assert_eq!(ext["output_tokens"], json!(50));
        assert_eq!(ext["model"], json!("gpt-4"));
    }

    #[test]
    fn usage_no_tokens_fails() {
        let err = UsageBuilder::new().build().unwrap_err();
        assert!(err.field.contains("tokens"));
    }

    #[test]
    fn usage_partial_tokens_ok() {
        let e = UsageBuilder::new().input_tokens(42).build().unwrap();
        let ext = e.ext.unwrap();
        assert_eq!(ext["input_tokens"], json!(42));
        assert!(!ext.contains_key("output_tokens"));
    }

    // ── Error ───────────────────────────────────────────────────────

    #[test]
    fn error_ok() {
        let e = ErrorBuilder::new("something broke").build().unwrap();
        match &e.kind {
            AgentEventKind::Error { message, .. } => assert_eq!(message, "something broke"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn error_empty_fails() {
        let err = ErrorBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "message");
    }

    // ── Warning ─────────────────────────────────────────────────────

    #[test]
    fn warning_ok() {
        let e = WarningBuilder::new("careful").build().unwrap();
        match &e.kind {
            AgentEventKind::Warning { message } => assert_eq!(message, "careful"),
            _ => panic!("expected Warning"),
        }
    }

    #[test]
    fn warning_empty_fails() {
        let err = WarningBuilder::new("").build().unwrap_err();
        assert_eq!(err.field, "message");
    }

    // ── RunStarted / RunCompleted ───────────────────────────────────

    #[test]
    fn run_started_ok() {
        let e = RunStartedBuilder::new("starting").build().unwrap();
        assert!(
            matches!(e.kind, AgentEventKind::RunStarted { ref message } if message == "starting")
        );
    }

    #[test]
    fn run_started_empty_fails() {
        assert!(RunStartedBuilder::new("").build().is_err());
    }

    #[test]
    fn run_completed_ok() {
        let e = RunCompletedBuilder::new("done").build().unwrap();
        assert!(
            matches!(e.kind, AgentEventKind::RunCompleted { ref message } if message == "done")
        );
    }

    #[test]
    fn run_completed_empty_fails() {
        assert!(RunCompletedBuilder::new("").build().is_err());
    }

    // ── TextMessage ─────────────────────────────────────────────────

    #[test]
    fn text_message_ok() {
        let e = TextMessageBuilder::new("hello world").build().unwrap();
        assert!(
            matches!(e.kind, AgentEventKind::AssistantMessage { ref text } if text == "hello world")
        );
    }

    #[test]
    fn text_message_empty_fails() {
        assert!(TextMessageBuilder::new("").build().is_err());
    }

    // ── Custom timestamp ────────────────────────────────────────────

    #[test]
    fn custom_timestamp_propagates() {
        use chrono::TimeZone;
        let fixed = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
        let e = TextDeltaBuilder::new("ts test")
            .timestamp(fixed)
            .build()
            .unwrap();
        assert_eq!(e.ts, fixed);
    }

    // ── EventBuildError display ─────────────────────────────────────

    #[test]
    fn error_display() {
        let err = EventBuildError {
            builder: "Test",
            field: "foo".into(),
        };
        assert!(err.to_string().contains("Test"));
        assert!(err.to_string().contains("foo"));
    }

    // ── All builders serialise to valid JSON ────────────────────────

    #[test]
    fn all_builders_produce_serializable_events() {
        let events: Vec<AgentEvent> = vec![
            TextDeltaBuilder::new("d").build().unwrap(),
            TextMessageBuilder::new("m").build().unwrap(),
            ToolCallBuilder::new("t").build().unwrap(),
            ToolResultBuilder::new("t")
                .output(json!("ok"))
                .build()
                .unwrap(),
            FileEditBuilder::new("f.rs").build().unwrap(),
            CommandRunBuilder::new("ls").build().unwrap(),
            ThinkingBuilder::new("hmm").build().unwrap(),
            UsageBuilder::new().input_tokens(1).build().unwrap(),
            ErrorBuilder::new("e").build().unwrap(),
            WarningBuilder::new("w").build().unwrap(),
            RunStartedBuilder::new("go").build().unwrap(),
            RunCompletedBuilder::new("done").build().unwrap(),
        ];
        for e in &events {
            let json = serde_json::to_string(e).expect("serialize");
            let _: AgentEvent = serde_json::from_str(&json).expect("roundtrip");
        }
    }
}
