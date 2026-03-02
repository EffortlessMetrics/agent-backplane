// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-shim-openai
//!
//! Drop-in OpenAI SDK shim that routes through ABP's intermediate representation.

use std::pin::Pin;

use abp_core::ir::{IrConversation, IrRole, IrToolDefinition, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Receipt, UsageNormalized, WorkOrder, WorkOrderBuilder};
use abp_openai_sdk::lowering;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

// Re-export key types from the OpenAI SDK for convenience.
pub use abp_openai_sdk::dialect::{
    OpenAIFunctionCall, OpenAIFunctionDef, OpenAIToolCall, OpenAIToolDef, ToolChoice,
    ToolChoiceFunctionRef, ToolChoiceMode,
};
pub use abp_openai_sdk::response_format::ResponseFormat;

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

// ── Message types ───────────────────────────────────────────────────────

/// Role of a message author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// System prompt.
    System,
    /// User message.
    User,
    /// Assistant response.
    Assistant,
    /// Tool result.
    Tool,
}

/// A chat message in the OpenAI format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: Role,
    /// Text content of the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls requested by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ID of the tool call this message responds to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Create a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with tool calls.
    #[must_use]
    pub fn assistant_with_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    #[must_use]
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

// ── Tool types ──────────────────────────────────────────────────────────

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: FunctionCall,
}

/// The function invocation inside a [`ToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

/// A tool definition for function calling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tool {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition payload.
    pub function: FunctionDef,
}

/// Function definition inside a [`Tool`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

impl Tool {
    /// Create a function tool definition.
    #[must_use]
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".into(),
            function: FunctionDef {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

// ── Request / Response types ────────────────────────────────────────────

/// A chat completion request matching the OpenAI API surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model identifier.
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// Controls which tool the model should call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Sampling temperature (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Whether to stream the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Response format constraint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

impl ChatCompletionRequest {
    /// Create a new builder for a chat completion request.
    #[must_use]
    pub fn builder() -> ChatCompletionRequestBuilder {
        ChatCompletionRequestBuilder::default()
    }
}

/// Builder for [`ChatCompletionRequest`].
#[derive(Debug, Default)]
pub struct ChatCompletionRequestBuilder {
    model: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<Tool>>,
    tool_choice: Option<ToolChoice>,
    temperature: Option<f64>,
    max_tokens: Option<u32>,
    stop: Option<Vec<String>>,
    stream: Option<bool>,
    response_format: Option<ResponseFormat>,
}

impl ChatCompletionRequestBuilder {
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
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tool choice.
    #[must_use]
    pub fn tool_choice(mut self, tc: ToolChoice) -> Self {
        self.tool_choice = Some(tc);
        self
    }

    /// Set the temperature.
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the max tokens.
    #[must_use]
    pub fn max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    /// Set the stop sequences.
    #[must_use]
    pub fn stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }

    /// Set the stream flag.
    #[must_use]
    pub fn stream(mut self, stream: bool) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Set the response format.
    #[must_use]
    pub fn response_format(mut self, rf: ResponseFormat) -> Self {
        self.response_format = Some(rf);
        self
    }

    /// Build the request, defaulting model to `"gpt-4o"` if unset.
    #[must_use]
    pub fn build(self) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: self.model.unwrap_or_else(|| "gpt-4o".into()),
            messages: self.messages,
            tools: self.tools,
            tool_choice: self.tool_choice,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            stop: self.stop,
            stream: self.stream,
            response_format: self.response_format,
        }
    }
}

/// A chat completion response matching the OpenAI API surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (`"chat.completion"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model used.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<Choice>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single choice in the completion response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: Message,
    /// Reason the model stopped (`"stop"`, `"tool_calls"`, etc.).
    pub finish_reason: Option<String>,
}

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

// ── Streaming types ─────────────────────────────────────────────────────

/// A streaming event from a chat completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Unique chunk identifier.
    pub id: String,
    /// Object type (`"chat.completion.chunk"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model used.
    pub model: String,
    /// Streaming choices.
    pub choices: Vec<StreamChoice>,
    /// Usage (only on final chunk if requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single choice inside a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChoice {
    /// Zero-based index.
    pub index: u32,
    /// The incremental delta.
    pub delta: Delta,
    /// Finish reason (present on final chunk).
    pub finish_reason: Option<String>,
}

