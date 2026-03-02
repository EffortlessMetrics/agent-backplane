// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-shim-kimi
//!
//! Drop-in Kimi SDK shim that routes through ABP's intermediate representation.

use std::pin::Pin;

use abp_core::ir::{IrConversation, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Receipt, UsageNormalized, WorkOrder, WorkOrderBuilder};
use abp_kimi_sdk::dialect::{
    KimiChoice, KimiChunk, KimiChunkChoice, KimiChunkDelta, KimiFunctionCall, KimiMessage,
    KimiRequest, KimiResponse, KimiResponseMessage, KimiToolCall, KimiUsage,
};
use abp_kimi_sdk::lowering;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

// Re-export key types from the Kimi SDK for convenience.
pub use abp_kimi_sdk::dialect::{
    KimiBuiltinFunction, KimiBuiltinTool, KimiFunctionDef, KimiRole, KimiTool, KimiToolDef,
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

// ── Message constructors ────────────────────────────────────────────────

/// A chat message in the Kimi format (convenience wrapper).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: String,
    /// Text content of the message.
    pub content: Option<String>,
    /// Tool calls (assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiToolCall>>,
    /// Tool call ID this message responds to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Create a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with tool calls.
    #[must_use]
    pub fn assistant_with_tool_calls(tool_calls: Vec<KimiToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    #[must_use]
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ── Request builder ─────────────────────────────────────────────────────

/// Builder for [`KimiRequest`].
#[derive(Debug, Default)]
pub struct KimiRequestBuilder {
    model: Option<String>,
    messages: Vec<Message>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    stream: Option<bool>,
    tools: Option<Vec<KimiTool>>,
    use_search: Option<bool>,
}

impl KimiRequestBuilder {
    /// Create a new builder for a Kimi request.
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

    /// Set the maximum tokens.
    #[must_use]
    pub fn max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    /// Set the temperature.
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the stream flag.
    #[must_use]
    pub fn stream(mut self, stream: bool) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Set the tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<KimiTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the use_search flag.
    #[must_use]
    pub fn use_search(mut self, use_search: bool) -> Self {
        self.use_search = Some(use_search);
        self
    }

    /// Build the request, defaulting model to `"moonshot-v1-8k"` if unset.
    #[must_use]
    pub fn build(self) -> KimiRequest {
        KimiRequest {
            model: self.model.unwrap_or_else(|| "moonshot-v1-8k".into()),
            messages: self.messages.into_iter().map(to_kimi_message).collect(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            stream: self.stream,
            tools: self.tools,
            use_search: self.use_search,
        }
    }
}

/// Convert a shim [`Message`] to a [`KimiMessage`].
fn to_kimi_message(msg: Message) -> KimiMessage {
    KimiMessage {
        role: msg.role,
        content: msg.content,
        tool_call_id: msg.tool_call_id,
        tool_calls: msg.tool_calls,
    }
}

// ── Conversion: request → IR → WorkOrder ────────────────────────────────

/// Convert a [`KimiRequest`] into an [`IrConversation`].
pub fn request_to_ir(request: &KimiRequest) -> IrConversation {
    lowering::to_ir(&request.messages)
}

/// Convert a [`KimiRequest`] into an ABP [`WorkOrder`].
pub fn request_to_work_order(request: &KimiRequest) -> WorkOrder {
    let conv = request_to_ir(request);
    let task = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "kimi completion".into());

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    let mut vendor = std::collections::BTreeMap::new();
    if let Some(temp) = request.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(max) = request.max_tokens {
        vendor.insert("max_tokens".to_string(), serde_json::Value::from(max));
    }
    let config = abp_core::RuntimeConfig {
        model: Some(request.model.clone()),
        vendor,
        ..Default::default()
    };
    builder = builder.config(config);

    builder.build()
}

// ── Conversion: Receipt → KimiResponse ──────────────────────────────────

/// Build a [`KimiResponse`] from a [`Receipt`] and the original model name.
pub fn receipt_to_response(receipt: &Receipt, model: &str) -> KimiResponse {
    let mut content: Option<String> = None;
    let mut tool_calls: Vec<KimiToolCall> = Vec::new();
    let mut finish_reason = "stop".to_string();

    for event in &receipt.trace {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                content = Some(text.clone());
            }
            AgentEventKind::AssistantDelta { text } => {
                let c = content.get_or_insert_with(String::new);
                c.push_str(text);
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                tool_calls.push(KimiToolCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: tool_name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
                finish_reason = "tool_calls".to_string();
            }
            AgentEventKind::Error { message, .. } => {
                content = Some(format!("Error: {message}"));
                finish_reason = "stop".to_string();
            }
            _ => {}
        }
    }

    let message = KimiResponseMessage {
        role: "assistant".into(),
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
    };

    let usage = usage_from_receipt(&receipt.usage);

    KimiResponse {
        id: format!("cmpl-{}", receipt.meta.run_id),
        model: model.to_string(),
        choices: vec![KimiChoice {
            index: 0,
            message,
            finish_reason: Some(finish_reason),
        }],
        usage: Some(usage),
        refs: None,
    }
}

/// Convert normalized usage to Kimi-style usage.
fn usage_from_receipt(usage: &UsageNormalized) -> KimiUsage {
    let prompt = usage.input_tokens.unwrap_or(0);
    let completion = usage.output_tokens.unwrap_or(0);
    KimiUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
    }
}

