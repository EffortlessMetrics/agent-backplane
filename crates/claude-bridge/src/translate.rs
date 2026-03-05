// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translation between Claude Messages API types and ABP Intermediate
//! Representation (IR).
//!
//! This module is gated behind the `normalized` feature because it depends
//! on `abp-core` for IR types.

#[cfg(feature = "normalized")]
mod inner {
    use abp_core::ir::{
        IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage,
    };

    use crate::claude_types::{
        ApiError, ContentBlock, ImageSource, Message, MessageContent, MessagesRequest,
        MessagesResponse, Role, StreamDelta, StreamEvent, SystemMessage, ThinkingConfig,
        ToolChoice, ToolDefinition, Usage,
    };
    use crate::error::BridgeError;
    use crate::thinking::ThinkingBlock;
    use crate::tool_use::{CachedToolDefinition, RichToolResult, ToolResultContent};
    use crate::vision::ImageMediaType;

    // ── Role mapping ────────────────────────────────────────────────────

    /// Map a Claude [`Role`] to an IR [`IrRole`].
    pub fn role_to_ir(role: Role) -> IrRole {
        match role {
            Role::User => IrRole::User,
            Role::Assistant => IrRole::Assistant,
        }
    }

    /// Map an IR [`IrRole`] to a Claude [`Role`].
    ///
    /// System and Tool roles are mapped to [`Role::User`] since Claude
    /// uses a top-level `system` field and tool results are sent as user
    /// messages.
    pub fn role_from_ir(role: IrRole) -> Role {
        match role {
            IrRole::Assistant => Role::Assistant,
            IrRole::User | IrRole::System | IrRole::Tool => Role::User,
        }
    }

    // ── Content block mapping ───────────────────────────────────────────

