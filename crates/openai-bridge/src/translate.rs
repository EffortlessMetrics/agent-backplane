// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translation between OpenAI Chat Completions API types and ABP Intermediate
//! Representation (IR).
//!
//! This module is gated behind the `normalized` feature because it depends
//! on `abp-core` for IR types.

#[cfg(feature = "normalized")]
mod inner {
    use abp_core::ir::{
        IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage,
    };

    use crate::error::BridgeError;
    use crate::openai_types::{
        ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ChatMessage,
        ChatMessageRole, FunctionCall, FunctionDefinition, ToolCall, ToolDefinition, Usage,
    };

    // ── Role mapping ────────────────────────────────────────────────────

    /// Map an OpenAI [`ChatMessageRole`] to an IR [`IrRole`].
    pub fn role_to_ir(role: ChatMessageRole) -> IrRole {
        match role {
            ChatMessageRole::System => IrRole::System,
            ChatMessageRole::User => IrRole::User,
            ChatMessageRole::Assistant => IrRole::Assistant,
            ChatMessageRole::Tool => IrRole::Tool,
        }
    }

    /// Map an IR [`IrRole`] to an OpenAI [`ChatMessageRole`].
    pub fn role_from_ir(role: IrRole) -> ChatMessageRole {
        match role {
            IrRole::System => ChatMessageRole::System,
            IrRole::User => ChatMessageRole::User,
            IrRole::Assistant => ChatMessageRole::Assistant,
            IrRole::Tool => ChatMessageRole::Tool,
        }
    }

    // ── Message mapping ─────────────────────────────────────────────────

