// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-shim-copilot
//!
//! Drop-in Copilot SDK shim that routes through ABP's intermediate representation.

/// HTTP client for the GitHub Copilot Chat API.
pub mod client;
/// Conversion layer between Copilot SDK types and ABP core types.
pub mod convert;
/// Copilot SDK–specific types: messages, request builder, and helpers.
pub mod types;

use std::pin::Pin;

use abp_copilot_sdk::dialect::{CopilotRequest, CopilotResponse, CopilotStreamEvent};
use abp_core::{AgentEvent, Receipt, UsageNormalized, WorkOrder};
use chrono::Utc;
use tokio_stream::Stream;

// Re-export key types from the Copilot SDK for convenience.
pub use abp_copilot_sdk::dialect::{CopilotFunctionDef, CopilotToolType};

// Re-export types and conversions for backward compatibility.
pub use convert::*;
pub use types::*;

// ── Error types ─────────────────────────────────────────────────────────

/// Errors produced by the shim client.
#[derive(Debug, thiserror::Error)]
pub enum ShimError {
    /// The request was invalid.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// An internal processing error.
    #[error("internal error: {0}")]
    Internal(String),
    /// Serialization / deserialization error.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Result alias for shim operations.
pub type Result<T> = std::result::Result<T, ShimError>;

// ── Client types ────────────────────────────────────────────────────────

/// A callback function that processes a [`WorkOrder`] and returns a [`Receipt`].
pub type ProcessFn = Box<dyn Fn(&WorkOrder) -> Receipt + Send + Sync>;

/// Drop-in compatible Copilot client that routes through ABP.
pub struct CopilotClient {
    model: String,
    processor: Option<ProcessFn>,
}

impl std::fmt::Debug for CopilotClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopilotClient")
            .field("model", &self.model)
            .finish()
    }
}

impl CopilotClient {
    /// Create a new client targeting the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            processor: None,
        }
    }

    /// Set a custom processor function for handling work orders.
    #[must_use]
    pub fn with_processor(mut self, processor: ProcessFn) -> Self {
        self.processor = Some(processor);
        self
    }

    /// Get the configured model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Create a Copilot response (non-streaming).
    pub async fn create(&self, request: CopilotRequest) -> Result<CopilotResponse> {
        let work_order = request_to_work_order(&request);

        let receipt = if let Some(processor) = &self.processor {
            processor(&work_order)
        } else {
            return Err(ShimError::Internal(
                "no processor configured; use with_processor() to set a backend".into(),
            ));
        };

        Ok(receipt_to_response(&receipt, &request.model))
    }

    /// Create a streaming Copilot response.
    pub async fn create_stream(
        &self,
        request: CopilotRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CopilotStreamEvent> + Send>>> {
        let work_order = request_to_work_order(&request);
        let model = request.model.clone();

        let receipt = if let Some(processor) = &self.processor {
            processor(&work_order)
        } else {
            return Err(ShimError::Internal(
                "no processor configured; use with_processor() to set a backend".into(),
            ));
        };

        let stream_events = events_to_stream_events(&receipt.trace, &model);
        Ok(Box::pin(tokio_stream::iter(stream_events)))
    }
}

// ── Test helpers ────────────────────────────────────────────────────────

/// Create a mock receipt for testing purposes.
#[must_use]
pub fn mock_receipt(events: Vec<AgentEvent>) -> Receipt {
    mock_receipt_with_usage(events, UsageNormalized::default())
}