    /// Convert a Claude [`ContentBlock`] to an IR [`IrContentBlock`].
    pub fn content_block_to_ir(block: &ContentBlock) -> IrContentBlock {
        match block {
            ContentBlock::Text { text } => IrContentBlock::Text { text: text.clone() },
            ContentBlock::ToolUse { id, name, input } => IrContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            },
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let nested = match content {
                    Some(text) => vec![IrContentBlock::Text { text: text.clone() }],
                    None => Vec::new(),
                };
                IrContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: nested,
                    is_error: is_error.unwrap_or(false),
                }
            }
            ContentBlock::Thinking {
                thinking,
                signature: _,
            } => IrContentBlock::Thinking {
                text: thinking.clone(),
            },
            ContentBlock::Image { source } => match source {
                ImageSource::Base64 { media_type, data } => IrContentBlock::Image {
                    media_type: media_type.clone(),
                    data: data.clone(),
                },
                ImageSource::Url { url } => IrContentBlock::Text {
                    text: format!("[image: {url}]"),
                },
            },
        }
    }

    /// Convert an IR [`IrContentBlock`] to a Claude [`ContentBlock`].
    pub fn content_block_from_ir(block: &IrContentBlock) -> ContentBlock {
        match block {
            IrContentBlock::Text { text } => ContentBlock::Text { text: text.clone() },
            IrContentBlock::ToolUse { id, name, input } => ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            },
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let text = content
                    .iter()
                    .filter_map(|b| match b {
                        IrContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: if text.is_empty() { None } else { Some(text) },
                    is_error: if *is_error { Some(true) } else { None },
                }
            }
            IrContentBlock::Thinking { text } => ContentBlock::Thinking {
                thinking: text.clone(),
                signature: None,
            },
            IrContentBlock::Image { media_type, data } => ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: media_type.clone(),
                    data: data.clone(),
                },
            },
        }
    }

    // ── Rich tool result → IR ───────────────────────────────────────────

    /// Convert a [`RichToolResult`] to an IR [`IrContentBlock::ToolResult`].
    ///
    /// Image parts inside the rich result are converted to IR image blocks
    /// nested within the tool result.
    pub fn rich_tool_result_to_ir(result: &RichToolResult) -> IrContentBlock {
        let nested: Vec<IrContentBlock> = result
            .content
            .iter()
            .map(|part| match part {
                ToolResultContent::Text { text } => IrContentBlock::Text { text: text.clone() },
                ToolResultContent::Image { source } => match source {
                    ImageSource::Base64 { media_type, data } => IrContentBlock::Image {
                        media_type: media_type.clone(),
                        data: data.clone(),
                    },
                    ImageSource::Url { url } => IrContentBlock::Text {
                        text: format!("[image: {url}]"),
                    },
                },
            })
            .collect();
        IrContentBlock::ToolResult {
            tool_use_id: result.tool_use_id.clone(),
            content: nested,
            is_error: result.is_error.unwrap_or(false),
        }
    }

    // ── ThinkingBlock → IR ──────────────────────────────────────────────

    /// Convert a [`ThinkingBlock`] to an IR [`IrContentBlock::Thinking`].
    ///
    /// The signature is deliberately dropped — it is Claude-specific and
    /// not meaningful in the cross-dialect IR.
    pub fn thinking_block_to_ir(block: &ThinkingBlock) -> IrContentBlock {
        IrContentBlock::Thinking {
            text: block.thinking.clone(),
        }
    }

    /// Convert an IR [`IrContentBlock::Thinking`] to a [`ThinkingBlock`].
    ///
    /// The resulting block has no signature since the IR doesn't carry one.
    pub fn thinking_block_from_ir(block: &IrContentBlock) -> Option<ThinkingBlock> {
        match block {
            IrContentBlock::Thinking { text } => Some(ThinkingBlock::new(text.clone())),
            _ => None,
        }
    }

    // ── CachedToolDefinition → IR ───────────────────────────────────────

    /// Convert a [`CachedToolDefinition`] to an IR [`IrToolDefinition`].
    ///
    /// Cache-control metadata is dropped (not part of the IR).
    pub fn cached_tool_def_to_ir(tool: &CachedToolDefinition) -> IrToolDefinition {
        IrToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
        }
    }

    /// Convert an IR [`IrToolDefinition`] to a [`CachedToolDefinition`]
    /// with no cache control.
    pub fn cached_tool_def_from_ir(tool: &IrToolDefinition) -> CachedToolDefinition {
        CachedToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.parameters.clone(),
            cache_control: None,
        }
    }

    // ── ImageMediaType → IR ─────────────────────────────────────────────

    /// Build an IR image block from a typed [`ImageMediaType`] and base64 data.
    pub fn typed_image_to_ir(media_type: ImageMediaType, data: &str) -> IrContentBlock {
        IrContentBlock::Image {
            media_type: media_type.as_str().to_string(),
            data: data.to_string(),
        }
    }

    // ── ToolChoice → IR metadata ────────────────────────────────────────

    /// Convert a [`ToolChoice`] to a JSON value for stashing in IR metadata.
    pub fn tool_choice_to_value(choice: &ToolChoice) -> serde_json::Value {
        serde_json::to_value(choice).unwrap_or(serde_json::Value::Null)
    }

    /// Attempt to recover a [`ToolChoice`] from a JSON value.
    pub fn tool_choice_from_value(value: &serde_json::Value) -> Option<ToolChoice> {
        serde_json::from_value(value.clone()).ok()
    }

    // ── ThinkingConfig → IR metadata ────────────────────────────────────

    /// Convert a [`ThinkingConfig`] to a JSON value for stashing in IR metadata.
    pub fn thinking_config_to_value(config: &ThinkingConfig) -> serde_json::Value {
        serde_json::to_value(config).unwrap_or(serde_json::Value::Null)
    }

    /// Attempt to recover a [`ThinkingConfig`] from a JSON value.
    pub fn thinking_config_from_value(value: &serde_json::Value) -> Option<ThinkingConfig> {
        serde_json::from_value(value.clone()).ok()
    }

    // ── Message mapping ─────────────────────────────────────────────────

    /// Convert a Claude [`Message`] to an IR [`IrMessage`].
    pub fn message_to_ir(msg: &Message) -> IrMessage {
        let role = role_to_ir(msg.role);
        let content = match &msg.content {
            MessageContent::Text(text) => vec![IrContentBlock::Text { text: text.clone() }],
            MessageContent::Blocks(blocks) => blocks.iter().map(content_block_to_ir).collect(),
        };
        IrMessage::new(role, content)
    }

    /// Convert an IR [`IrMessage`] to a Claude [`Message`].
    pub fn message_from_ir(msg: &IrMessage) -> Message {
        let role = role_from_ir(msg.role);
        let blocks: Vec<ContentBlock> = msg.content.iter().map(content_block_from_ir).collect();
        Message {
            role,
            content: MessageContent::Blocks(blocks),
        }
    }

    // ── Conversation mapping ────────────────────────────────────────────

    /// Convert Claude messages into an IR conversation.
    ///
    /// If a system message is provided, it becomes the first message in the
    /// conversation with [`IrRole::System`].
    pub fn conversation_to_ir(
        messages: &[Message],
        system: Option<&SystemMessage>,
    ) -> IrConversation {
        let mut ir_messages = Vec::new();

        if let Some(sys) = system {
            let text = match sys {
                SystemMessage::Text(t) => t.clone(),
                SystemMessage::Blocks(blocks) => blocks
                    .iter()
                    .map(|b| match b {
                        crate::claude_types::SystemBlock::Text { text, .. } => text.as_str(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            ir_messages.push(IrMessage::text(IrRole::System, text));
        }

        for msg in messages {
            ir_messages.push(message_to_ir(msg));
        }

        IrConversation::from_messages(ir_messages)
    }

    // ── Tool definition mapping ─────────────────────────────────────────

    /// Convert a Claude [`ToolDefinition`] to an IR [`IrToolDefinition`].
    pub fn tool_def_to_ir(tool: &ToolDefinition) -> IrToolDefinition {
        IrToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
        }
    }

    /// Convert an IR [`IrToolDefinition`] to a Claude [`ToolDefinition`].
    pub fn tool_def_from_ir(tool: &IrToolDefinition) -> ToolDefinition {
        ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.parameters.clone(),
        }
    }

    // ── Usage mapping ───────────────────────────────────────────────────

    /// Convert Claude [`Usage`] to IR [`IrUsage`].
    pub fn usage_to_ir(usage: &Usage) -> IrUsage {
        IrUsage::with_cache(
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_input_tokens.unwrap_or(0),
            usage.cache_creation_input_tokens.unwrap_or(0),
        )
    }

    /// Convert IR [`IrUsage`] to Claude [`Usage`].
    pub fn usage_from_ir(usage: &IrUsage) -> Usage {
        Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_creation_input_tokens: if usage.cache_write_tokens > 0 {
                Some(usage.cache_write_tokens)
            } else {
                None
            },
            cache_read_input_tokens: if usage.cache_read_tokens > 0 {
                Some(usage.cache_read_tokens)
            } else {
                None
            },
        }
    }

    /// Extract [`IrUsage`] from a [`MessagesResponse`].
    pub fn extract_usage(response: &MessagesResponse) -> IrUsage {
        usage_to_ir(&response.usage)
    }

    /// Merge two [`Usage`] values by summing all fields.
    pub fn merge_usage(a: &Usage, b: &Usage) -> Usage {
        Usage {
            input_tokens: a.input_tokens + b.input_tokens,
            output_tokens: a.output_tokens + b.output_tokens,
            cache_creation_input_tokens: match (
                a.cache_creation_input_tokens,
                b.cache_creation_input_tokens,
            ) {
                (Some(x), Some(y)) => Some(x + y),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            },
            cache_read_input_tokens: match (a.cache_read_input_tokens, b.cache_read_input_tokens) {
                (Some(x), Some(y)) => Some(x + y),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            },
        }
    }

    // ── Response mapping ────────────────────────────────────────────────

    /// Extract content blocks from a [`MessagesResponse`] as IR blocks.
    pub fn response_content_to_ir(response: &MessagesResponse) -> Vec<IrContentBlock> {
        response.content.iter().map(content_block_to_ir).collect()
    }

    /// Convert a [`MessagesResponse`] into an IR assistant message.
    pub fn response_to_ir_message(response: &MessagesResponse) -> IrMessage {
        let blocks = response_content_to_ir(response);
        IrMessage::new(IrRole::Assistant, blocks)
    }

    // ── Stream event mapping ────────────────────────────────────────────

    /// Accumulated state from streaming events.
    #[derive(Debug, Default)]
    pub struct StreamAccumulator {
        /// Content blocks built from streaming deltas.
        pub content_blocks: Vec<ContentBlockBuilder>,
        /// Accumulated usage (from message_start and message_delta).
        pub usage: Usage,
        /// Stop reason from message_delta.
        pub stop_reason: Option<String>,
        /// Model from message_start.
        pub model: Option<String>,
        /// Message ID from message_start.
        pub message_id: Option<String>,
    }

    /// Builder for a single content block during streaming.
    #[derive(Debug, Clone)]
    pub enum ContentBlockBuilder {
        /// Text block accumulating deltas.
        Text(String),
        /// Tool use block accumulating partial JSON.
        ToolUse {
            /// Tool invocation ID.
            id: String,
            /// Tool name.
            name: String,
            /// Accumulated partial JSON string.
            partial_json: String,
        },
        /// Thinking block accumulating deltas.
        Thinking {
            /// Accumulated thinking text.
            text: String,
            /// Accumulated signature.
            signature: String,
        },
    }

    impl StreamAccumulator {
        /// Create a new empty accumulator.
        pub fn new() -> Self {
            Self::default()
        }

        /// Process a single [`StreamEvent`] and update internal state.
        ///
        /// Returns an optional text delta for immediate forwarding.
        pub fn feed(&mut self, event: &StreamEvent) -> Option<StreamFragment> {
            match event {
                StreamEvent::MessageStart { message } => {
                    self.usage = message.usage;
                    self.model = Some(message.model.clone());
                    self.message_id = Some(message.id.clone());
                    None
                }
                StreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => {
                    let idx = *index as usize;
                    let builder = match content_block {
                        ContentBlock::Text { text } => ContentBlockBuilder::Text(text.clone()),
                        ContentBlock::ToolUse { id, name, .. } => ContentBlockBuilder::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            partial_json: String::new(),
                        },
                        ContentBlock::Thinking { thinking, .. } => ContentBlockBuilder::Thinking {
                            text: thinking.clone(),
                            signature: String::new(),
                        },
                        _ => ContentBlockBuilder::Text(String::new()),
                    };
                    // Ensure capacity
                    while self.content_blocks.len() <= idx {
                        self.content_blocks
                            .push(ContentBlockBuilder::Text(String::new()));
                    }
                    self.content_blocks[idx] = builder;
                    None
                }
                StreamEvent::ContentBlockDelta { index, delta } => {
                    let idx = *index as usize;
                    if idx < self.content_blocks.len() {
                        match (&mut self.content_blocks[idx], delta) {
                            (ContentBlockBuilder::Text(buf), StreamDelta::TextDelta { text }) => {
                                buf.push_str(text);
                                Some(StreamFragment::TextDelta(text.clone()))
                            }
                            (
                                ContentBlockBuilder::ToolUse { partial_json, .. },
                                StreamDelta::InputJsonDelta {
                                    partial_json: chunk,
                                },
                            ) => {
                                partial_json.push_str(chunk);
                                None
                            }
                            (
                                ContentBlockBuilder::Thinking { text, .. },
                                StreamDelta::ThinkingDelta { thinking: chunk },
                            ) => {
                                text.push_str(chunk);
                                Some(StreamFragment::ThinkingDelta(chunk.clone()))
                            }
                            (
                                ContentBlockBuilder::Thinking { signature, .. },
                                StreamDelta::SignatureDelta { signature: chunk },
                            ) => {
                                signature.push_str(chunk);
                                None
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                StreamEvent::ContentBlockStop { .. } => None,
                StreamEvent::MessageDelta { delta, usage } => {
                    self.stop_reason = delta.stop_reason.clone();
                    if let Some(u) = usage {
                        self.usage = merge_usage(&self.usage, u);
                    }
                    None
                }
                StreamEvent::MessageStop {} => None,
                StreamEvent::Ping {} => None,
                StreamEvent::Error { error } => Some(StreamFragment::Error(error.clone())),
            }
        }

        /// Finalize the accumulated state into IR content blocks and usage.
        pub fn finish(self) -> (Vec<IrContentBlock>, IrUsage) {
            let blocks = self
                .content_blocks
                .into_iter()
                .map(|b| match b {
                    ContentBlockBuilder::Text(text) => IrContentBlock::Text { text },
                    ContentBlockBuilder::ToolUse {
                        id,
                        name,
                        partial_json,
                    } => {
                        let input =
                            serde_json::from_str(&partial_json).unwrap_or(serde_json::Value::Null);
                        IrContentBlock::ToolUse { id, name, input }
                    }
                    ContentBlockBuilder::Thinking { text, .. } => IrContentBlock::Thinking { text },
                })
                .collect();
            let usage = usage_to_ir(&self.usage);
            (blocks, usage)
        }
    }

    /// A fragment emitted during streaming for immediate processing.
    #[derive(Debug, Clone, PartialEq)]
    pub enum StreamFragment {
        /// Incremental text.
        TextDelta(String),
        /// Incremental thinking.
        ThinkingDelta(String),
        /// An error was received.
        Error(ApiError),
    }

    // ── Error translation ───────────────────────────────────────────────

    /// Convert a Claude [`ApiError`] into a [`BridgeError`].
    pub fn api_error_to_bridge(error: &ApiError) -> BridgeError {
        match error.error_type.as_str() {
            "authentication_error" => {
                BridgeError::Config(format!("authentication failed: {}", error.message))
            }
            "invalid_request_error" => {
                BridgeError::Config(format!("invalid request: {}", error.message))
            }
            "rate_limit_error" => BridgeError::Run(format!("rate limited: {}", error.message)),
            "overloaded_error" => BridgeError::Run(format!("API overloaded: {}", error.message)),
            "api_error" | "server_error" => {
                BridgeError::Run(format!("API server error: {}", error.message))
            }
            other => BridgeError::Run(format!("{other}: {}", error.message)),
        }
    }

    // ── Request construction ────────────────────────────────────────────

    /// Build a minimal [`MessagesRequest`] from a task string.
    pub fn task_to_request(task: &str, model: &str, max_tokens: u32) -> MessagesRequest {
        MessagesRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text(task.to_string()),
            }],
            max_tokens,
            system: None,
            tools: None,
            metadata: None,
            stream: Some(true),
            stop_sequences: None,
            temperature: None,
            top_p: None,
            top_k: None,
            tool_choice: None,
            thinking: None,
        }
    }
}

#[cfg(feature = "normalized")]
pub use inner::*;
