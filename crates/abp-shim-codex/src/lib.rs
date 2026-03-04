// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-shim-codex
//!
//! Drop-in Codex SDK shim that routes through ABP's intermediate representation.

/// Conversion layer between Codex Responses API types and ABP core types.
pub mod convert;
/// HTTP client for the OpenAI Codex Responses API.
pub mod client;
/// Codex Responses API types (builder, usage statistics).
pub mod types;

// Re-export everything from types and convert for backward compatibility.
pub use convert::*;
pub use types::*;

use std::pin::Pin;

use abp_codex_sdk::dialect::{CodexInputItem, CodexRequest, CodexResponse, CodexStreamEvent};
use abp_core::{AgentEvent, Receipt, UsageNormalized, WorkOrder};
use chrono::Utc;
use tokio_stream::Stream;

// These are used by tests via `use super::*`.
#[cfg(test)]
use abp_codex_sdk::dialect::{CodexContentPart, CodexResponseItem};
#[cfg(test)]
use abp_core::ir::{IrRole, IrUsage};
#[cfg(test)]
use abp_core::AgentEventKind;

// Re-export key types from the Codex SDK for convenience.
pub use abp_codex_sdk::dialect::{
    CodexFunctionDef, CodexTextFormat, CodexTool, CodexToolDef, SandboxConfig,
};

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

// ── Input item constructors ─────────────────────────────────────────────

/// Create a [`CodexInputItem::Message`] conveniently.
#[must_use]
pub fn codex_message(role: impl Into<String>, content: impl Into<String>) -> CodexInputItem {
    CodexInputItem::Message {
        role: role.into(),
        content: content.into(),
    }
}

/// A Codex Responses API request matching the Codex API surface.
///
/// This is a re-export of [`abp_codex_sdk::dialect::CodexRequest`].
pub use abp_codex_sdk::dialect::CodexRequest as CodexShimRequest;

// ── Client types ────────────────────────────────────────────────────────

/// A callback function that processes a [`WorkOrder`] and returns a [`Receipt`].
pub type ProcessFn = Box<dyn Fn(&WorkOrder) -> Receipt + Send + Sync>;

/// Drop-in compatible Codex client that routes through ABP.
pub struct CodexClient {
    model: String,
    processor: Option<ProcessFn>,
}

impl std::fmt::Debug for CodexClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexClient")
            .field("model", &self.model)
            .finish()
    }
}

impl CodexClient {
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

