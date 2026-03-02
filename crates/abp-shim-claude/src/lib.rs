// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
//! Drop-in Anthropic Claude SDK shim that routes through ABP.
#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::pin::Pin;
use std::task::{Context, Poll};

use abp_claude_sdk::dialect::{
    self, ClaudeConfig, ClaudeContentBlock, ClaudeImageSource, ClaudeMessage, ClaudeResponse,
    ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage, ThinkingConfig,
};
use abp_core::{AgentEvent, AgentEventKind, WorkOrderBuilder};
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by the shim client.
#[derive(Debug, thiserror::Error)]
pub enum ShimError {
    /// Request validation failed.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// Backend returned an error.
    #[error("api error ({error_type}): {message}")]
    ApiError {
        /// Error type identifier.
        error_type: String,
        /// Human-readable message.
        message: String,
    },
    /// Internal conversion failure.
    #[error("internal: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// Public request/response types (mirrors Anthropic SDK)
// ---------------------------------------------------------------------------

/// Role for a message in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// User message.
    User,
    /// Assistant message.
    Assistant,
}

/// A content block within a message — mirrors Anthropic content block types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text {
        /// The text payload.
        text: String,
    },
    /// A tool invocation from the assistant.
    ToolUse {
        /// Unique tool use identifier.
        id: String,
        /// Tool name.
        name: String,
        /// JSON input for the tool.
        input: serde_json::Value,
    },
    /// Result of a tool invocation.
    ToolResult {
        /// Correlating tool use ID.
        tool_use_id: String,
        /// Text content of the result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Whether the tool produced an error.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// Extended thinking block.
    Thinking {
        /// The model's internal reasoning.
        thinking: String,
        /// Cryptographic signature.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// Image content block.
    Image {
        /// Image source.
        source: ImageSource,
    },
}

/// Image source for image content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64-encoded image data.
    Base64 {
        /// MIME type.
        media_type: String,
        /// Base64-encoded bytes.
        data: String,
    },
    /// URL-referenced image.
    Url {
        /// The image URL.
        url: String,
    },
}

/// A message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: Role,
    /// Content blocks.
    pub content: Vec<ContentBlock>,
}

/// Request to the messages API — mirrors `POST /v1/messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRequest {
    /// Model identifier (e.g. `"claude-sonnet-4-20250514"`).
    pub model: String,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Optional system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Optional temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Optional stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Extended thinking configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// Whether to stream the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// Token usage in a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens generated.
    pub output_tokens: u64,
    /// Tokens written to cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    /// Tokens read from cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

/// Response from the messages API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageResponse {
    /// Unique message identifier.
    pub id: String,
    /// Object type (always `"message"`).
    #[serde(rename = "type")]
    pub response_type: String,
    /// Role (always `"assistant"`).
    pub role: String,
    /// Content blocks.
    pub content: Vec<ContentBlock>,
    /// Model that generated the response.
    pub model: String,
    /// Reason the model stopped.
    pub stop_reason: Option<String>,
    /// Stop sequence that triggered the stop.
    pub stop_sequence: Option<String>,
    /// Token usage.
    pub usage: Usage,
}

/// A streaming event from the messages API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Stream has started — contains initial message metadata.
    MessageStart {
        /// Initial (incomplete) response.
        message: MessageResponse,
    },
    /// A content block has begun.
    ContentBlockStart {
        /// Block index.
        index: u32,
        /// Initial block value.
        content_block: ContentBlock,
    },
    /// Incremental update to a content block.
    ContentBlockDelta {
        /// Block index.
        index: u32,
        /// Delta payload.
        delta: StreamDelta,
    },
    /// A content block has finished.
    ContentBlockStop {
        /// Block index.
        index: u32,
    },
    /// Message-level metadata update.
    MessageDelta {
        /// Delta with stop_reason etc.
        delta: MessageDeltaPayload,
        /// Updated usage.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    /// Stream has ended.
    MessageStop {},
    /// Keep-alive.
    Ping {},
    /// Error during streaming.
    Error {
        /// Error details.
        error: ApiError,
    },
}

/// Delta payload for content block updates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamDelta {
    /// Incremental text.
    TextDelta {
        /// Text fragment.
        text: String,
    },
    /// Incremental tool input JSON.
    InputJsonDelta {
        /// Partial JSON string.
        partial_json: String,
    },
    /// Incremental thinking text.
    ThinkingDelta {
        /// Thinking fragment.
        thinking: String,
    },
    /// Incremental signature.
    SignatureDelta {
        /// Signature fragment.
        signature: String,
    },
}

