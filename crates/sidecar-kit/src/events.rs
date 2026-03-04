// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Typed event builders for constructing [`AgentEvent`]s.
//!
//! These helpers produce fully typed [`abp_core::AgentEvent`] values,
//! complementing the value-based helpers in [`crate::builders`].
//!
//! # Example
//! ```
//! use sidecar_kit::events::{text_event, delta_event, error_event};
//!
//! let msg = text_event("Hello, world!");
//! assert_eq!(msg.kind, abp_core::AgentEventKind::AssistantMessage { text: "Hello, world!".into() });
//!
//! let delta = delta_event("streaming chunk");
//! let err = error_event("E001", "something broke");
//! ```

use chrono::Utc;
use serde_json::Value;

use abp_core::{AgentEvent, AgentEventKind};

/// Fluent builder for constructing [`AgentEvent`] instances.
///
/// Allows setting event kind, optional extensions, and an explicit timestamp.
/// If no timestamp is provided, the current UTC time is used at build time.
///
/// # Example
/// ```
/// use sidecar_kit::events::EventBuilder;
/// use abp_core::AgentEventKind;
///
/// let event = EventBuilder::new(AgentEventKind::Warning { message: "low memory".into() })
///     .build();
/// assert!(matches!(event.kind, AgentEventKind::Warning { .. }));
/// ```
#[derive(Debug, Clone)]
pub struct EventBuilder {
    kind: AgentEventKind,
    ts: Option<chrono::DateTime<Utc>>,
    ext: Option<std::collections::BTreeMap<String, Value>>,
}

impl EventBuilder {
    /// Create a builder for the given event kind.
    #[must_use]
    pub fn new(kind: AgentEventKind) -> Self {
        Self {
            kind,
            ts: None,
            ext: None,
        }
    }

    /// Override the event timestamp (defaults to `Utc::now()` at build time).
    #[must_use]
    pub fn timestamp(mut self, ts: chrono::DateTime<Utc>) -> Self {
        self.ts = Some(ts);
        self
    }

    /// Attach an extension field for passthrough raw data.
    #[must_use]
    pub fn ext(mut self, key: impl Into<String>, value: Value) -> Self {
        self.ext
            .get_or_insert_with(std::collections::BTreeMap::new)
            .insert(key.into(), value);
        self
    }

    /// Consume the builder and produce an [`AgentEvent`].
    #[must_use]
    pub fn build(self) -> AgentEvent {
        AgentEvent {
            ts: self.ts.unwrap_or_else(Utc::now),
            kind: self.kind,
            ext: self.ext,
        }
    }
}

// ── Convenience constructors ────────────────────────────────────────

/// Create a complete assistant message event.
#[must_use]
pub fn text_event(text: impl Into<String>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

/// Create a streaming assistant delta event.
#[must_use]
pub fn delta_event(delta: impl Into<String>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: delta.into() },
        ext: None,
    }
}

/// Create a tool call event.
#[must_use]
pub fn tool_call_event(
    tool_name: impl Into<String>,
    tool_use_id: Option<String>,
    input: Value,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: tool_name.into(),
            tool_use_id,
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

/// Create a tool result event.
#[must_use]
pub fn tool_result_event(
    tool_name: impl Into<String>,
    tool_use_id: Option<String>,
    output: Value,
    is_error: bool,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: tool_name.into(),
            tool_use_id,
            output,
            is_error,
        },
        ext: None,
    }
}

/// Create an error event with a machine-readable code and message.
#[must_use]
pub fn error_event(code: impl Into<String>, message: impl Into<String>) -> AgentEvent {
    // The code is stored in the message field for informational purposes;
    // the typed ErrorCode enum requires an abp-error dependency, so we
    // embed the code string in the message.
    let code_str = code.into();
    let msg = message.into();
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: format!("[{code_str}] {msg}"),
            error_code: None,
        },
        ext: None,
    }
}

/// Create a warning event.
#[must_use]
pub fn warning_event(message: impl Into<String>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: message.into(),
        },
        ext: None,
    }
}

/// Create a file-changed event.
#[must_use]
pub fn file_changed_event(path: impl Into<String>, action: impl Into<String>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: path.into(),
            summary: action.into(),
        },
        ext: None,
    }
}