/// Delta payload inside a streaming choice.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Delta {
    /// Role (only in the first chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Incremental text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

/// A tool call fragment inside a streaming delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamToolCall {
    /// Index of the tool call in the array.
    pub index: u32,
    /// Tool call ID (first fragment only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Call type (first fragment only).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// Incremental function call data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<StreamFunctionCall>,
}

/// Incremental function call data inside a streaming tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamFunctionCall {
    /// Function name (first fragment only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ── Conversion: shim types → IR ─────────────────────────────────────────

/// Convert shim [`Message`]s to the ABP lowering format.
fn to_openai_messages(messages: &[Message]) -> Vec<abp_openai_sdk::dialect::OpenAIMessage> {
    messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            let tool_calls = m.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| OpenAIToolCall {
                        id: tc.id.clone(),
                        call_type: tc.call_type.clone(),
                        function: OpenAIFunctionCall {
                            name: tc.function.name.clone(),
                            arguments: tc.function.arguments.clone(),
                        },
                    })
                    .collect()
            });
            abp_openai_sdk::dialect::OpenAIMessage {
                role: role.to_string(),
                content: m.content.clone(),
                tool_calls,
                tool_call_id: m.tool_call_id.clone(),
            }
        })
        .collect()
}

/// Convert shim [`Tool`]s to IR tool definitions.
pub fn tools_to_ir(tools: &[Tool]) -> Vec<IrToolDefinition> {
    tools
        .iter()
        .map(|t| IrToolDefinition {
            name: t.function.name.clone(),
            description: t.function.description.clone(),
            parameters: t.function.parameters.clone(),
        })
        .collect()
}

/// Convert a [`ChatCompletionRequest`] into an [`IrConversation`].
pub fn request_to_ir(request: &ChatCompletionRequest) -> IrConversation {
    let openai_msgs = to_openai_messages(&request.messages);
    lowering::to_ir(&openai_msgs)
}

