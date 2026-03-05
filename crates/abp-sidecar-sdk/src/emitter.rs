// SPDX-License-Identifier: MIT OR Apache-2.0
//! Event emitter for streaming agent events back to the control plane.
//!
//! [`EventEmitter`] provides ergonomic helper methods for emitting the
//! most common event types without manually constructing [`AgentEvent`]
//! structs.

use abp_core::{AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder};
use abp_error::ErrorCode;
use chrono::Utc;
use tokio::sync::mpsc;

/// Sends [`AgentEvent`]s to the runtime for JSONL serialization.
///
/// Each method builds the correct [`AgentEventKind`], wraps it in an
/// [`AgentEvent`] with the current timestamp, and sends it through the
/// internal channel.
///
/// # Examples
///
/// ```
/// use abp_sidecar_sdk::emitter::EventEmitter;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let (emitter, mut rx) = EventEmitter::new("run-1", 16);
/// emitter.emit_text_delta("hello").await.unwrap();
/// let event = rx.recv().await.unwrap();
/// assert!(matches!(event.kind, abp_core::AgentEventKind::AssistantDelta { .. }));
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct EventEmitter {
    ref_id: String,
    tx: mpsc::Sender<AgentEvent>,
}

impl EventEmitter {
    /// Create a new emitter and its receiving channel.
    ///
    /// `buffer` controls the channel capacity.
    #[must_use]
    pub fn new(ref_id: impl Into<String>, buffer: usize) -> (Self, mpsc::Receiver<AgentEvent>) {
        let (tx, rx) = mpsc::channel(buffer);
        (
            Self {
                ref_id: ref_id.into(),
                tx,
            },
            rx,
        )
    }

    /// Create an emitter from an existing sender.
    #[must_use]
    pub fn from_sender(ref_id: impl Into<String>, tx: mpsc::Sender<AgentEvent>) -> Self {
        Self {
            ref_id: ref_id.into(),
            tx,
        }
    }

    /// The run reference ID this emitter is associated with.
    #[must_use]
    pub fn ref_id(&self) -> &str {
        &self.ref_id
    }

    /// Emit a streaming text delta (token).
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_text_delta(&self, text: &str) -> Result<(), EmitError> {
        self.emit(AgentEventKind::AssistantDelta {
            text: text.to_string(),
        })
        .await
    }

    /// Emit a complete assistant message.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_message(&self, text: &str) -> Result<(), EmitError> {
        self.emit(AgentEventKind::AssistantMessage {
            text: text.to_string(),
        })
        .await
    }

    /// Emit a tool call start event.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_tool_call_start(
        &self,
        tool_name: &str,
        tool_id: &str,
        input: serde_json::Value,
    ) -> Result<(), EmitError> {
        self.emit(AgentEventKind::ToolCall {
            tool_name: tool_name.to_string(),
            tool_use_id: Some(tool_id.to_string()),
            parent_tool_use_id: None,
            input,
        })
        .await
    }

    /// Emit a tool result event.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_tool_result(
        &self,
        tool_name: &str,
        tool_id: &str,
        output: serde_json::Value,
        is_error: bool,
    ) -> Result<(), EmitError> {
        self.emit(AgentEventKind::ToolResult {
            tool_name: tool_name.to_string(),
            tool_use_id: Some(tool_id.to_string()),
            output,
            is_error,
        })
        .await
    }

    /// Emit a warning event.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_warning(&self, message: &str) -> Result<(), EmitError> {
        self.emit(AgentEventKind::Warning {
            message: message.to_string(),
        })
        .await
    }

    /// Emit an error event with an optional error code.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_error(
        &self,
        code: Option<ErrorCode>,
        message: &str,
    ) -> Result<(), EmitError> {
        self.emit(AgentEventKind::Error {
            message: message.to_string(),
            error_code: code,
        })
        .await
    }

    /// Emit a run started event.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_run_started(&self, message: &str) -> Result<(), EmitError> {
        self.emit(AgentEventKind::RunStarted {
            message: message.to_string(),
        })
        .await
    }

    /// Emit a run completed event.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_run_completed(&self, message: &str) -> Result<(), EmitError> {
        self.emit(AgentEventKind::RunCompleted {
            message: message.to_string(),
        })
        .await
    }

    /// Emit a file-changed event.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_file_changed(&self, path: &str, summary: &str) -> Result<(), EmitError> {
        self.emit(AgentEventKind::FileChanged {
            path: path.to_string(),
            summary: summary.to_string(),
        })
        .await
    }

    /// Emit a command-executed event.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiving end has been dropped.
    pub async fn emit_command_executed(
        &self,
        command: &str,
        exit_code: Option<i32>,
        output_preview: Option<&str>,
    ) -> Result<(), EmitError> {
        self.emit(AgentEventKind::CommandExecuted {
            command: command.to_string(),
            exit_code,
            output_preview: output_preview.map(String::from),
        })
        .await
    }

    /// Build a simple completed receipt for this run's backend.
    #[must_use]
    pub fn finish(&self, backend_id: &str) -> Receipt {
        ReceiptBuilder::new(backend_id)
            .outcome(Outcome::Complete)
            .build()
    }

    /// Build a failed receipt for this run's backend.
    #[must_use]
    pub fn finish_failed(&self, backend_id: &str) -> Receipt {
        ReceiptBuilder::new(backend_id)
            .outcome(Outcome::Failed)
            .build()
    }

    // -- internal ---------------------------------------------------------

    async fn emit(&self, kind: AgentEventKind) -> Result<(), EmitError> {
        let event = AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        };
        self.tx
            .send(event)
            .await
            .map_err(|_| EmitError::ChannelClosed)
    }
}

/// Errors that can occur when emitting events.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EmitError {
    /// The receiving end of the event channel has been dropped.
    #[error("event channel closed")]
    ChannelClosed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_text_delta() {
        let (emitter, mut rx) = EventEmitter::new("run-1", 4);
        emitter.emit_text_delta("hi").await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { text } if text == "hi"));
    }

    #[tokio::test]
    async fn emit_on_closed_channel() {
        let (emitter, rx) = EventEmitter::new("run-1", 4);
        drop(rx);
        let err = emitter.emit_text_delta("hi").await.unwrap_err();
        assert!(matches!(err, EmitError::ChannelClosed));
    }
}