/// Create a command-executed event.
#[must_use]
pub fn command_event(command: impl Into<String>, exit_code: Option<i32>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: command.into(),
            exit_code,
            output_preview: None,
        },
        ext: None,
    }
}

/// Create a run-started event.
#[must_use]
pub fn run_started_event(message: impl Into<String>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: message.into(),
        },
        ext: None,
    }
}

/// Create a run-completed event.
#[must_use]
pub fn run_completed_event(message: impl Into<String>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: message.into(),
        },
        ext: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_event_has_correct_kind() {
        let e = text_event("hello");
        assert!(matches!(
            e.kind,
            AgentEventKind::AssistantMessage { ref text } if text == "hello"
        ));
        assert!(e.ext.is_none());
    }

    #[test]
    fn delta_event_has_correct_kind() {
        let e = delta_event("chunk");
        assert!(matches!(
            e.kind,
            AgentEventKind::AssistantDelta { ref text } if text == "chunk"
        ));
    }

    #[test]
    fn tool_call_event_fields() {
        let e = tool_call_event(
            "read_file",
            Some("tc-1".into()),
            serde_json::json!({"path": "a.rs"}),
        );
        match &e.kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                parent_tool_use_id,
                input,
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
                assert!(parent_tool_use_id.is_none());
                assert_eq!(input["path"], "a.rs");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn tool_result_event_fields() {
        let e = tool_result_event(
            "read_file",
            Some("tc-1".into()),
            serde_json::json!("ok"),
            false,
        );
        match &e.kind {
            AgentEventKind::ToolResult {
                tool_name,
                tool_use_id,
                output,
                is_error,
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("tc-1"));
                assert_eq!(output, &serde_json::json!("ok"));
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn error_event_embeds_code_in_message() {
        let e = error_event("E001", "something broke");
        match &e.kind {
            AgentEventKind::Error {
                message,
                error_code,
            } => {
                assert!(message.contains("E001"));
                assert!(message.contains("something broke"));
                assert!(error_code.is_none());
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn warning_event_has_correct_kind() {
        let e = warning_event("be careful");
        assert!(matches!(
            e.kind,
            AgentEventKind::Warning { ref message } if message == "be careful"
        ));
    }

    #[test]
    fn file_changed_event_fields() {
        let e = file_changed_event("src/main.rs", "modified");
        match &e.kind {
            AgentEventKind::FileChanged { path, summary } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(summary, "modified");
            }
            _ => panic!("expected FileChanged"),
        }
    }

    #[test]
    fn command_event_fields() {
        let e = command_event("cargo test", Some(0));
        match &e.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code,
                output_preview,
            } => {
                assert_eq!(command, "cargo test");
                assert_eq!(*exit_code, Some(0));
                assert!(output_preview.is_none());
            }
            _ => panic!("expected CommandExecuted"),
        }
    }

    #[test]
    fn run_lifecycle_events() {
        let start = run_started_event("starting run");
        assert!(matches!(
            start.kind,
            AgentEventKind::RunStarted { ref message } if message == "starting run"
        ));

        let end = run_completed_event("done");
        assert!(matches!(
            end.kind,
            AgentEventKind::RunCompleted { ref message } if message == "done"
        ));
    }

    #[test]
    fn event_builder_with_extensions() {
        let e = EventBuilder::new(AgentEventKind::Warning {
            message: "test".into(),
        })
        .ext("raw_message", serde_json::json!({"original": true}))
        .build();

        assert!(e.ext.is_some());
        let ext = e.ext.as_ref().unwrap();
        assert!(ext.contains_key("raw_message"));
    }

    #[test]
    fn event_builder_custom_timestamp() {
        use chrono::TimeZone;
        let fixed = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let e = EventBuilder::new(AgentEventKind::Warning {
            message: "test".into(),
        })
        .timestamp(fixed)
        .build();

        assert_eq!(e.ts, fixed);
    }

    #[test]
    fn events_serialize_to_valid_json() {
        let events = vec![
            text_event("hi"),
            delta_event("chunk"),
            warning_event("warn"),
            error_event("E1", "err"),
            file_changed_event("a.rs", "created"),
            command_event("ls", Some(0)),
            run_started_event("go"),
            run_completed_event("done"),
        ];
        for e in events {
            let json = serde_json::to_string(&e).expect("serialize");
            let _roundtrip: AgentEvent = serde_json::from_str(&json).expect("deserialize");
        }
    }
}
