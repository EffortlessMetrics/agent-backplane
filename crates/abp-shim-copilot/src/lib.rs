// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-shim-copilot
//!
//! Drop-in Copilot SDK shim that routes through ABP's intermediate representation.

use std::pin::Pin;

use abp_copilot_sdk::dialect::{
    CopilotError, CopilotFunctionCall, CopilotMessage, CopilotReference, CopilotRequest,
    CopilotResponse, CopilotStreamEvent, CopilotTool, CopilotTurnEntry,
};
use abp_copilot_sdk::lowering;
use abp_core::ir::{IrConversation, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Receipt, UsageNormalized, WorkOrder, WorkOrderBuilder};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

// Re-export key types from the Copilot SDK for convenience.
pub use abp_copilot_sdk::dialect::{CopilotFunctionDef, CopilotToolType};

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

// ── Message constructors ────────────────────────────────────────────────

/// A chat message in the Copilot format (convenience wrapper).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Optional display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// References attached to this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
}

impl Message {
    /// Create a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            name: None,
            copilot_references: Vec::new(),
        }
    }

    /// Create a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            name: None,
            copilot_references: Vec::new(),
        }
    }

    /// Create an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            name: None,
            copilot_references: Vec::new(),
        }
    }

    /// Create a user message with references.
    #[must_use]
    pub fn user_with_refs(content: impl Into<String>, refs: Vec<CopilotReference>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            name: None,
            copilot_references: refs,
        }
    }
}

// ── Request builder ─────────────────────────────────────────────────────

/// Builder for [`CopilotRequest`].
#[derive(Debug, Default)]
pub struct CopilotRequestBuilder {
    model: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<CopilotTool>>,
    turn_history: Vec<CopilotTurnEntry>,
    references: Vec<CopilotReference>,
}

impl CopilotRequestBuilder {
    /// Create a new builder for a Copilot request.
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

    /// Set the messages.
    #[must_use]
    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Set the tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<CopilotTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the turn history.
    #[must_use]
    pub fn turn_history(mut self, history: Vec<CopilotTurnEntry>) -> Self {
        self.turn_history = history;
        self
    }

    /// Set the references.
    #[must_use]
    pub fn references(mut self, refs: Vec<CopilotReference>) -> Self {
        self.references = refs;
        self
    }

    /// Build the request, defaulting model to `"gpt-4o"` if unset.
    #[must_use]
    pub fn build(self) -> CopilotRequest {
        CopilotRequest {
            model: self.model.unwrap_or_else(|| "gpt-4o".into()),
            messages: self.messages.into_iter().map(to_copilot_message).collect(),
            tools: self.tools,
            turn_history: self.turn_history,
            references: self.references,
        }
    }
}

/// Convert a shim [`Message`] to a [`CopilotMessage`].
fn to_copilot_message(msg: Message) -> CopilotMessage {
    CopilotMessage {
        role: msg.role,
        content: msg.content,
        name: msg.name,
        copilot_references: msg.copilot_references,
    }
}

// ── Conversion: request → IR → WorkOrder ────────────────────────────────

/// Convert a [`CopilotRequest`] into an [`IrConversation`].
pub fn request_to_ir(request: &CopilotRequest) -> IrConversation {
    lowering::to_ir(&request.messages)
}

/// Convert a [`CopilotRequest`] into an ABP [`WorkOrder`].
pub fn request_to_work_order(request: &CopilotRequest) -> WorkOrder {
    let conv = request_to_ir(request);
    let task = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "copilot completion".into());

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    let config = abp_core::RuntimeConfig {
        model: Some(request.model.clone()),
        ..Default::default()
    };
    builder = builder.config(config);

    builder.build()
}

// ── Conversion: Receipt → CopilotResponse ───────────────────────────────

/// Build a [`CopilotResponse`] from a [`Receipt`] and the original model name.
pub fn receipt_to_response(receipt: &Receipt, _model: &str) -> CopilotResponse {
    let mut message = String::new();
    let mut errors = Vec::new();
    let mut function_call: Option<CopilotFunctionCall> = None;

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                message = text.clone();
            }
            AgentEventKind::AssistantDelta { text } => {
                message.push_str(text);
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                function_call = Some(CopilotFunctionCall {
                    name: tool_name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                    id: tool_use_id.clone(),
                });
            }
            AgentEventKind::Error {
                message: msg,
                error_code,
            } => {
                errors.push(CopilotError {
                    error_type: "backend_error".into(),
                    message: msg.clone(),
                    code: error_code.as_ref().map(|c| c.to_string()),
                    identifier: None,
                });
            }
            _ => {}
        }
    }

    CopilotResponse {
        message,
        copilot_references: vec![],
        copilot_errors: errors,
        copilot_confirmation: None,
        function_call,
    }
}

/// Build [`CopilotStreamEvent`]s from a sequence of [`AgentEvent`]s.
pub fn events_to_stream_events(events: &[AgentEvent], _model: &str) -> Vec<CopilotStreamEvent> {
    let mut stream_events = Vec::new();

    // Initial references event (empty)
    stream_events.push(CopilotStreamEvent::CopilotReferences { references: vec![] });

    for event in events {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                stream_events.push(CopilotStreamEvent::TextDelta { text: text.clone() });
            }
            AgentEventKind::AssistantMessage { text } => {
                stream_events.push(CopilotStreamEvent::TextDelta { text: text.clone() });
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                stream_events.push(CopilotStreamEvent::FunctionCall {
                    function_call: CopilotFunctionCall {
                        name: tool_name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                        id: tool_use_id.clone(),
                    },
                });
            }
            AgentEventKind::Error { message, .. } => {
                stream_events.push(CopilotStreamEvent::CopilotErrors {
                    errors: vec![CopilotError {
                        error_type: "backend_error".into(),
                        message: message.clone(),
                        code: None,
                        identifier: None,
                    }],
                });
            }
            _ => {}
        }
    }

    // Final done event
    stream_events.push(CopilotStreamEvent::Done {});

    stream_events
}

/// Convert a [`CopilotResponse`] into an [`IrConversation`].
pub fn response_to_ir(response: &CopilotResponse) -> IrConversation {
    if response.message.is_empty() {
        return IrConversation::from_messages(vec![]);
    }
    let msgs = vec![CopilotMessage {
        role: "assistant".into(),
        content: response.message.clone(),
        name: None,
        copilot_references: response.copilot_references.clone(),
    }];
    lowering::to_ir(&msgs)
}

/// Convert an [`IrConversation`] back to shim [`Message`]s.
pub fn ir_to_messages(conv: &IrConversation) -> Vec<Message> {
    let copilot_msgs = lowering::from_ir(conv);
    copilot_msgs
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: m.content,
            name: m.name,
            copilot_references: m.copilot_references,
        })
        .collect()
}

/// Convert shim [`Message`]s to an [`IrConversation`].
pub fn messages_to_ir(messages: &[Message]) -> IrConversation {
    let copilot_msgs: Vec<CopilotMessage> = messages
        .iter()
        .map(|m| to_copilot_message(m.clone()))
        .collect();
    lowering::to_ir(&copilot_msgs)
}

/// Convert an [`IrUsage`] to a simple usage tuple (input, output, total).
pub fn ir_usage_to_tuple(ir: &IrUsage) -> (u64, u64, u64) {
    (ir.input_tokens, ir.output_tokens, ir.total_tokens)
}

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
