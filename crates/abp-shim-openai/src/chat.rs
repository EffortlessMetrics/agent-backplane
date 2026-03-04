// SPDX-License-Identifier: MIT OR Apache-2.0
//! Chat completion request/response types and builder API.
//!
//! Provides a builder-pattern API for constructing chat completion requests
//! that matches the ergonomics of the official OpenAI Python/Node SDKs, along
//! with the response and streaming chunk types.

use std::pin::Pin;

use futures_core::Stream;

use crate::error::ApiError;

// Re-use the core types already defined in lib.rs / types.rs:
use crate::types::{
    ChatCompletionRequest as WireChatCompletionRequest,
    ChatCompletionResponse as WireChatCompletionResponse, ChatMessage, StreamChunk, Tool, ToolCall,
    ToolChoice,
};

// ── Chat completion chunk (streaming) ───────────────────────────────────

/// A single streaming chunk from a chat completion.
///
/// This is a type alias for the wire-format [`StreamChunk`] that mirrors
/// what the OpenAI API returns for each SSE `data:` line.
pub type ChatCompletionChunk = StreamChunk;

/// A typed stream of chat completion chunks.
pub type ChatCompletionStream =
    Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk, ApiError>> + Send>>;

// ── Builder ─────────────────────────────────────────────────────────────

/// Ergonomic builder for chat completion requests.
///
/// # Example
///
/// ```rust
/// use abp_shim_openai::chat::ChatRequestBuilder;
/// use abp_shim_openai::types::ChatMessage;
///
/// let request = ChatRequestBuilder::new("gpt-4o")
///     .system("You are a helpful assistant.")
///     .user("Hello!")
///     .temperature(0.7)
///     .max_tokens(1024)
///     .build();
///
/// assert_eq!(request.model, "gpt-4o");
/// assert_eq!(request.messages.len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct ChatRequestBuilder {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    max_tokens: Option<u32>,
    stream: Option<bool>,
    tools: Option<Vec<Tool>>,
    tool_choice: Option<ToolChoice>,
}

impl ChatRequestBuilder {
    /// Create a new builder targeting the given model.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: Vec::new(),
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
        }
    }

    /// Append a system message.
    #[must_use]
    pub fn system(mut self, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::System {
            content: content.into(),
        });
        self
    }

    /// Append a user message (text).
    #[must_use]
    pub fn user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::User {
            content: crate::types::MessageContent::Text(content.into()),
        });
        self
    }

    /// Append an assistant message.
    #[must_use]
    pub fn assistant(mut self, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::Assistant {
            content: Some(content.into()),
            tool_calls: None,
        });
        self
    }

    /// Append an assistant message with tool calls.
    #[must_use]
    pub fn assistant_with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.messages.push(ChatMessage::Assistant {
            content: None,
            tool_calls: Some(tool_calls),
        });
        self
    }

    /// Append a tool result message.
    #[must_use]
    pub fn tool_result(mut self, tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::Tool {
            content: content.into(),
            tool_call_id: tool_call_id.into(),
        });
        self
    }

    /// Append a raw [`ChatMessage`].
    #[must_use]
    pub fn message(mut self, msg: ChatMessage) -> Self {
        self.messages.push(msg);
        self
    }

    /// Set all messages at once.
    #[must_use]
    pub fn messages(mut self, msgs: Vec<ChatMessage>) -> Self {
        self.messages = msgs;
        self
    }

    /// Set the sampling temperature (0.0–2.0).
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the nucleus sampling parameter.
    #[must_use]
    pub fn top_p(mut self, p: f64) -> Self {
        self.top_p = Some(p);
        self
    }

    /// Set the maximum number of tokens to generate.
    #[must_use]
    pub fn max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    /// Enable or disable streaming.
    #[must_use]
    pub fn stream(mut self, enabled: bool) -> Self {
        self.stream = Some(enabled);
        self
    }

    /// Set the tool definitions.
    #[must_use]
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tool choice strategy.
    #[must_use]
    pub fn tool_choice(mut self, tc: ToolChoice) -> Self {
        self.tool_choice = Some(tc);
        self
    }

    /// Build the wire-format [`WireChatCompletionRequest`].
    #[must_use]
    pub fn build(self) -> WireChatCompletionRequest {
        WireChatCompletionRequest {
            model: self.model,
            messages: self.messages,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            stream: self.stream,
            tools: self.tools,
            tool_choice: self.tool_choice,
        }
    }
}

// ── Convenience type aliases ────────────────────────────────────────────

/// Wire-format chat completion request.
pub type ChatCompletionRequest = WireChatCompletionRequest;

/// Wire-format chat completion response.
pub type ChatCompletionResponse = WireChatCompletionResponse;

// ── Response helpers ────────────────────────────────────────────────────