/// Message-level delta (stop reason, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageDeltaPayload {
    /// Reason the model stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Stop sequence that triggered the stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// API error information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiError {
    /// Error type identifier.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable message.
    pub message: String,
}

// ---------------------------------------------------------------------------
// Conversion: Shim types ↔ Claude SDK types
// ---------------------------------------------------------------------------

/// Convert a shim `ContentBlock` to the Claude SDK `ClaudeContentBlock`.
#[must_use]
pub fn content_block_to_ir(block: &ContentBlock) -> ClaudeContentBlock {
    match block {
        ContentBlock::Text { text } => ClaudeContentBlock::Text { text: text.clone() },
        ContentBlock::ToolUse { id, name, input } => ClaudeContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => ClaudeContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
        },
        ContentBlock::Thinking {
            thinking,
            signature,
        } => ClaudeContentBlock::Thinking {
            thinking: thinking.clone(),
            signature: signature.clone(),
        },
        ContentBlock::Image { source } => ClaudeContentBlock::Image {
            source: image_source_to_ir(source),
        },
    }
}

/// Convert a Claude SDK `ClaudeContentBlock` to the shim `ContentBlock`.
#[must_use]
pub fn content_block_from_ir(block: &ClaudeContentBlock) -> ContentBlock {
    match block {
        ClaudeContentBlock::Text { text } => ContentBlock::Text { text: text.clone() },
        ClaudeContentBlock::ToolUse { id, name, input } => ContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => ContentBlock::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
        },
        ClaudeContentBlock::Thinking {
            thinking,
            signature,
        } => ContentBlock::Thinking {
            thinking: thinking.clone(),
            signature: signature.clone(),
        },
        ClaudeContentBlock::Image { source } => ContentBlock::Image {
            source: image_source_from_ir(source),
        },
    }
}

fn image_source_to_ir(source: &ImageSource) -> ClaudeImageSource {
    match source {
        ImageSource::Base64 { media_type, data } => ClaudeImageSource::Base64 {
            media_type: media_type.clone(),
            data: data.clone(),
        },
        ImageSource::Url { url } => ClaudeImageSource::Url { url: url.clone() },
    }
}

fn image_source_from_ir(source: &ClaudeImageSource) -> ImageSource {
    match source {
        ClaudeImageSource::Base64 { media_type, data } => ImageSource::Base64 {
            media_type: media_type.clone(),
            data: data.clone(),
        },
        ClaudeImageSource::Url { url } => ImageSource::Url { url: url.clone() },
    }
}

/// Convert a shim `Message` to a Claude SDK `ClaudeMessage`.
#[must_use]
pub fn message_to_ir(msg: &Message) -> ClaudeMessage {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };

    let has_structured = msg
        .content
        .iter()
        .any(|b| !matches!(b, ContentBlock::Text { .. }));

    if has_structured || msg.content.len() > 1 {
        let blocks: Vec<ClaudeContentBlock> = msg.content.iter().map(content_block_to_ir).collect();
        let content = serde_json::to_string(&blocks).unwrap_or_default();
        ClaudeMessage {
            role: role.to_string(),
            content,
        }
    } else {
        let text = msg.content.first().map_or(String::new(), |b| match b {
            ContentBlock::Text { text } => text.clone(),
            _ => serde_json::to_string(&[content_block_to_ir(b)]).unwrap_or_default(),
        });
        ClaudeMessage {
            role: role.to_string(),
            content: text,
        }
    }
}

/// Convert a shim `MessageRequest` to a Claude SDK `ClaudeRequest`.
#[must_use]
pub fn request_to_claude(req: &MessageRequest) -> abp_claude_sdk::dialect::ClaudeRequest {
    let messages: Vec<ClaudeMessage> = req.messages.iter().map(message_to_ir).collect();
    abp_claude_sdk::dialect::ClaudeRequest {
        model: req.model.clone(),
        max_tokens: req.max_tokens,
        system: req.system.clone(),
        messages,
        thinking: req.thinking.clone(),
    }
}