/// Create a mock receipt with specified usage.
#[must_use]
pub fn mock_receipt_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> Receipt {
    let now = Utc::now();
    let run_id = uuid::Uuid::new_v4();
    Receipt {
        meta: abp_core::RunMetadata {
            run_id,
            work_order_id: uuid::Uuid::new_v4(),
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: abp_core::BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: Default::default(),
        mode: abp_core::ExecutionMode::Mapped,
        usage_raw: serde_json::Value::Null,
        usage,
        trace: events,
        artifacts: vec![],
        verification: Default::default(),
        outcome: abp_core::Outcome::Complete,
        receipt_sha256: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEventKind;
    use abp_core::ir::{IrRole, IrUsage};
    use serde_json::json;
    use tokio_stream::StreamExt;

    fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
        Box::new(move |_wo| mock_receipt(events.clone()))
    }

    // ── 1. Simple completion roundtrip ──────────────────────────────────

    #[tokio::test]
    async fn simple_completion() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello from Copilot!".into(),
            },
            ext: None,
        }];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "Hello from Copilot!");
        assert!(resp.copilot_errors.is_empty());
    }

    // ── 2. Streaming completion ─────────────────────────────────────────

    #[tokio::test]
    async fn streaming_completion() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "Hel".into() },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta { text: "lo!".into() },
                ext: None,
            },
        ];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .build();

        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
        // 1 references + 2 deltas + 1 done
        assert_eq!(chunks.len(), 4);
        assert!(matches!(
            &chunks[0],
            CopilotStreamEvent::CopilotReferences { .. }
        ));
        assert!(matches!(&chunks[1], CopilotStreamEvent::TextDelta { .. }));
        assert!(matches!(&chunks[2], CopilotStreamEvent::TextDelta { .. }));
        assert!(matches!(&chunks[3], CopilotStreamEvent::Done {}));
    }

    // ── 3. Tool use / function calling ──────────────────────────────────

    #[tokio::test]
    async fn tool_use_function_calling() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_abc".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        }];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Read the main file")])
            .build();

        let resp = client.create(req).await.unwrap();
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "read_file");
        assert_eq!(fc.id.as_deref(), Some("call_abc"));
        assert!(fc.arguments.contains("main.rs"));
    }

    // ── 4. System message handling ──────────────────────────────────────

    #[tokio::test]
    async fn system_message_handling() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "I am helpful.".into(),
            },
            ext: None,
        }];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .messages(vec![
                Message::system("You are a helpful assistant."),
                Message::user("Hello"),
            ])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "I am helpful.");
    }

    // ── 5. Multi-turn conversation ──────────────────────────────────────

    #[tokio::test]
    async fn multi_turn_conversation() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "4".into() },
            ext: None,
        }];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .messages(vec![
                Message::user("What is 2+2?"),
                Message::assistant("Let me calculate..."),
                Message::user("Just the number"),
            ])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "4");
    }

    // ── 6. Model name preservation ──────────────────────────────────────

    #[tokio::test]
    async fn model_name_in_work_order() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let _client = CopilotClient::new("gpt-4-turbo").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    // ── 7. Error response ───────────────────────────────────────────────

    #[tokio::test]
    async fn error_response() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded".into(),
                error_code: None,
            },
            ext: None,
        }];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("rate limit"));
    }

    // ── 8. Request to IR roundtrip ──────────────────────────────────────

    #[test]
    fn request_to_ir_roundtrip() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::system("Be concise."), Message::user("Hello")])
            .build();

        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[1].text_content(), "Hello");
    }

    // ── 9. Messages to IR and back ──────────────────────────────────────

    #[test]
    fn messages_to_ir_and_back() {
        let messages = vec![
            Message::system("System prompt"),
            Message::user("User message"),
            Message::assistant("Assistant reply"),
        ];

        let conv = messages_to_ir(&messages);
        let back = ir_to_messages(&conv);

        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content, "System prompt");
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    // ── 10. No processor returns error ──────────────────────────────────

    #[tokio::test]
    async fn no_processor_returns_error() {
        let client = CopilotClient::new("gpt-4o");
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── 11. Builder defaults model ──────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        assert_eq!(req.model, "gpt-4o");
    }

    // ── 12. Stream events include done ──────────────────────────────────

    #[test]
    fn stream_events_end_with_done() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        }];

        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(matches!(
            stream.last().unwrap(),
            CopilotStreamEvent::Done {}
        ));
    }

    // ── 13. IR usage conversion ─────────────────────────────────────────

    #[test]
    fn ir_usage_converts_correctly() {
        let ir = IrUsage::from_io(200, 100);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 200);
        assert_eq!(output, 100);
        assert_eq!(total, 300);
    }

    // ── 14. Request model maps to work order ────────────────────────────

    #[test]
    fn request_model_maps_to_work_order() {
        let req = CopilotRequestBuilder::new()
            .model("o3-mini")
            .messages(vec![Message::user("test")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }

    // ── 15. Response to IR ──────────────────────────────────────────────

    #[test]
    fn response_to_ir_roundtrip() {
        let resp = CopilotResponse {
            message: "Hello!".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };

        let conv = response_to_ir(&resp);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Hello!");
    }

    // ── 16. Empty response to IR ────────────────────────────────────────

    #[test]
    fn empty_response_to_ir() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };

        let conv = response_to_ir(&resp);
        assert!(conv.is_empty());
    }

    // ── 17. Stream error event ──────────────────────────────────────────

    #[test]
    fn stream_error_event() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "boom".into(),
                error_code: None,
            },
            ext: None,
        }];

        let stream = events_to_stream_events(&events, "gpt-4o");
        // references + error + done
        assert_eq!(stream.len(), 3);
        assert!(matches!(
            &stream[1],
            CopilotStreamEvent::CopilotErrors { .. }
        ));
    }

    // ── 18. No processor stream returns error ───────────────────────────

    #[tokio::test]
    async fn no_processor_stream_returns_error() {
        let client = CopilotClient::new("gpt-4o");
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let result = client.create_stream(req).await;
        assert!(result.is_err());
    }

    // ── 19. Function call in stream ─────────────────────────────────────

    #[tokio::test]
    async fn function_call_in_stream() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "rust"}),
            },
            ext: None,
        }];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Search")])
            .build();

        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
        // references + function_call + done
        assert_eq!(chunks.len(), 3);
        assert!(matches!(
            &chunks[1],
            CopilotStreamEvent::FunctionCall { .. }
        ));
    }
}
