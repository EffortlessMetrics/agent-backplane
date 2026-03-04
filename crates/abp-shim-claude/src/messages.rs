// SPDX-License-Identifier: MIT OR Apache-2.0
//! `CreateMessageRequest` builder matching the Anthropic SDK surface.
//!
//! Provides a fluent builder API for constructing Claude Messages API requests,
//! mirroring the pattern used in official Anthropic client libraries.

use crate::types::{
    ClaudeContent, ClaudeMessage, ClaudeTool, ClaudeToolChoice, ContentBlock, MessagesRequest,
    ThinkingConfig,
};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Fluent builder for [`MessagesRequest`], matching the Anthropic SDK style.
///
/// # Example
///
/// ```rust
/// use abp_shim_claude::messages::CreateMessageRequest;
///
/// let request = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
///     .system("You are a helpful assistant.")
///     .user("What is the capital of France?")
///     .temperature(0.7)
///     .build();
///
/// assert_eq!(request.model, "claude-sonnet-4-20250514");
/// assert_eq!(request.max_tokens, 4096);
/// assert_eq!(request.messages.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct CreateMessageRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ClaudeMessage>,
    system: Option<String>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_k: Option<u32>,
    stream: Option<bool>,
    stop_sequences: Option<Vec<String>>,
    tools: Option<Vec<ClaudeTool>>,
    tool_choice: Option<ClaudeToolChoice>,
    thinking: Option<ThinkingConfig>,
}

impl CreateMessageRequest {
    /// Create a new builder with the required model and max_tokens.
    #[must_use]
    pub fn new(model: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            messages: Vec::new(),
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stream: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        }
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system(mut self, prompt: impl Into<String>) -> Self {
        self.system = Some(prompt.into());
        self
    }

    /// Add a user message with text content.
    #[must_use]
    pub fn user(mut self, text: impl Into<String>) -> Self {
        self.messages.push(ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeContent::Text(text.into()),
        });
        self
    }

    /// Add an assistant message with text content.
    #[must_use]
    pub fn assistant(mut self, text: impl Into<String>) -> Self {
        self.messages.push(ClaudeMessage {
            role: "assistant".to_string(),
            content: ClaudeContent::Text(text.into()),
        });
        self
    }

    /// Add a user message with structured content blocks.
    #[must_use]
    pub fn user_blocks(mut self, blocks: Vec<ContentBlock>) -> Self {
        self.messages.push(ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeContent::Blocks(blocks),
        });
        self
    }

    /// Add an assistant message with structured content blocks.
    #[must_use]
    pub fn assistant_blocks(mut self, blocks: Vec<ContentBlock>) -> Self {
        self.messages.push(ClaudeMessage {
            role: "assistant".to_string(),
            content: ClaudeContent::Blocks(blocks),
        });
        self
    }

    /// Add a raw message.
    #[must_use]
    pub fn message(mut self, msg: ClaudeMessage) -> Self {
        self.messages.push(msg);
        self
    }

    /// Set multiple messages at once, replacing any previously added messages.
    #[must_use]
    pub fn messages(mut self, msgs: Vec<ClaudeMessage>) -> Self {
        self.messages = msgs;
        self
    }

    /// Set the sampling temperature (0.0–1.0).
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the nucleus sampling probability mass.
    #[must_use]
    pub fn top_p(mut self, p: f64) -> Self {
        self.top_p = Some(p);
        self
    }

    /// Set the top-K sampling parameter.
    #[must_use]
    pub fn top_k(mut self, k: u32) -> Self {
        self.top_k = Some(k);
        self
    }

    /// Enable or disable streaming.
    #[must_use]
    pub fn stream(mut self, enable: bool) -> Self {
        self.stream = Some(enable);
        self
    }

    /// Set custom stop sequences.
    #[must_use]
    pub fn stop_sequences(mut self, seqs: Vec<String>) -> Self {
        self.stop_sequences = Some(seqs);
        self
    }

    /// Set the available tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<ClaudeTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Add a single tool definition.
    #[must_use]
    pub fn tool(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        let tool = ClaudeTool {
            name: name.into(),
            description: Some(description.into()),
            input_schema,
        };
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }

    /// Set tool choice strategy.
    #[must_use]
    pub fn tool_choice(mut self, choice: ClaudeToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Enable extended thinking with the given budget.
    #[must_use]
    pub fn thinking(mut self, budget_tokens: u32) -> Self {
        self.thinking = Some(ThinkingConfig::new(budget_tokens));
        self
    }

    /// Build the final [`MessagesRequest`].
    #[must_use]
    pub fn build(self) -> MessagesRequest {
        MessagesRequest {
            model: self.model,
            messages: self.messages,
            max_tokens: self.max_tokens,
            system: self.system,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            stream: self.stream,
            stop_sequences: self.stop_sequences,
            tools: self.tools,
            tool_choice: self.tool_choice,
            thinking: self.thinking,
        }
    }
}

// ---------------------------------------------------------------------------
// MessagesApi handle (mirrors Anthropic SDK `client.messages`)
// ---------------------------------------------------------------------------