/// Convert a Claude SDK `ClaudeResponse` to a shim `MessageResponse`.
#[must_use]
pub fn response_from_claude(resp: &ClaudeResponse) -> MessageResponse {
    let content: Vec<ContentBlock> = resp.content.iter().map(content_block_from_ir).collect();
    let usage = resp.usage.as_ref().map_or(
        Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        |u| Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_creation_input_tokens: u.cache_creation_input_tokens,
            cache_read_input_tokens: u.cache_read_input_tokens,
        },
    );

    MessageResponse {
        id: resp.id.clone(),
        response_type: "message".to_string(),
        role: resp.role.clone(),
        content,
        model: resp.model.clone(),
        stop_reason: resp.stop_reason.clone(),
        stop_sequence: None,
        usage,
    }
}

/// Convert a Claude SDK `ClaudeStreamEvent` to a shim `StreamEvent`.
#[must_use]
pub fn stream_event_from_claude(event: &ClaudeStreamEvent) -> StreamEvent {
    match event {
        ClaudeStreamEvent::MessageStart { message } => StreamEvent::MessageStart {
            message: response_from_claude(message),
        },
        ClaudeStreamEvent::ContentBlockStart {
            index,
            content_block,
        } => StreamEvent::ContentBlockStart {
            index: *index,
            content_block: content_block_from_ir(content_block),
        },
        ClaudeStreamEvent::ContentBlockDelta { index, delta } => StreamEvent::ContentBlockDelta {
            index: *index,
            delta: stream_delta_from_claude(delta),
        },
        ClaudeStreamEvent::ContentBlockStop { index } => {
            StreamEvent::ContentBlockStop { index: *index }
        }
        ClaudeStreamEvent::MessageDelta { delta, usage } => StreamEvent::MessageDelta {
            delta: MessageDeltaPayload {
                stop_reason: delta.stop_reason.clone(),
                stop_sequence: delta.stop_sequence.clone(),
            },
            usage: usage.as_ref().map(|u| Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_creation_input_tokens: u.cache_creation_input_tokens,
                cache_read_input_tokens: u.cache_read_input_tokens,
            }),
        },
        ClaudeStreamEvent::MessageStop {} => StreamEvent::MessageStop {},
        ClaudeStreamEvent::Ping {} => StreamEvent::Ping {},
        ClaudeStreamEvent::Error { error } => StreamEvent::Error {
            error: ApiError {
                error_type: error.error_type.clone(),
                message: error.message.clone(),
            },
        },
    }
}