    /// Convert an OpenAI [`ChatMessage`] to an IR [`IrMessage`].
    pub fn message_to_ir(msg: &ChatMessage) -> IrMessage {
        let role = role_to_ir(msg.role);
        let mut blocks = Vec::new();

        if let Some(text) = &msg.content {
            if !text.is_empty() {
                blocks.push(IrContentBlock::Text { text: text.clone() });
            }
        }

        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                let input =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
                blocks.push(IrContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input,
                });
            }
        }

        // Tool result messages map to a ToolResult content block
        if msg.role == ChatMessageRole::Tool {
            if let Some(tool_call_id) = &msg.tool_call_id {
                let content_blocks = if let Some(text) = &msg.content {
                    vec![IrContentBlock::Text { text: text.clone() }]
                } else {
                    Vec::new()
                };
                // Replace any text block we added with a ToolResult block
                let blocks_replaced = vec![IrContentBlock::ToolResult {
                    tool_use_id: tool_call_id.clone(),
                    content: content_blocks,
                    is_error: false,
                }];
                return IrMessage::new(IrRole::Tool, blocks_replaced);
            }
        }

        IrMessage::new(role, blocks)
    }

    /// Convert an IR [`IrMessage`] to an OpenAI [`ChatMessage`].
    pub fn message_from_ir(msg: &IrMessage) -> ChatMessage {
        let role = role_from_ir(msg.role);

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_id = None;

        for block in &msg.content {
            match block {
                IrContentBlock::Text { text } => {
                    text_parts.push(text.clone());
                }
                IrContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id: id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_default(),
                        },
                    });
                }
                IrContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => {
                    tool_call_id = Some(tool_use_id.clone());
                    let text = content
                        .iter()
                        .filter_map(|b| match b {
                            IrContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if !text.is_empty() {
                        text_parts.push(text);
                    }
                }
                IrContentBlock::Thinking { text } => {
                    text_parts.push(format!("[thinking: {text}]"));
                }
                IrContentBlock::Image { media_type, data } => {
                    text_parts.push(format!("[image: {media_type}, {} bytes]", data.len()));
                }
            }
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        ChatMessage {
            role,
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id,
        }
    }

    // ── Conversation mapping ────────────────────────────────────────────

    /// Convert OpenAI messages into an IR conversation.
    pub fn conversation_to_ir(messages: &[ChatMessage]) -> IrConversation {
        let ir_messages: Vec<IrMessage> = messages.iter().map(message_to_ir).collect();
        IrConversation::from_messages(ir_messages)
    }

    /// Convert an IR conversation into OpenAI messages.
    pub fn conversation_from_ir(conv: &IrConversation) -> Vec<ChatMessage> {
        conv.messages.iter().map(message_from_ir).collect()
    }

    // ── Tool definition mapping ─────────────────────────────────────────

    /// Convert an OpenAI [`ToolDefinition`] to an IR [`IrToolDefinition`].
    pub fn tool_def_to_ir(tool: &ToolDefinition) -> IrToolDefinition {
        IrToolDefinition {
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
            parameters: tool.function.parameters.clone(),
        }
    }

    /// Convert an IR [`IrToolDefinition`] to an OpenAI [`ToolDefinition`].
    pub fn tool_def_from_ir(tool: &IrToolDefinition) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
        }
    }

    // ── Usage mapping ───────────────────────────────────────────────────

    /// Convert OpenAI [`Usage`] to IR [`IrUsage`].
    pub fn usage_to_ir(usage: &Usage) -> IrUsage {
        IrUsage::from_io(usage.prompt_tokens, usage.completion_tokens)
    }

    /// Convert IR [`IrUsage`] to OpenAI [`Usage`].
    pub fn usage_from_ir(usage: &IrUsage) -> Usage {
        Usage {
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            total_tokens: usage.input_tokens + usage.output_tokens,
        }
    }

    /// Extract [`IrUsage`] from a [`ChatCompletionResponse`].
    pub fn extract_usage(response: &ChatCompletionResponse) -> IrUsage {
        match &response.usage {
            Some(u) => usage_to_ir(u),
            None => IrUsage::default(),
        }
    }

    /// Merge two [`Usage`] values by summing all fields.
    pub fn merge_usage(a: &Usage, b: &Usage) -> Usage {
        Usage {
            prompt_tokens: a.prompt_tokens + b.prompt_tokens,
            completion_tokens: a.completion_tokens + b.completion_tokens,
            total_tokens: a.total_tokens + b.total_tokens,
        }
    }

    // ── Response mapping ────────────────────────────────────────────────

    /// Extract content blocks from a [`ChatCompletionResponse`] as IR blocks.
    pub fn response_content_to_ir(response: &ChatCompletionResponse) -> Vec<IrContentBlock> {
        response
            .choices
            .iter()
            .flat_map(|c| {
                let msg = message_to_ir(&c.message);
                msg.content
            })
            .collect()
    }

    /// Convert a [`ChatCompletionResponse`] into an IR assistant message.
    pub fn response_to_ir_message(response: &ChatCompletionResponse) -> IrMessage {
        let blocks = response_content_to_ir(response);
        IrMessage::new(IrRole::Assistant, blocks)
    }

    // ── Stream accumulator ──────────────────────────────────────────────

    /// Accumulated state from streaming chunks.
    #[derive(Debug, Default)]
    pub struct StreamAccumulator {
        /// Accumulated text content.
        pub text: String,
        /// Accumulated tool calls.
        pub tool_calls: Vec<ToolCallBuilder>,
        /// Accumulated usage.
        pub usage: Usage,
        /// Finish reason from the final chunk.
        pub finish_reason: Option<String>,
        /// Model from the first chunk.
        pub model: Option<String>,
        /// Chunk ID from the first chunk.
        pub chunk_id: Option<String>,
    }

    /// Builder for a single tool call during streaming.
    #[derive(Debug, Clone, Default)]
    pub struct ToolCallBuilder {
        /// Tool call ID.
        pub id: String,
        /// Function name.
        pub name: String,
        /// Accumulated arguments JSON string.
        pub arguments: String,
    }

    impl StreamAccumulator {
        /// Create a new empty accumulator.
        pub fn new() -> Self {
            Self::default()
        }

        /// Process a single [`ChatCompletionChunk`] and update internal state.
        ///
        /// Returns an optional text delta for immediate forwarding.
        pub fn feed(&mut self, chunk: &ChatCompletionChunk) -> Option<StreamFragment> {
            if self.model.is_none() {
                self.model = Some(chunk.model.clone());
            }
            if self.chunk_id.is_none() {
                self.chunk_id = Some(chunk.id.clone());
            }

            for choice in &chunk.choices {
                if let Some(ref reason) = choice.finish_reason {
                    self.finish_reason = Some(reason.clone());
                }

                if let Some(ref content) = choice.delta.content {
                    self.text.push_str(content);
                    return Some(StreamFragment::TextDelta(content.clone()));
                }

                if let Some(ref tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        let idx = tc.index as usize;
                        while self.tool_calls.len() <= idx {
                            self.tool_calls.push(ToolCallBuilder::default());
                        }
                        if let Some(ref id) = tc.id {
                            self.tool_calls[idx].id = id.clone();
                        }
                        if let Some(ref func) = tc.function {
                            if let Some(ref name) = func.name {
                                self.tool_calls[idx].name = name.clone();
                            }
                            if let Some(ref args) = func.arguments {
                                self.tool_calls[idx].arguments.push_str(args);
                            }
                        }
                    }
                }
            }

            None
        }

        /// Finalize the accumulated state into IR content blocks and usage.
        pub fn finish(self) -> (Vec<IrContentBlock>, IrUsage) {
            let mut blocks = Vec::new();

            if !self.text.is_empty() {
                blocks.push(IrContentBlock::Text { text: self.text });
            }

            for tc in self.tool_calls {
                let input = serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null);
                blocks.push(IrContentBlock::ToolUse {
                    id: tc.id,
                    name: tc.name,
                    input,
                });
            }

            let usage = usage_to_ir(&self.usage);
            (blocks, usage)
        }
    }

    /// A fragment emitted during streaming for immediate processing.
    #[derive(Debug, Clone, PartialEq)]
    pub enum StreamFragment {
        /// Incremental text.
        TextDelta(String),
        /// An error was received.
        Error(String),
    }

    // ── Request construction ────────────────────────────────────────────

    /// Build a minimal [`ChatCompletionRequest`] from a task string.
    pub fn task_to_request(task: &str, model: &str, max_tokens: u32) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: ChatMessageRole::User,
                content: Some(task.to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            tools: None,
            temperature: None,
            max_tokens: Some(max_tokens),
            stream: Some(true),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            n: None,
            tool_choice: None,
        }
    }

    // ── Error translation ───────────────────────────────────────────────

    /// Convert an OpenAI API error into a [`BridgeError`].
    pub fn api_error_to_bridge(error: &crate::openai_types::ApiError) -> BridgeError {
        match error.error.error_type.as_str() {
            "authentication_error" => {
                BridgeError::Config(format!("authentication failed: {}", error.error.message))
            }
            "invalid_request_error" => {
                BridgeError::Config(format!("invalid request: {}", error.error.message))
            }
            "rate_limit_error" => {
                BridgeError::Run(format!("rate limited: {}", error.error.message))
            }
            "server_error" => {
                BridgeError::Run(format!("API server error: {}", error.error.message))
            }
            other => BridgeError::Run(format!("{other}: {}", error.error.message)),
        }
    }

    // ── Streaming chunk translation ─────────────────────────────────────

    /// Extract an IR text delta from a single [`ChatCompletionChunk`], if present.
    pub fn chunk_text_delta(chunk: &ChatCompletionChunk) -> Option<String> {
        chunk.choices.first().and_then(|c| c.delta.content.clone())
    }

    /// Extract the finish reason from a [`ChatCompletionChunk`], if present.
    pub fn chunk_finish_reason(chunk: &ChatCompletionChunk) -> Option<String> {
        chunk.choices.first().and_then(|c| c.finish_reason.clone())
    }

    /// Convert a completed stream (accumulated text + tool calls) into an IR message.
    pub fn stream_to_ir_message(text: &str, tool_calls: &[ToolCall]) -> IrMessage {
        let mut blocks = Vec::new();

        if !text.is_empty() {
            blocks.push(IrContentBlock::Text {
                text: text.to_string(),
            });
        }

        for tc in tool_calls {
            let input =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
            blocks.push(IrContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                input,
            });
        }

        IrMessage::new(IrRole::Assistant, blocks)
    }

    // ── Embedding usage translation ─────────────────────────────────────

    /// Convert embedding usage to IR usage (output_tokens = 0 for embeddings).
    pub fn embedding_usage_to_ir(usage: &crate::embeddings::EmbeddingUsage) -> IrUsage {
        IrUsage::from_io(usage.prompt_tokens, 0)
    }

    // ── Function calling translation ────────────────────────────────────

    /// Convert a [`ToolChoice`](crate::function_calling::ToolChoice) to a `serde_json::Value`
    /// suitable for the `tool_choice` field of a [`ChatCompletionRequest`].
    pub fn tool_choice_to_value(choice: &crate::function_calling::ToolChoice) -> serde_json::Value {
        serde_json::to_value(choice).unwrap_or(serde_json::Value::Null)
    }

    /// Try to parse a `serde_json::Value` as a [`ToolChoice`](crate::function_calling::ToolChoice).
    pub fn tool_choice_from_value(
        value: &serde_json::Value,
    ) -> Result<crate::function_calling::ToolChoice, BridgeError> {
        serde_json::from_value(value.clone())
            .map_err(|e| BridgeError::Run(format!("invalid tool_choice: {e}")))
    }
}

#[cfg(feature = "normalized")]
pub use inner::*;