/// Build [`KimiChunk`]s from a sequence of [`AgentEvent`]s.
pub fn events_to_stream_chunks(events: &[AgentEvent], model: &str) -> Vec<KimiChunk> {
    let run_id = format!("cmpl-{}", uuid::Uuid::new_v4());
    let created = Utc::now().timestamp() as u64;
    let mut chunks = Vec::new();

    for event in events {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                chunks.push(KimiChunk {
                    id: run_id.clone(),
                    object: "chat.completion.chunk".into(),
                    created,
                    model: model.to_string(),
                    choices: vec![KimiChunkChoice {
                        index: 0,
                        delta: KimiChunkDelta {
                            role: None,
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                    refs: None,
                });
            }
            AgentEventKind::AssistantMessage { text } => {
                chunks.push(KimiChunk {
                    id: run_id.clone(),
                    object: "chat.completion.chunk".into(),
                    created,
                    model: model.to_string(),
                    choices: vec![KimiChunkChoice {
                        index: 0,
                        delta: KimiChunkDelta {
                            role: Some("assistant".into()),
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                    refs: None,
                });
            }
            _ => {}
        }
    }

    // Final stop chunk
    chunks.push(KimiChunk {
        id: run_id,
        object: "chat.completion.chunk".into(),
        created,
        model: model.to_string(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    });

    chunks
}

/// Convert a [`KimiResponse`] into an [`IrConversation`].
pub fn response_to_ir(response: &KimiResponse) -> IrConversation {
    let msgs: Vec<KimiMessage> = response
        .choices
        .iter()
        .map(|c| KimiMessage {
            role: c.message.role.clone(),
            content: c.message.content.clone(),
            tool_call_id: None,
            tool_calls: c.message.tool_calls.clone(),
        })
        .collect();
    lowering::to_ir(&msgs)
}

/// Convert an [`IrConversation`] back to shim [`Message`]s.
pub fn ir_to_messages(conv: &IrConversation) -> Vec<Message> {
    let kimi_msgs = lowering::from_ir(conv);
    kimi_msgs
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: m.content,
            tool_calls: m.tool_calls,
            tool_call_id: m.tool_call_id,
        })
        .collect()
}

/// Convert shim [`Message`]s to an [`IrConversation`].
pub fn messages_to_ir(messages: &[Message]) -> IrConversation {
    let kimi_msgs: Vec<KimiMessage> = messages
        .iter()
        .map(|m| to_kimi_message(m.clone()))
        .collect();
    lowering::to_ir(&kimi_msgs)
}

/// Convert an [`IrUsage`] to shim [`Usage`].
pub fn ir_usage_to_usage(ir: &IrUsage) -> Usage {
    Usage {
        prompt_tokens: ir.input_tokens,
        completion_tokens: ir.output_tokens,
        total_tokens: ir.total_tokens,
    }
}

// ── Client types ────────────────────────────────────────────────────────

/// A callback function that processes a [`WorkOrder`] and returns a [`Receipt`].
pub type ProcessFn = Box<dyn Fn(&WorkOrder) -> Receipt + Send + Sync>;

/// Drop-in compatible Kimi client that routes through ABP.
pub struct KimiClient {
    model: String,
    processor: Option<ProcessFn>,
}

impl std::fmt::Debug for KimiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KimiClient")
            .field("model", &self.model)
            .finish()
    }
}

impl KimiClient {
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

    /// Create a chat completion (non-streaming).
    pub async fn create(&self, request: KimiRequest) -> Result<KimiResponse> {
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

    /// Create a streaming chat completion.
    pub async fn create_stream(
        &self,
        request: KimiRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = KimiChunk> + Send>>> {
        let work_order = request_to_work_order(&request);
        let model = request.model.clone();

        let receipt = if let Some(processor) = &self.processor {
            processor(&work_order)
        } else {
            return Err(ShimError::Internal(
                "no processor configured; use with_processor() to set a backend".into(),
            ));
        };

        let chunks = events_to_stream_chunks(&receipt.trace, &model);
        Ok(Box::pin(tokio_stream::iter(chunks)))
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

    // ── 1. Simple chat completion roundtrip ─────────────────────────────

    #[tokio::test]
    async fn simple_chat_completion() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello!".into(),
            },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("Hi")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 2. Streaming chat completion ────────────────────────────────────

    #[tokio::test]
    async fn streaming_chat_completion() {
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
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Hi")])
            .stream(true)
            .build();

        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<KimiChunk> = stream.collect().await;
        // 2 deltas + 1 final stop chunk
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hel"));
        assert_eq!(chunks[1].choices[0].delta.content.as_deref(), Some("lo!"));
        assert_eq!(chunks[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 3. Tool use / function calling ──────────────────────────────────

    #[tokio::test]
    async fn tool_use_function_calling() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "web_search".into(),
                tool_use_id: Some("call_abc".into()),
                parent_tool_use_id: None,
                input: json!({"query": "rust async"}),
            },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Search for rust async")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(tcs[0].function.name, "web_search");
        assert!(tcs[0].function.arguments.contains("rust async"));
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
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![
                Message::system("You are a helpful assistant."),
                Message::user("Hello"),
            ])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("I am helpful.")
        );
    }

    // ── 5. Multi-turn conversation ──────────────────────────────────────

    #[tokio::test]
    async fn multi_turn_conversation() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "4".into() },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![
                Message::user("What is 2+2?"),
                Message::assistant("Let me calculate..."),
                Message::user("Just the number please"),
            ])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("4"));
    }

    // ── 6. Temperature mapping ──────────────────────────────────────────

    #[test]
    fn temperature_mapped_to_work_order() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .temperature(0.7)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("temperature"),
            Some(&serde_json::Value::from(0.7))
        );
    }

    // ── 7. Max tokens mapping ───────────────────────────────────────────

    #[test]
    fn max_tokens_mapped_to_work_order() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .max_tokens(1024)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::Value::from(1024))
        );
    }

    // ── 8. Model name preservation ──────────────────────────────────────

    #[tokio::test]
    async fn model_name_preserved_in_response() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let client = KimiClient::new("moonshot-v1-128k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "moonshot-v1-128k");
    }

    // ── 9. Error response ───────────────────────────────────────────────

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
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.create(req).await.unwrap();
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit exceeded"));
    }

    // ── 10. Token usage tracking ────────────────────────────────────────

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
        let client = KimiClient::new("moonshot-v1-8k")
            .with_processor(make_processor_with_usage(events, usage));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.create(req).await.unwrap();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    // ── 11. Request to IR roundtrip ─────────────────────────────────────

    #[test]
    fn request_to_ir_roundtrip() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::system("Be concise."), Message::user("Hello")])
            .build();

        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[1].text_content(), "Hello");
    }

    // ── 12. Messages to IR and back ─────────────────────────────────────

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
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    // ── 13. No processor returns error ──────────────────────────────────

    #[tokio::test]
    async fn no_processor_returns_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── 14. Builder defaults model ──────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        assert_eq!(req.model, "moonshot-v1-8k");
    }

    // ── 15. Stream chunks end with stop ─────────────────────────────────

    #[test]
    fn stream_chunks_end_with_stop() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        }];

        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    // ── 16. IR usage conversion ─────────────────────────────────────────

    #[test]
    fn ir_usage_converts_correctly() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── 17. Request model maps to work order ────────────────────────────

    #[test]
    fn request_model_maps_to_work_order() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-128k")
            .messages(vec![Message::user("test")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    // ── 18. Multi-tool calls in response ────────────────────────────────

    #[tokio::test]
    async fn multi_tool_calls_in_response() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: Some("call_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"q": "a"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "search".into(),
                    tool_use_id: Some("call_2".into()),
                    parent_tool_use_id: None,
                    input: json!({"q": "b"}),
                },
                ext: None,
            },
        ];
        let client = KimiClient::new("moonshot-v1-8k").with_processor(make_processor(events));
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("Search")])
            .build();

        let resp = client.create(req).await.unwrap();
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[1].id, "call_2");
    }

    // ── 19. No processor stream returns error ───────────────────────────

    #[tokio::test]
    async fn no_processor_stream_returns_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = KimiRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();

        let result = client.create_stream(req).await;
        assert!(result.is_err());
    }
}