    /// Create a Codex response (non-streaming).
    pub async fn create(&self, request: CodexRequest) -> Result<CodexResponse> {
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

    /// Create a streaming Codex response.
    pub async fn create_stream(
        &self,
        request: CodexRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CodexStreamEvent> + Send>>> {
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
    use serde_json::json;
    use tokio_stream::StreamExt;

    fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
        Box::new(move |_wo| mock_receipt(events.clone()))
    }

    fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
        Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
    }

    // ── 1. Simple completion roundtrip ──────────────────────────────────

    #[tokio::test]
    async fn simple_completion() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        }];
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Hi")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "codex-mini-latest");
        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexResponseItem::Message { content, .. } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    CodexContentPart::OutputText { text } => assert_eq!(text, "Hello!"),
                }
            }
            other => panic!("expected Message, got {other:?}"),
        }
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
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "Hi")])
            .build();

        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<CodexStreamEvent> = stream.collect().await;
        // 1 created + 2 deltas + 1 completed
        assert_eq!(chunks.len(), 4);
        assert!(matches!(
            &chunks[0],
            CodexStreamEvent::ResponseCreated { .. }
        ));
        assert!(matches!(
            &chunks[3],
            CodexStreamEvent::ResponseCompleted { .. }
        ));
    }

    // ── 3. Tool use / function calling ──────────────────────────────────

    #[tokio::test]
    async fn tool_use_function_calling() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "shell".into(),
                tool_use_id: Some("fc_abc".into()),
                parent_tool_use_id: None,
                input: json!({"command": "ls"}),
            },
            ext: None,
        }];
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "List files")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.output.len(), 1);
        match &resp.output[0] {
            CodexResponseItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "fc_abc");
                assert_eq!(name, "shell");
                assert!(arguments.contains("ls"));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    // ── 4. Model name preservation ──────────────────────────────────────

    #[tokio::test]
    async fn model_name_preserved() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let client = CodexClient::new("o3-mini").with_processor(make_processor(events));
        let req = CodexRequestBuilder::new()
            .model("o3-mini")
            .input(vec![codex_message("user", "test")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "o3-mini");
    }

    // ── 5. Error response ───────────────────────────────────────────────

    #[tokio::test]
    async fn error_response() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "rate limit".into(),
                error_code: None,
            },
            ext: None,
        }];
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();

        let resp = client.create(req).await.unwrap();
        match &resp.output[0] {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => {
                    assert!(text.contains("rate limit"));
                }
            },
            other => panic!("expected Message, got {other:?}"),
        }
    }

    // ── 6. Token usage tracking ─────────────────────────────────────────

    #[tokio::test]
    async fn token_usage_tracking() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        };
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "response".into(),
            },
            ext: None,
        }];
        let client = CodexClient::new("codex-mini-latest")
            .with_processor(make_processor_with_usage(events, usage));
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();

        let resp = client.create(req).await.unwrap();
        let u = resp.usage.unwrap();
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    // ── 7. Request to IR roundtrip ──────────────────────────────────────

    #[test]
    fn request_to_ir_roundtrip() {
        let req = CodexRequestBuilder::new()
            .input(vec![
                codex_message("system", "Be concise."),
                codex_message("user", "Hello"),
            ])
            .build();

        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[1].text_content(), "Hello");
    }

    // ── 8. Response to IR and back ──────────────────────────────────────

    #[test]
    fn response_to_ir_and_back() {
        let resp = CodexResponse {
            id: "resp_1".into(),
            model: "codex-mini-latest".into(),
            output: vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Done".into(),
                }],
            }],
            usage: None,
            status: None,
        };

        let conv = response_to_ir(&resp);
        assert_eq!(conv.len(), 1);
        let back = ir_to_response_items(&conv);
        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Done"),
            },
            other => panic!("expected Message, got {other:?}"),
        }
    }

    // ── 9. No processor returns error ───────────────────────────────────

    #[tokio::test]
    async fn no_processor_returns_error() {
        let client = CodexClient::new("codex-mini-latest");
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();

        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── 10. Builder defaults model ──────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();

        assert_eq!(req.model, "codex-mini-latest");
    }

    // ── 11. Stream events include bookends ──────────────────────────────

    #[test]
    fn stream_events_have_created_and_completed() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        }];

        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(matches!(
            &stream[0],
            CodexStreamEvent::ResponseCreated { .. }
        ));
        assert!(matches!(
            stream.last().unwrap(),
            CodexStreamEvent::ResponseCompleted { .. }
        ));
    }

    // ── 12. IR usage conversion ─────────────────────────────────────────

    #[test]
    fn ir_usage_converts_correctly() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── 13. Request model maps to work order ────────────────────────────

    #[test]
    fn request_model_maps_to_work_order() {
        let req = CodexRequestBuilder::new()
            .model("o3-mini")
            .input(vec![codex_message("user", "test")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }

    // ── 14. Temperature mapping ─────────────────────────────────────────

    #[test]
    fn temperature_mapped_to_work_order() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .temperature(0.7)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.7))
        );
    }

    // ── 15. Max output tokens mapping ───────────────────────────────────

    #[test]
    fn max_output_tokens_mapped_to_work_order() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "test")])
            .max_output_tokens(2048)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_output_tokens"),
            Some(&serde_json::Value::from(2048))
        );
    }

    // ── 16. Multi-tool calls in response ────────────────────────────────

    #[tokio::test]
    async fn multi_tool_calls() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read".into(),
                    tool_use_id: Some("fc_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "a.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read".into(),
                    tool_use_id: Some("fc_2".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "b.rs"}),
                },
                ext: None,
            },
        ];
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "Read files")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.output.len(), 2);
        assert!(matches!(
            &resp.output[0],
            CodexResponseItem::FunctionCall { .. }
        ));
        assert!(matches!(
            &resp.output[1],
            CodexResponseItem::FunctionCall { .. }
        ));
    }

    // ── 17. Response status is completed ─────────────────────────────────

    #[tokio::test]
    async fn response_status_completed() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let client = CodexClient::new("codex-mini-latest").with_processor(make_processor(events));
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.status.as_deref(), Some("completed"));
    }

    // ── 18. No processor stream returns error ───────────────────────────

    #[tokio::test]
    async fn no_processor_stream_returns_error() {
        let client = CodexClient::new("codex-mini-latest");
        let req = CodexRequestBuilder::new()
            .input(vec![codex_message("user", "test")])
            .build();

        let result = client.create_stream(req).await;
        assert!(result.is_err());
    }
}