fn stream_delta_from_claude(delta: &ClaudeStreamDelta) -> StreamDelta {
    match delta {
        ClaudeStreamDelta::TextDelta { text } => StreamDelta::TextDelta { text: text.clone() },
        ClaudeStreamDelta::InputJsonDelta { partial_json } => StreamDelta::InputJsonDelta {
            partial_json: partial_json.clone(),
        },
        ClaudeStreamDelta::ThinkingDelta { thinking } => StreamDelta::ThinkingDelta {
            thinking: thinking.clone(),
        },
        ClaudeStreamDelta::SignatureDelta { signature } => StreamDelta::SignatureDelta {
            signature: signature.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// ABP pipeline: request → WorkOrder → Receipt → response
// ---------------------------------------------------------------------------

/// Convert a `MessageRequest` into an ABP `WorkOrder`.
#[must_use]
pub fn request_to_work_order(req: &MessageRequest) -> abp_core::WorkOrder {
    let mut builder = WorkOrderBuilder::new(
        req.messages
            .last()
            .and_then(|m| {
                m.content.iter().find_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
            })
            .unwrap_or_else(|| "Claude shim request".to_string()),
    )
    .model(&req.model);

    if let Some(temp) = req.temperature {
        let mut vendor = std::collections::BTreeMap::new();
        vendor.insert("temperature".to_string(), serde_json::Value::from(temp));
        if let Some(ref stop) = req.stop_sequences {
            vendor.insert(
                "stop_sequences".to_string(),
                serde_json::to_value(stop).unwrap_or_default(),
            );
        }
        vendor.insert(
            "max_tokens".to_string(),
            serde_json::Value::from(req.max_tokens),
        );
        let config = abp_core::RuntimeConfig {
            model: Some(req.model.clone()),
            vendor,
            ..Default::default()
        };
        builder = builder.config(config);
    }

    builder.build()
}

/// Synthesize a `MessageResponse` from ABP agent events (mock pipeline).
///
/// In a full implementation, the runtime would execute the work order against
/// a real backend. This function builds a response from a list of events
/// such as those returned by `dialect::map_response`.
#[must_use]
pub fn response_from_events(
    events: &[AgentEvent],
    model: &str,
    usage: Option<&ClaudeUsage>,
) -> MessageResponse {
    let mut content = Vec::new();
    let mut stop_reason = None;

    for event in events {
        match &event.kind {
            AgentEventKind::AssistantMessage { text } => {
                let is_thinking = event
                    .ext
                    .as_ref()
                    .and_then(|e| e.get("thinking"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if is_thinking {
                    let signature = event
                        .ext
                        .as_ref()
                        .and_then(|e| e.get("signature"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    content.push(ContentBlock::Thinking {
                        thinking: text.clone(),
                        signature,
                    });
                } else {
                    content.push(ContentBlock::Text { text: text.clone() });
                }
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                content.push(ContentBlock::ToolUse {
                    id: tool_use_id.clone().unwrap_or_default(),
                    name: tool_name.clone(),
                    input: input.clone(),
                });
                stop_reason = Some("tool_use".to_string());
            }
            AgentEventKind::RunCompleted { .. } if stop_reason.is_none() => {
                stop_reason = Some("end_turn".to_string());
            }
            _ => {}
        }
    }

    if stop_reason.is_none() && !content.is_empty() {
        stop_reason = Some("end_turn".to_string());
    }

    let usage_val = usage.map_or(
        Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        |u| Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_creation_input_tokens: u.cache_creation_input_tokens,
            cache_read_input_tokens: u.cache_read_input_tokens,
        },
    );

    MessageResponse {
        id: format!("msg_{}", uuid::Uuid::new_v4().as_simple()),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: model.to_string(),
        stop_reason,
        stop_sequence: None,
        usage: usage_val,
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Callback type for processing requests through a custom pipeline.
///
/// The function receives a `MessageRequest` and returns a `MessageResponse`.
pub type RequestHandler =
    Box<dyn Fn(&MessageRequest) -> Result<MessageResponse, ShimError> + Send + Sync>;

/// Callback for streaming requests.
pub type StreamHandler =
    Box<dyn Fn(&MessageRequest) -> Result<Vec<StreamEvent>, ShimError> + Send + Sync>;

/// Drop-in-compatible Anthropic client backed by ABP.
///
/// By default, uses a mock pipeline that converts through the Claude SDK
/// dialect types. A custom `RequestHandler` can be installed for real backend
/// integration.
pub struct AnthropicClient {
    model: String,
    max_tokens: u32,
    handler: Option<RequestHandler>,
    stream_handler: Option<StreamHandler>,
}

impl std::fmt::Debug for AnthropicClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicClient")
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl Default for AnthropicClient {
    fn default() -> Self {
        Self {
            model: dialect::DEFAULT_MODEL.to_string(),
            max_tokens: 4096,
            handler: None,
            stream_handler: None,
        }
    }
}

impl AnthropicClient {
    /// Create a new client with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a client with a specific model.
    #[must_use]
    pub fn with_model(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Self::default()
        }
    }

    /// Set a custom request handler for non-streaming requests.
    pub fn set_handler(&mut self, handler: RequestHandler) {
        self.handler = Some(handler);
    }

    /// Set a custom stream handler.
    pub fn set_stream_handler(&mut self, handler: StreamHandler) {
        self.stream_handler = Some(handler);
    }

    /// Non-streaming message creation — mirrors `client.messages.create(...)`.
    ///
    /// Converts the request through ABP's Claude dialect, runs a mock
    /// pipeline, and returns the response.
    ///
    /// # Errors
    ///
    /// Returns `ShimError` if the request is invalid or the pipeline fails.
    pub async fn create(&self, request: MessageRequest) -> Result<MessageResponse, ShimError> {
        if request.messages.is_empty() {
            return Err(ShimError::InvalidRequest(
                "messages must not be empty".into(),
            ));
        }

        if let Some(ref handler) = self.handler {
            return handler(&request);
        }

        // Default mock pipeline:
        // 1. Convert to Claude SDK request
        let claude_req = request_to_claude(&request);

        // 2. Build a config and map to WorkOrder
        let config = ClaudeConfig {
            model: request.model.clone(),
            max_tokens: request.max_tokens,
            system_prompt: request.system.clone(),
            thinking: request.thinking.clone(),
            ..ClaudeConfig::default()
        };
        let _work_order = dialect::map_work_order(&request_to_work_order(&request), &config);

        // 3. Simulate a response via the dialect layer
        let claude_resp = ClaudeResponse {
            id: format!("msg_{}", uuid::Uuid::new_v4().as_simple()),
            model: claude_req.model.clone(),
            role: "assistant".to_string(),
            content: vec![ClaudeContentBlock::Text {
                text: format!(
                    "Mock response to: {}",
                    claude_req
                        .messages
                        .last()
                        .map(|m| m.content.as_str())
                        .unwrap_or("(empty)")
                ),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: Some(ClaudeUsage {
                input_tokens: 10,
                output_tokens: 25,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };

        // 4. Map through ABP events and back
        let events = dialect::map_response(&claude_resp);
        let resp = response_from_events(&events, &claude_resp.model, claude_resp.usage.as_ref());

        Ok(resp)
    }

    /// Streaming message creation — mirrors `client.messages.stream(...)`.
    ///
    /// Returns a `Pin<Box<dyn Stream<Item = StreamEvent>>>` that yields
    /// streaming events in the canonical Anthropic order.
    ///
    /// # Errors
    ///
    /// Returns `ShimError` if the request is invalid.
    pub async fn create_stream(&self, request: MessageRequest) -> Result<EventStream, ShimError> {
        if request.messages.is_empty() {
            return Err(ShimError::InvalidRequest(
                "messages must not be empty".into(),
            ));
        }

        if let Some(ref handler) = self.stream_handler {
            let events = handler(&request)?;
            return Ok(EventStream::from_vec(events));
        }

        // Default mock streaming pipeline
        let claude_req = request_to_claude(&request);
        let response_text = format!(
            "Mock streamed response to: {}",
            claude_req
                .messages
                .last()
                .map(|m| m.content.as_str())
                .unwrap_or("(empty)")
        );

        let model = claude_req.model.clone();
        let msg_resp = MessageResponse {
            id: format!("msg_{}", uuid::Uuid::new_v4().as_simple()),
            response_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![],
            model: model.clone(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };

        let events = vec![
            StreamEvent::MessageStart { message: msg_resp },
            StreamEvent::Ping {},
            StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ContentBlock::Text {
                    text: String::new(),
                },
            },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta {
                    text: response_text,
                },
            },
            StreamEvent::ContentBlockStop { index: 0 },
            StreamEvent::MessageDelta {
                delta: MessageDeltaPayload {
                    stop_reason: Some("end_turn".to_string()),
                    stop_sequence: None,
                },
                usage: Some(Usage {
                    input_tokens: 10,
                    output_tokens: 25,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
            StreamEvent::MessageStop {},
        ];

        Ok(EventStream::from_vec(events))
    }
}

// ---------------------------------------------------------------------------
// EventStream — a simple Stream adapter
// ---------------------------------------------------------------------------

/// A stream of `StreamEvent` items.
#[derive(Debug)]
pub struct EventStream {
    events: Vec<StreamEvent>,
    index: usize,
}

impl EventStream {
    /// Create from a pre-built event list.
    #[must_use]
    pub fn from_vec(events: Vec<StreamEvent>) -> Self {
        Self { events, index: 0 }
    }

    /// Collect all remaining events.
    pub async fn collect_all(mut self) -> Vec<StreamEvent> {
        use tokio_stream::StreamExt;
        let mut out = Vec::new();
        while let Some(event) = StreamExt::next(&mut self).await {
            out.push(event);
        }
        out
    }
}

impl Stream for EventStream {
    type Item = StreamEvent;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.index < self.events.len() {
            let event = self.events[self.index].clone();
            self.index += 1;
            Poll::Ready(Some(event))
        } else {
            Poll::Ready(None)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.events.len() - self.index;
        (remaining, Some(remaining))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_claude_sdk::dialect::ClaudeApiError;
    use chrono::Utc;
    use serde_json::json;

    // Helper: simple user message request
    fn simple_request(text: &str) -> MessageRequest {
        MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: text.to_string(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        }
    }

    // ── 1. Simple message roundtrip ─────────────────────────────────────

    #[tokio::test]
    async fn simple_message_roundtrip() {
        let client = AnthropicClient::new();
        let resp = client.create(simple_request("Hello")).await.unwrap();

        assert_eq!(resp.response_type, "message");
        assert_eq!(resp.role, "assistant");
        assert!(!resp.content.is_empty());
        assert!(resp.stop_reason.is_some());
    }

    #[tokio::test]
    async fn response_contains_text_block() {
        let client = AnthropicClient::new();
        let resp = client.create(simple_request("Hi")).await.unwrap();

        let has_text = resp
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { .. }));
        assert!(has_text);
    }

    // ── 2. Extended thinking blocks ─────────────────────────────────────

    #[test]
    fn thinking_content_block_serde_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "Let me consider...".to_string(),
            signature: Some("sig_abc".to_string()),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[tokio::test]
    async fn thinking_blocks_in_response() {
        let mut client = AnthropicClient::new();
        client.set_handler(Box::new(|req| {
            let events = vec![
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantMessage {
                        text: "Reasoning about the question...".into(),
                    },
                    ext: Some({
                        let mut m = std::collections::BTreeMap::new();
                        m.insert("thinking".into(), serde_json::Value::Bool(true));
                        m
                    }),
                },
                AgentEvent {
                    ts: Utc::now(),
                    kind: AgentEventKind::AssistantMessage {
                        text: "The answer is 42.".into(),
                    },
                    ext: None,
                },
            ];
            Ok(response_from_events(&events, &req.model, None))
        }));

        let mut req = simple_request("What is the meaning of life?");
        req.thinking = Some(ThinkingConfig::new(1024));

        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(&resp.content[0], ContentBlock::Thinking { .. }));
        assert!(matches!(&resp.content[1], ContentBlock::Text { .. }));
    }

    // ── 3. Tool use with content blocks ─────────────────────────────────

    #[test]
    fn tool_use_content_block_conversion() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".to_string(),
            name: "read_file".to_string(),
            input: json!({"path": "src/main.rs"}),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn tool_result_content_block_conversion() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: Some("file contents here".to_string()),
            is_error: Some(false),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[tokio::test]
    async fn tool_use_response_via_handler() {
        let mut client = AnthropicClient::new();
        client.set_handler(Box::new(|req| {
            let events = vec![AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tu_123".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "lib.rs"}),
                },
                ext: None,
            }];
            Ok(response_from_events(&events, &req.model, None))
        }));

        let resp = client
            .create(simple_request("Read the file"))
            .await
            .unwrap();
        assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
        match &resp.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_123");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "lib.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    // ── 4. Image input handling ─────────────────────────────────────────

    #[test]
    fn image_base64_content_block_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".to_string(),
                data: "iVBORw0KGgo=".to_string(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn image_url_content_block_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/image.png".to_string(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(block, back);
    }

    #[test]
    fn image_message_to_claude() {
        let msg = Message {
            role: Role::User,
            content: vec![
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type: "image/jpeg".into(),
                        data: "base64data".into(),
                    },
                },
                ContentBlock::Text {
                    text: "What is this?".into(),
                },
            ],
        };
        let claude_msg = message_to_ir(&msg);
        assert_eq!(claude_msg.role, "user");
        // Structured content is serialized as JSON
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msg.content).unwrap();
        assert_eq!(blocks.len(), 2);
    }

    // ── 5. System message (top-level vs in-message) ─────────────────────

    #[test]
    fn system_prompt_in_request() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".to_string(),
                }],
            }],
            system: Some("You are a helpful assistant.".to_string()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let claude_req = request_to_claude(&req);
        assert_eq!(
            claude_req.system.as_deref(),
            Some("You are a helpful assistant.")
        );
        // System should NOT be in messages
        assert_eq!(claude_req.messages.len(), 1);
        assert_eq!(claude_req.messages[0].role, "user");
    }

    #[test]
    fn system_prompt_maps_to_work_order() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Help me".to_string(),
                }],
            }],
            system: Some("Be concise.".to_string()),
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = request_to_work_order(&req);
        assert!(wo.task.contains("Help me"));
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    // ── 6. Multi-turn conversation ──────────────────────────────────────

    #[test]
    fn multi_turn_conversion() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text { text: "Hi".into() }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text {
                        text: "Hello!".into(),
                    }],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "How are you?".into(),
                    }],
                },
            ],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let claude_req = request_to_claude(&req);
        assert_eq!(claude_req.messages.len(), 3);
        assert_eq!(claude_req.messages[0].role, "user");
        assert_eq!(claude_req.messages[1].role, "assistant");
        assert_eq!(claude_req.messages[2].role, "user");
    }

    #[tokio::test]
    async fn multi_turn_roundtrip() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "What is 2+2?".into(),
                    }],
                },
                Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text { text: "4".into() }],
                },
                Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "And 3+3?".into(),
                    }],
                },
            ],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.role, "assistant");
    }

    // ── 7. Temperature, max_tokens, stop sequences ──────────────────────

    #[test]
    fn temperature_preserved_in_request() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 1024,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "test".into(),
                }],
            }],
            system: None,
            temperature: Some(0.7),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["temperature"], 0.7);
        assert_eq!(json["max_tokens"], 1024);
    }

    #[test]
    fn stop_sequences_preserved() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "test".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: Some(vec!["STOP".to_string(), "END".to_string()]),
            thinking: None,
            stream: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        let stops = json["stop_sequences"].as_array().unwrap();
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0], "STOP");
    }

    #[test]
    fn max_tokens_in_work_order_vendor() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 2048,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "test".into(),
                }],
            }],
            system: None,
            temperature: Some(0.5),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo = request_to_work_order(&req);
        let max_tok = wo.config.vendor.get("max_tokens");
        assert_eq!(max_tok, Some(&serde_json::Value::from(2048)));
    }

    // ── 8. Streaming event sequence ─────────────────────────────────────

    #[tokio::test]
    async fn streaming_event_sequence() {
        let client = AnthropicClient::new();
        let stream = client.create_stream(simple_request("Hello")).await.unwrap();
        let events = stream.collect_all().await;

        assert!(events.len() >= 5);
        assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
        assert!(matches!(
            events.last().unwrap(),
            StreamEvent::MessageStop {}
        ));

        // Verify canonical ordering
        let has_content_start = events
            .iter()
            .any(|e| matches!(e, StreamEvent::ContentBlockStart { .. }));
        let has_content_delta = events
            .iter()
            .any(|e| matches!(e, StreamEvent::ContentBlockDelta { .. }));
        let has_content_stop = events
            .iter()
            .any(|e| matches!(e, StreamEvent::ContentBlockStop { .. }));
        assert!(has_content_start);
        assert!(has_content_delta);
        assert!(has_content_stop);
    }

    #[tokio::test]
    async fn streaming_text_delta_content() {
        let client = AnthropicClient::new();
        let stream = client.create_stream(simple_request("Test")).await.unwrap();
        let events = stream.collect_all().await;

        let text_delta = events.iter().find_map(|e| match e {
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::TextDelta { text },
                ..
            } => Some(text.clone()),
            _ => None,
        });
        assert!(text_delta.is_some());
        assert!(!text_delta.unwrap().is_empty());
    }

    #[tokio::test]
    async fn streaming_message_delta_has_stop_reason() {
        let client = AnthropicClient::new();
        let stream = client.create_stream(simple_request("Test")).await.unwrap();
        let events = stream.collect_all().await;

        let msg_delta = events.iter().find_map(|e| match e {
            StreamEvent::MessageDelta { delta, .. } => Some(delta.clone()),
            _ => None,
        });
        assert!(msg_delta.is_some());
        assert_eq!(msg_delta.unwrap().stop_reason.as_deref(), Some("end_turn"));
    }

    // ── 9. Token usage in response ──────────────────────────────────────

    #[tokio::test]
    async fn usage_in_non_streaming_response() {
        let client = AnthropicClient::new();
        let resp = client.create(simple_request("Hi")).await.unwrap();

        assert!(resp.usage.input_tokens > 0 || resp.usage.output_tokens > 0);
    }

    #[test]
    fn usage_serde_roundtrip() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 250,
            cache_creation_input_tokens: Some(50),
            cache_read_input_tokens: Some(30),
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
    }

    #[test]
    fn usage_from_claude_usage() {
        let claude_usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(20),
        };
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "test".into(),
            },
            ext: None,
        }];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", Some(&claude_usage));
        assert_eq!(resp.usage.input_tokens, 100);
        assert_eq!(resp.usage.output_tokens, 200);
        assert_eq!(resp.usage.cache_creation_input_tokens, Some(10));
        assert_eq!(resp.usage.cache_read_input_tokens, Some(20));
    }

    // ── 10. Model name preservation ─────────────────────────────────────

    #[tokio::test]
    async fn model_name_preserved() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-opus-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.model, "claude-opus-4-20250514");
    }

    #[test]
    fn model_name_in_request_serde() {
        let req = simple_request("test");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "claude-sonnet-4-20250514");
    }

    // ── 11. Error responses ─────────────────────────────────────────────

    #[tokio::test]
    async fn empty_messages_returns_error() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn empty_messages_stream_returns_error() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let err = client.create_stream(req).await.unwrap_err();
        assert!(matches!(err, ShimError::InvalidRequest(_)));
    }

    #[test]
    fn api_error_serde() {
        let err = ApiError {
            error_type: "invalid_request_error".to_string(),
            message: "max_tokens must be positive".to_string(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn error_stream_event() {
        let event = StreamEvent::Error {
            error: ApiError {
                error_type: "overloaded_error".into(),
                message: "Server busy".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[tokio::test]
    async fn custom_handler_error() {
        let mut client = AnthropicClient::new();
        client.set_handler(Box::new(|_| {
            Err(ShimError::ApiError {
                error_type: "rate_limit_error".into(),
                message: "Too many requests".into(),
            })
        }));
        let err = client.create(simple_request("test")).await.unwrap_err();
        match err {
            ShimError::ApiError { error_type, .. } => {
                assert_eq!(error_type, "rate_limit_error");
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    // ── Additional coverage ─────────────────────────────────────────────

    #[test]
    fn message_request_serde_roundtrip() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("Be helpful".into()),
            temperature: Some(0.5),
            stop_sequences: Some(vec!["END".into()]),
            thinking: Some(ThinkingConfig::new(2048)),
            stream: Some(true),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: MessageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, req.model);
        assert_eq!(back.max_tokens, req.max_tokens);
        assert_eq!(back.system, req.system);
        assert_eq!(back.temperature, req.temperature);
        assert_eq!(back.stop_sequences, req.stop_sequences);
    }

    #[test]
    fn message_response_serde_roundtrip() {
        let resp = MessageResponse {
            id: "msg_abc".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Hello!".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: MessageResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_event_serde_roundtrip() {
        let events = vec![
            StreamEvent::Ping {},
            StreamEvent::MessageStop {},
            StreamEvent::ContentBlockStop { index: 0 },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta { text: "hi".into() },
            },
        ];
        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let back: StreamEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(*event, back);
        }
    }

    #[test]
    fn claude_stream_event_conversion() {
        let claude_events = vec![
            ClaudeStreamEvent::Ping {},
            ClaudeStreamEvent::MessageStop {},
            ClaudeStreamEvent::ContentBlockStop { index: 1 },
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::TextDelta {
                    text: "hello".into(),
                },
            },
            ClaudeStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ClaudeStreamDelta::ThinkingDelta {
                    thinking: "hmm".into(),
                },
            },
            ClaudeStreamEvent::Error {
                error: ClaudeApiError {
                    error_type: "test".into(),
                    message: "test error".into(),
                },
            },
        ];
        for ce in &claude_events {
            let se = stream_event_from_claude(ce);
            // Just verify it doesn't panic and produces a valid event
            let json = serde_json::to_string(&se).unwrap();
            assert!(!json.is_empty());
        }
    }

    #[test]
    fn client_debug_impl() {
        let client = AnthropicClient::new();
        let dbg = format!("{client:?}");
        assert!(dbg.contains("AnthropicClient"));
        assert!(dbg.contains("claude"));
    }

    #[test]
    fn client_with_model() {
        let client = AnthropicClient::with_model("claude-opus-4-20250514");
        let dbg = format!("{client:?}");
        assert!(dbg.contains("claude-opus-4-20250514"));
    }

    #[tokio::test]
    async fn event_stream_size_hint() {
        let stream = EventStream::from_vec(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}]);
        assert_eq!(stream.size_hint(), (2, Some(2)));
    }

    #[test]
    fn response_from_events_end_turn_default() {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "done".into(),
            },
            ext: None,
        }];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn response_from_events_empty() {
        let resp = response_from_events(&[], "test-model", None);
        assert!(resp.content.is_empty());
        assert!(resp.stop_reason.is_none());
    }
}