/// Convert a [`ChatCompletionRequest`] into an ABP [`WorkOrder`].
pub fn request_to_work_order(request: &ChatCompletionRequest) -> WorkOrder {
    let conv = request_to_ir(request);
    let task = extract_task_from_conversation(&conv);

    let mut builder = WorkOrderBuilder::new(task).model(request.model.clone());

    let mut vendor = std::collections::BTreeMap::new();
    if let Some(temp) = request.temperature {
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
    }
    if let Some(max) = request.max_tokens {
        vendor.insert("max_tokens".to_string(), serde_json::Value::from(max));
    }
    if let Some(stop) = &request.stop {
        vendor.insert(
            "stop".to_string(),
            serde_json::to_value(stop).unwrap_or_default(),
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

/// Extract a task description from the conversation (last user message or first user message).
fn extract_task_from_conversation(conv: &IrConversation) -> String {
    // Use the last user message as the task
    conv.messages
        .iter()
        .rev()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_else(|| "chat completion".into())
}

/// Build a [`ChatCompletionResponse`] from a [`Receipt`] and the original model name.
pub fn receipt_to_response(receipt: &Receipt, model: &str) -> ChatCompletionResponse {
    let mut content: Option<String> = None;
    let mut tool_calls: Vec<ToolCall> = Vec::new();
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
                tool_calls.push(ToolCall {
                    id: tool_use_id
                        .clone()
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                    call_type: "function".into(),
                    function: FunctionCall {
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

    let message = Message {
        role: Role::Assistant,
        content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_call_id: None,
    };

    let usage = usage_from_receipt(&receipt.usage);

    ChatCompletionResponse {
        id: format!("chatcmpl-{}", receipt.meta.run_id),
        object: "chat.completion".into(),
        created: receipt.meta.started_at.timestamp() as u64,
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message,
            finish_reason: Some(finish_reason),
        }],
        usage: Some(usage),
    }
}

/// Convert normalized usage to OpenAI-style usage.
fn usage_from_receipt(usage: &UsageNormalized) -> Usage {
    let prompt = usage.input_tokens.unwrap_or(0);
    let completion = usage.output_tokens.unwrap_or(0);
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
    }
}

/// Build [`StreamEvent`]s from a sequence of [`AgentEvent`]s.
pub fn events_to_stream_events(events: &[AgentEvent], model: &str) -> Vec<StreamEvent> {
    let run_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = Utc::now().timestamp() as u64;
    let mut stream_events = Vec::new();

    for event in events {
        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                stream_events.push(StreamEvent {
                    id: run_id.clone(),
                    object: "chat.completion.chunk".into(),
                    created,
                    model: model.to_string(),
                    choices: vec![StreamChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                });
            }
            AgentEventKind::AssistantMessage { text } => {
                stream_events.push(StreamEvent {
                    id: run_id.clone(),
                    object: "chat.completion.chunk".into(),
                    created,
                    model: model.to_string(),
                    choices: vec![StreamChoice {
                        index: 0,
                        delta: Delta {
                            role: Some("assistant".into()),
                            content: Some(text.clone()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                });
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                stream_events.push(StreamEvent {
                    id: run_id.clone(),
                    object: "chat.completion.chunk".into(),
                    created,
                    model: model.to_string(),
                    choices: vec![StreamChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: None,
                            tool_calls: Some(vec![StreamToolCall {
                                index: 0,
                                id: tool_use_id.clone(),
                                call_type: Some("function".into()),
                                function: Some(StreamFunctionCall {
                                    name: Some(tool_name.clone()),
                                    arguments: Some(
                                        serde_json::to_string(input).unwrap_or_default(),
                                    ),
                                }),
                            }]),
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                });
            }
            _ => {}
        }
    }

    // Final stop chunk
    stream_events.push(StreamEvent {
        id: run_id,
        object: "chat.completion.chunk".into(),
        created,
        model: model.to_string(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    });

    stream_events
}

/// Convert an [`IrConversation`] back to shim [`Message`]s.
pub fn ir_to_messages(conv: &IrConversation) -> Vec<Message> {
    let openai_msgs = lowering::from_ir(conv);
    openai_msgs
        .into_iter()
        .map(|m| {
            let role = match m.role.as_str() {
                "system" => Role::System,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            };
            let tool_calls = m.tool_calls.map(|tcs| {
                tcs.into_iter()
                    .map(|tc| ToolCall {
                        id: tc.id,
                        call_type: tc.call_type,
                        function: FunctionCall {
                            name: tc.function.name,
                            arguments: tc.function.arguments,
                        },
                    })
                    .collect()
            });
            Message {
                role,
                content: m.content,
                tool_calls,
                tool_call_id: m.tool_call_id,
            }
        })
        .collect()
}

/// Convert shim [`Message`]s to an [`IrConversation`].
pub fn messages_to_ir(messages: &[Message]) -> IrConversation {
    let openai_msgs = to_openai_messages(messages);
    lowering::to_ir(&openai_msgs)
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

/// Drop-in compatible OpenAI client that routes through ABP.
pub struct OpenAiClient {
    model: String,
    processor: Option<ProcessFn>,
}

impl std::fmt::Debug for OpenAiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiClient")
            .field("model", &self.model)
            .finish()
    }
}

impl OpenAiClient {
    /// Create a new client targeting the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            processor: None,
        }
    }

    /// Set a custom processor function for handling work orders.
    ///
    /// This is used for testing and custom routing.
    #[must_use]
    pub fn with_processor(mut self, processor: ProcessFn) -> Self {
        self.processor = Some(processor);
        self
    }

    /// Access the chat completions API.
    pub fn chat(&self) -> ChatApi<'_> {
        ChatApi { client: self }
    }

    /// Get the configured model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
}

/// Chat API namespace (mirrors `client.chat`).
pub struct ChatApi<'a> {
    client: &'a OpenAiClient,
}

impl<'a> ChatApi<'a> {
    /// Access the completions endpoint.
    pub fn completions(&self) -> CompletionsApi<'a> {
        CompletionsApi {
            client: self.client,
        }
    }
}

/// Completions API namespace (mirrors `client.chat.completions`).
pub struct CompletionsApi<'a> {
    client: &'a OpenAiClient,
}

impl<'a> CompletionsApi<'a> {
    /// Create a chat completion (non-streaming).
    ///
    /// Converts the request to IR, then to a WorkOrder, processes it,
    /// and converts the receipt back into a ChatCompletionResponse.
    pub async fn create(&self, request: ChatCompletionRequest) -> Result<ChatCompletionResponse> {
        let work_order = request_to_work_order(&request);

        let receipt = if let Some(processor) = &self.client.processor {
            processor(&work_order)
        } else {
            return Err(ShimError::Internal(
                "no processor configured; use with_processor() to set a backend".into(),
            ));
        };

        Ok(receipt_to_response(&receipt, &request.model))
    }

    /// Create a streaming chat completion.
    ///
    /// Returns a stream of [`StreamEvent`]s.
    pub async fn create_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>> {
        let work_order = request_to_work_order(&request);
        let model = request.model.clone();

        let receipt = if let Some(processor) = &self.client.processor {
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
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "gpt-4o");
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
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .stream(true)
            .build();

        let stream = client
            .chat()
            .completions()
            .create_stream(req)
            .await
            .unwrap();
        let chunks: Vec<StreamEvent> = stream.collect().await;
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
                tool_name: "get_weather".into(),
                tool_use_id: Some("call_abc123".into()),
                parent_tool_use_id: None,
                input: json!({"location": "San Francisco"}),
            },
            ext: None,
        }];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("What's the weather?")])
            .tools(vec![Tool::function(
                "get_weather",
                "Get weather for a location",
                json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    }
                }),
            )])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc123");
        assert_eq!(tcs[0].function.name, "get_weather");
        assert!(tcs[0].function.arguments.contains("San Francisco"));
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
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![
                Message::system("You are a helpful assistant."),
                Message::user("Hello"),
            ])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
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
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![
                Message::user("What is 2+2?"),
                Message::assistant("Let me calculate..."),
                Message::user("Please give me just the number"),
            ])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("4"));
    }

    // ── 6. Temperature mapping ──────────────────────────────────────────

    #[test]
    fn temperature_mapped_to_work_order() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
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
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .max_tokens(1024)
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(
            wo.config.vendor.get("max_tokens"),
            Some(&serde_json::Value::from(1024))
        );
    }

    // ── 8. Stop sequences mapping ───────────────────────────────────────

    #[test]
    fn stop_sequences_mapped_to_work_order() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .stop(vec!["END".into(), "STOP".into()])
            .build();

        let wo = request_to_work_order(&req);
        let stop = wo.config.vendor.get("stop").unwrap();
        assert_eq!(stop, &json!(["END", "STOP"]));
    }

    // ── 9. Model name preservation ──────────────────────────────────────

    #[tokio::test]
    async fn model_name_preserved_in_response() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let client = OpenAiClient::new("gpt-4-turbo").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(resp.model, "gpt-4-turbo");
    }

    // ── 10. Error response ──────────────────────────────────────────────

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
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit exceeded"));
    }

    // ── 11. Token usage tracking ────────────────────────────────────────

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
        let client =
            OpenAiClient::new("gpt-4o").with_processor(make_processor_with_usage(events, usage));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    // ── 12. Request to IR roundtrip ─────────────────────────────────────

    #[test]
    fn request_to_ir_roundtrip() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::system("Be concise."), Message::user("Hello")])
            .build();

        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[1].text_content(), "Hello");
    }

    // ── 13. Messages to IR and back ─────────────────────────────────────

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
        assert_eq!(back[0].role, Role::System);
        assert_eq!(back[0].content.as_deref(), Some("System prompt"));
        assert_eq!(back[1].role, Role::User);
        assert_eq!(back[2].role, Role::Assistant);
    }

    // ── 14. Tool call IR roundtrip ──────────────────────────────────────

    #[test]
    fn tool_call_ir_roundtrip() {
        let messages = vec![Message::assistant_with_tool_calls(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }])];

        let conv = messages_to_ir(&messages);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert!(!conv.messages[0].tool_use_blocks().is_empty());

        let back = ir_to_messages(&conv);
        let tc = &back[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_1");
        assert_eq!(tc.function.name, "read_file");
    }

    // ── 15. Tool result IR roundtrip ────────────────────────────────────

    #[test]
    fn tool_result_ir_roundtrip() {
        let messages = vec![Message::tool("call_1", "file contents here")];

        let conv = messages_to_ir(&messages);
        assert_eq!(conv.messages[0].role, IrRole::Tool);

        let back = ir_to_messages(&conv);
        assert_eq!(back[0].role, Role::Tool);
        assert_eq!(back[0].content.as_deref(), Some("file contents here"));
        assert_eq!(back[0].tool_call_id.as_deref(), Some("call_1"));
    }

    // ── 16. No processor returns error ──────────────────────────────────

    #[tokio::test]
    async fn no_processor_returns_error() {
        let client = OpenAiClient::new("gpt-4o");
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("test")])
            .build();

        let err = client.chat().completions().create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── 17. Builder defaults model ──────────────────────────────────────

    #[test]
    fn builder_defaults_model() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("test")])
            .build();

        assert_eq!(req.model, "gpt-4o");
    }

    // ── 18. Stream events include stop chunk ────────────────────────────

    #[test]
    fn stream_events_end_with_stop() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        }];

        let stream = events_to_stream_events(&events, "gpt-4o");
        assert_eq!(stream.len(), 2);
        assert_eq!(
            stream.last().unwrap().choices[0].finish_reason.as_deref(),
            Some("stop")
        );
    }

    // ── 19. Tool definitions to IR ──────────────────────────────────────

    #[test]
    fn tools_convert_to_ir() {
        let tools = vec![Tool::function(
            "search",
            "Search the web",
            json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        )];

        let ir = tools_to_ir(&tools);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir[0].name, "search");
        assert_eq!(ir[0].description, "Search the web");
    }

    // ── 20. IR usage conversion ─────────────────────────────────────────

    #[test]
    fn ir_usage_converts_correctly() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    // ── 21. Request to work order model mapping ─────────────────────────

    #[test]
    fn request_model_maps_to_work_order() {
        let req = ChatCompletionRequest::builder()
            .model("o3-mini")
            .messages(vec![Message::user("test")])
            .build();

        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }

    // ── 22. Multi-tool calls in response ────────────────────────────────

    #[tokio::test]
    async fn multi_tool_calls_in_response() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("call_1".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "a.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("call_2".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "b.rs"}),
                },
                ext: None,
            },
        ];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Read files")])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].id, "call_1");
        assert_eq!(tcs[1].id, "call_2");
    }

    // ── 23. Streaming tool call event ───────────────────────────────────

    #[tokio::test]
    async fn streaming_tool_call() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
                tool_use_id: Some("call_s1".into()),
                parent_tool_use_id: None,
                input: json!({"q": "rust"}),
            },
            ext: None,
        }];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("search")])
            .stream(true)
            .build();

        let stream = client
            .chat()
            .completions()
            .create_stream(req)
            .await
            .unwrap();
        let chunks: Vec<StreamEvent> = stream.collect().await;
        // tool call chunk + stop chunk
        assert_eq!(chunks.len(), 2);
        let tc = &chunks[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("search")
        );
    }

    // ── 24. Empty messages produce valid IR ─────────────────────────────

    #[test]
    fn empty_messages_produce_empty_ir() {
        let conv = messages_to_ir(&[]);
        assert!(conv.is_empty());
    }

    // ── 25. Message constructors ────────────────────────────────────────

    #[test]
    fn message_constructors() {
        let sys = Message::system("sys");
        assert_eq!(sys.role, Role::System);
        assert_eq!(sys.content.as_deref(), Some("sys"));

        let user = Message::user("usr");
        assert_eq!(user.role, Role::User);

        let asst = Message::assistant("asst");
        assert_eq!(asst.role, Role::Assistant);

        let tool = Message::tool("id1", "result");
        assert_eq!(tool.role, Role::Tool);
        assert_eq!(tool.tool_call_id.as_deref(), Some("id1"));
    }

    // ── 26. Response format serialization ───────────────────────────────

    #[test]
    fn response_format_in_request() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("test")])
            .response_format(ResponseFormat::json_object())
            .build();

        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("response_format").is_some());
    }

    // ── 27. Chat completion response ID format ──────────────────────────

    #[tokio::test]
    async fn response_id_format() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "ok".into() },
            ext: None,
        }];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("test")])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        assert!(resp.id.starts_with("chatcmpl-"));
    }

    // ── 28. Assistant message with text and tool calls ──────────────────

    #[tokio::test]
    async fn assistant_text_and_tool_calls() {
        let events = vec![
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Let me check.".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "ls".into(),
                    tool_use_id: Some("call_ls".into()),
                    parent_tool_use_id: None,
                    input: json!({}),
                },
                ext: None,
            },
        ];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("list files")])
            .build();

        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Let me check.")
        );
        assert!(resp.choices[0].message.tool_calls.is_some());
    }
}