/// Handle returned by [`AnthropicClient::messages()`][crate::client::AnthropicClient::messages]
/// that provides `create` and `stream` methods.
///
/// This mirrors the Anthropic SDK pattern:
/// ```ignore
/// let response = client.messages().create(request).await?;
/// let stream = client.messages().stream(request).await?;
/// ```
pub struct MessagesApi<'a> {
    pub(crate) client: &'a crate::client::AnthropicClient,
}

impl<'a> MessagesApi<'a> {
    /// Create a message (non-streaming).
    ///
    /// # Errors
    ///
    /// Returns an error if the request is invalid or the pipeline fails.
    pub async fn create(
        &self,
        request: &MessagesRequest,
    ) -> Result<crate::types::MessagesResponse, crate::error::ClaudeShimError> {
        self.client.create_message(request).await
    }

    /// Create a streaming message.
    ///
    /// # Errors
    ///
    /// Returns an error if the request is invalid.
    pub async fn stream(
        &self,
        request: &MessagesRequest,
    ) -> Result<crate::streaming::MessageStream, crate::error::ClaudeShimError> {
        self.client.create_stream(request).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builder_minimal() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("Hello")
            .build();
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.max_tokens, 4096);
        assert_eq!(req.messages.len(), 1);
        assert!(req.system.is_none());
        assert!(req.temperature.is_none());
    }

    #[test]
    fn builder_with_system() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 1024)
            .system("Be helpful")
            .user("Hi")
            .build();
        assert_eq!(req.system.as_deref(), Some("Be helpful"));
    }

    #[test]
    fn builder_multi_turn() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("What is 2+2?")
            .assistant("4")
            .user("And 3+3?")
            .build();
        assert_eq!(req.messages.len(), 3);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[1].role, "assistant");
        assert_eq!(req.messages[2].role, "user");
    }

    #[test]
    fn builder_with_temperature() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .temperature(0.7)
            .build();
        assert_eq!(req.temperature, Some(0.7));
    }

    #[test]
    fn builder_with_top_p_and_top_k() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .top_p(0.9)
            .top_k(40)
            .build();
        assert_eq!(req.top_p, Some(0.9));
        assert_eq!(req.top_k, Some(40));
    }

    #[test]
    fn builder_with_stop_sequences() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .stop_sequences(vec!["STOP".into(), "END".into()])
            .build();
        let stops = req.stop_sequences.unwrap();
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0], "STOP");
    }

    #[test]
    fn builder_with_tool() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("Read the file")
            .tool(
                "read_file",
                "Read a file from disk",
                json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            )
            .build();
        let tools = req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
    }

    #[test]
    fn builder_with_multiple_tools() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .tool("tool_a", "desc a", json!({"type": "object"}))
            .tool("tool_b", "desc b", json!({"type": "object"}))
            .build();
        assert_eq!(req.tools.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn builder_with_tools_vec() {
        let tools = vec![ClaudeTool {
            name: "search".into(),
            description: Some("Search the web".into()),
            input_schema: json!({"type": "object"}),
        }];
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .tools(tools)
            .build();
        assert_eq!(req.tools.unwrap()[0].name, "search");
    }

    #[test]
    fn builder_with_tool_choice() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .tool_choice(ClaudeToolChoice::Auto {})
            .build();
        assert!(matches!(req.tool_choice, Some(ClaudeToolChoice::Auto {})));
    }

    #[test]
    fn builder_with_thinking() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .thinking(2048)
            .build();
        let t = req.thinking.unwrap();
        assert_eq!(t.budget_tokens, 2048);
        assert_eq!(t.thinking_type, "enabled");
    }

    #[test]
    fn builder_stream_flag() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("test")
            .stream(true)
            .build();
        assert_eq!(req.stream, Some(true));
    }

    #[test]
    fn builder_with_user_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "Look at this".into(),
            },
            ContentBlock::Image {
                source: crate::types::ImageSource::Url {
                    url: "https://example.com/img.png".into(),
                },
            },
        ];
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user_blocks(blocks)
            .build();
        assert_eq!(req.messages.len(), 1);
        match &req.messages[0].content {
            ClaudeContent::Blocks(b) => assert_eq!(b.len(), 2),
            _ => panic!("expected blocks content"),
        }
    }

    #[test]
    fn builder_serde_roundtrip() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .system("Be concise")
            .user("Hello")
            .temperature(0.5)
            .top_p(0.9)
            .build();
        let json = serde_json::to_string(&req).unwrap();
        let back: MessagesRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "claude-sonnet-4-20250514");
        assert_eq!(back.system.as_deref(), Some("Be concise"));
    }

    #[test]
    fn builder_replace_messages() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .user("first")
            .messages(vec![ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("replaced".into()),
            }])
            .build();
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn builder_raw_message() {
        let req = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .message(ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Text("raw".into()),
            })
            .build();
        assert_eq!(req.messages.len(), 1);
    }

    #[test]
    fn builder_clone() {
        let base = CreateMessageRequest::new("claude-sonnet-4-20250514", 4096)
            .system("base system")
            .user("base msg");
        let req1 = base.clone().temperature(0.5).build();
        let req2 = base.temperature(0.9).build();
        assert_eq!(req1.temperature, Some(0.5));
        assert_eq!(req2.temperature, Some(0.9));
    }
}
