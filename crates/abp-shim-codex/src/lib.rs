// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-shim-codex
//!
//! Drop-in Codex SDK shim that routes through ABP's intermediate representation.

use std::pin::Pin;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexRequest, CodexResponse, CodexResponseItem,
    CodexStreamDelta, CodexStreamEvent, CodexUsage,
};
use abp_codex_sdk::lowering;
use abp_core::ir::{IrConversation, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Receipt, UsageNormalized, WorkOrder, WorkOrderBuilder};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

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

// ── Request builder ─────────────────────────────────────────────────────

/// A Codex Responses API request matching the Codex API surface.
///
/// This is a re-export of [`abp_codex_sdk::dialect::CodexRequest`].
pub use abp_codex_sdk::dialect::CodexRequest as CodexShimRequest;

/// Builder for [`CodexRequest`].
#[derive(Debug, Default)]
pub struct CodexRequestBuilder {
    model: Option<String>,
    input: Vec<CodexInputItem>,
    max_output_tokens: Option<u32>,
    temperature: Option<f64>,
    tools: Vec<CodexTool>,
    text: Option<CodexTextFormat>,
}

impl CodexRequestBuilder {
    /// Create a new builder for a Codex request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the input items.
    #[must_use]
    pub fn input(mut self, input: Vec<CodexInputItem>) -> Self {
        self.input = input;
        self
    }

    /// Set the maximum output tokens.
    #[must_use]
    pub fn max_output_tokens(mut self, max: u32) -> Self {
        self.max_output_tokens = Some(max);
        self
    }

    /// Set the temperature.
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<CodexTool>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the text format.
    #[must_use]
    pub fn text(mut self, text: CodexTextFormat) -> Self {
        self.text = Some(text);
        self
    }

    /// Build the request, defaulting model to `"codex-mini-latest"` if unset.
    #[must_use]
    pub fn build(self) -> CodexRequest {
        CodexRequest {
            model: self.model.unwrap_or_else(|| "codex-mini-latest".into()),
            input: self.input,
            max_output_tokens: self.max_output_tokens,
            temperature: self.temperature,
            tools: self.tools,
            text: self.text,
        }
    }
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Tokens consumed by the input.
    pub input_tokens: u64,
    /// Tokens generated in the output.
    pub output_tokens: u64,
    /// Total tokens (input + output).
    pub total_tokens: u64,
}

// ── Conversion: request → IR → WorkOrder ────────────────────────────────

/// Convert a [`CodexRequest`] into an [`IrConversation`].
pub fn request_to_ir(request: &CodexRequest) -> IrConversation {
    lowering::input_to_ir(&request.input)
}

/// Convert a [`CodexRequest`] into an ABP [`WorkOrder`].
pub fn request_to_work_order(request: &CodexRequest) -> WorkOrder {
    let conv = request_to_ir(request);
    let task = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "codex completion".into());

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    let mut vendor = std::collections::BTreeMap::new();
    if let Some(temp) = request.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(max) = request.max_output_tokens {
        vendor.insert(
            "max_output_tokens".to_string(),
            serde_json::Value::from(max),
        );
    }
    let config = abp_core::RuntimeConfig {
        model: Some(request.model.clone()),
        vendor,
        ..Default::default()
    };
    builder = builder.config(config);

    builder.build()
}

// ── Conversion: Receipt → CodexResponse ─────────────────────────────────

/// Build a [`CodexResponse`] from a [`Receipt`] and the original model name.
pub fn receipt_to_response(receipt: &Receipt, model: &str) -> CodexResponse {
    let mut output = Vec::new();

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                output.push(CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText { text: text.clone() }],
                });
            }
            AgentEventKind::AssistantDelta { text } => {
                output.push(CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText { text: text.clone() }],
                });
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                output.push(CodexResponseItem::FunctionCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("fc_{}", uuid::Uuid::new_v4())),
                    call_id: None,
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                });
            }
            AgentEventKind::Error { message, .. } => {
                output.push(CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText {
                        text: format!("Error: {message}"),
                    }],
                });
            }
            _ => {}
        }
    }

    let usage = usage_from_receipt(&receipt.usage);

    CodexResponse {
        id: format!("resp_{}", receipt.meta.run_id),
        model: model.to_string(),
        output,
        usage: Some(usage),
        status: Some("completed".into()),
    }
}

/// Convert normalized usage to Codex-style usage.
fn usage_from_receipt(usage: &UsageNormalized) -> CodexUsage {
    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    CodexUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input + output,
    }
}

/// Build [`CodexStreamEvent`]s from a sequence of [`AgentEvent`]s.
pub fn events_to_stream_events(events: &[AgentEvent], model: &str) -> Vec<CodexStreamEvent> {
    let run_id = format!("resp_{}", uuid::Uuid::new_v4());
    let mut stream_events = Vec::new();

    // Initial created event
    stream_events.push(CodexStreamEvent::ResponseCreated {
        response: CodexResponse {
            id: run_id.clone(),
            model: model.to_string(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    });

    for (i, event) in events.iter().enumerate() {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                stream_events.push(CodexStreamEvent::OutputItemDelta {
                    output_index: i,
                    delta: CodexStreamDelta::OutputTextDelta { text: text.clone() },
                });
            }
            AgentEventKind::AssistantMessage { text } => {
                let item = CodexResponseItem::Message {
                    role: "assistant".into(),
                    content: vec![CodexContentPart::OutputText { text: text.clone() }],
                };
                stream_events.push(CodexStreamEvent::OutputItemDone {
                    output_index: i,
                    item,
                });
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                let item = CodexResponseItem::FunctionCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("fc_{}", uuid::Uuid::new_v4())),
                    call_id: None,
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                };
                stream_events.push(CodexStreamEvent::OutputItemDone {
                    output_index: i,
                    item,
                });
            }
            _ => {}
        }
    }

    // Final completed event
    stream_events.push(CodexStreamEvent::ResponseCompleted {
        response: CodexResponse {
            id: run_id,
            model: model.to_string(),
            output: vec![],
            usage: None,
            status: Some("completed".into()),
        },
    });

    stream_events
}

/// Convert a [`CodexResponse`] into an [`IrConversation`].
pub fn response_to_ir(response: &CodexResponse) -> IrConversation {
    lowering::to_ir(&response.output)
}

/// Convert an [`IrConversation`] back into Codex response items.
pub fn ir_to_response_items(conv: &IrConversation) -> Vec<CodexResponseItem> {
    lowering::from_ir(conv)
}

/// Convert an [`IrUsage`] to shim [`Usage`].
pub fn ir_usage_to_usage(ir: &IrUsage) -> Usage {
    Usage {
        input_tokens: ir.input_tokens,
        output_tokens: ir.output_tokens,
        total_tokens: ir.total_tokens,
    }
}

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