/// Extract the first choice's text content from a completion response.
#[must_use]
pub fn first_content(resp: &WireChatCompletionResponse) -> Option<&str> {
    resp.choices
        .first()
        .and_then(|c| c.message.content.as_deref())
}

/// Extract the first choice's finish reason.
#[must_use]
pub fn first_finish_reason(resp: &WireChatCompletionResponse) -> Option<&str> {
    resp.choices
        .first()
        .and_then(|c| c.finish_reason.as_deref())
}

/// Collect streaming chunk deltas into a complete text string.
pub fn collect_stream_text(chunks: &[ChatCompletionChunk]) -> String {
    let mut text = String::new();
    for chunk in chunks {
        for choice in &chunk.choices {
            if let Some(content) = &choice.delta.content {
                text.push_str(content);
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ChatMessage, Choice as WireChoice, ChoiceMessage, FunctionCall,
        StreamChoice as WireStreamChoice, StreamDelta, ToolChoiceMode,
    };

    #[test]
    fn builder_basic_construction() {
        let req = ChatRequestBuilder::new("gpt-4o")
            .system("Be helpful")
            .user("Hello")
            .build();

        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 2);
        assert!(matches!(&req.messages[0], ChatMessage::System { content } if content == "Be helpful"));
    }

    #[test]
    fn builder_with_all_options() {
        let req = ChatRequestBuilder::new("gpt-4o-mini")
            .user("test")
            .temperature(0.5)
            .top_p(0.9)
            .max_tokens(512)
            .stream(true)
            .build();

        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.top_p, Some(0.9));
        assert_eq!(req.max_tokens, Some(512));
        assert_eq!(req.stream, Some(true));
    }

    #[test]
    fn builder_multi_turn() {
        let req = ChatRequestBuilder::new("gpt-4o")
            .system("System prompt")
            .user("User question")
            .assistant("Response")
            .user("Follow-up")
            .build();

        assert_eq!(req.messages.len(), 4);
    }

    #[test]
    fn builder_with_tool_result() {
        let req = ChatRequestBuilder::new("gpt-4o")
            .user("Read this file")
            .assistant_with_tool_calls(vec![ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"test.rs"}"#.into(),
                },
            }])
            .tool_result("call_1", "file contents")
            .build();

        assert_eq!(req.messages.len(), 3);
        assert!(matches!(&req.messages[2], ChatMessage::Tool { tool_call_id, .. } if tool_call_id == "call_1"));
    }

    #[test]
    fn builder_raw_message() {
        let req = ChatRequestBuilder::new("gpt-4o")
            .message(ChatMessage::System {
                content: "raw".into(),
            })
            .build();
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn builder_tool_choice() {
        let req = ChatRequestBuilder::new("gpt-4o")
            .user("test")
            .tool_choice(ToolChoice::Mode(ToolChoiceMode::Auto))
            .build();
        assert!(req.tool_choice.is_some());
    }

    #[test]
    fn first_content_extracts_text() {
        let resp = WireChatCompletionResponse {
            id: "test".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "gpt-4o".into(),
            choices: vec![WireChoice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".into(),
                    content: Some("Hello!".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        };

        assert_eq!(first_content(&resp), Some("Hello!"));
        assert_eq!(first_finish_reason(&resp), Some("stop"));
    }

    #[test]
    fn first_content_none_when_empty() {
        let resp = WireChatCompletionResponse {
            id: "test".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "gpt-4o".into(),
            choices: vec![],
            usage: None,
        };

        assert_eq!(first_content(&resp), None);
        assert_eq!(first_finish_reason(&resp), None);
    }

    #[test]
    fn collect_stream_text_assembles_content() {
        let chunks = vec![
            StreamChunk {
                id: "c1".into(),
                object: "chat.completion.chunk".into(),
                created: 0,
                model: "gpt-4o".into(),
                choices: vec![WireStreamChoice {
                    index: 0,
                    delta: StreamDelta {
                        role: Some("assistant".into()),
                        content: Some("Hel".into()),
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
            },
            StreamChunk {
                id: "c1".into(),
                object: "chat.completion.chunk".into(),
                created: 0,
                model: "gpt-4o".into(),
                choices: vec![WireStreamChoice {
                    index: 0,
                    delta: StreamDelta {
                        role: None,
                        content: Some("lo!".into()),
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
            },
        ];

        assert_eq!(collect_stream_text(&chunks), "Hello!");
    }

    #[test]
    fn builder_serde_roundtrip() {
        let req = ChatRequestBuilder::new("gpt-4o")
            .system("Be concise")
            .user("Hi")
            .temperature(0.7)
            .build();

        let json = serde_json::to_string(&req).unwrap();
        let parsed: WireChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4o");
        assert_eq!(parsed.temperature, Some(0.7));
        assert_eq!(parsed.messages.len(), 2);
    }
}
