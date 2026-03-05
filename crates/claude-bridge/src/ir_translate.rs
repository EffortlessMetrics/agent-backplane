// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translation between Claude Messages API types and the `abp-dialect`
//! Intermediate Representation (IR).
//!
//! This module is gated behind the `ir` feature because it depends on
//! `abp-dialect` for the IR types.

#[cfg(feature = "ir")]
mod inner {
    use std::collections::BTreeMap;

    use abp_dialect::ir::{
        IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
        IrStreamEvent, IrToolDefinition, IrUsage,
    };

    use crate::claude_types::{
        ContentBlock, ImageSource, Message, MessageContent, MessagesRequest, MessagesResponse,
        Role, StreamDelta, StreamEvent, SystemMessage, Usage,
    };

    // ── Role mapping ────────────────────────────────────────────────────

    fn role_to_ir(role: Role) -> IrRole {
        match role {
            Role::User => IrRole::User,
            Role::Assistant => IrRole::Assistant,
        }
    }

    fn role_from_ir(role: IrRole) -> Role {
        match role {
            IrRole::Assistant => Role::Assistant,
            IrRole::User | IrRole::System | IrRole::Tool => Role::User,
        }
    }

    // ── Content block mapping ───────────────────────────────────────────

    fn content_block_to_ir(block: &ContentBlock) -> IrContentBlock {
        match block {
            ContentBlock::Text { text } => IrContentBlock::Text { text: text.clone() },
            ContentBlock::ToolUse { id, name, input } => IrContentBlock::ToolCall {
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
                    tool_call_id: tool_use_id.clone(),
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

    fn content_block_from_ir(block: &IrContentBlock) -> ContentBlock {
        match block {
            IrContentBlock::Text { text } => ContentBlock::Text { text: text.clone() },
            IrContentBlock::ToolCall { id, name, input } => ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            },
            IrContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => {
                let text = content
                    .iter()
                    .filter_map(|b| b.as_text())
                    .collect::<Vec<_>>()
                    .join("");
                ContentBlock::ToolResult {
                    tool_use_id: tool_call_id.clone(),
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
            IrContentBlock::Audio { media_type, data } => ContentBlock::Text {
                text: format!("[audio: {media_type}, {} bytes]", data.len()),
            },
            IrContentBlock::Custom { custom_type, data } => ContentBlock::Text {
                text: format!("[custom:{custom_type}: {}]", data),
            },
        }
    }

    // ── Message mapping ─────────────────────────────────────────────────

    fn message_to_ir(msg: &Message) -> IrMessage {
        let role = role_to_ir(msg.role);
        let content = match &msg.content {
            MessageContent::Text(text) => vec![IrContentBlock::Text { text: text.clone() }],
            MessageContent::Blocks(blocks) => blocks.iter().map(content_block_to_ir).collect(),
        };
        IrMessage::new(role, content)
    }

    fn message_from_ir(msg: &IrMessage) -> Message {
        let role = role_from_ir(msg.role);
        let blocks: Vec<ContentBlock> = msg.content.iter().map(content_block_from_ir).collect();
        Message {
            role,
            content: MessageContent::Blocks(blocks),
        }
    }

    // ── Tool definition mapping ─────────────────────────────────────────

    fn tool_def_to_ir(tool: &crate::claude_types::ToolDefinition) -> IrToolDefinition {
        IrToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.input_schema.clone(),
        }
    }

    fn tool_def_from_ir(tool: &IrToolDefinition) -> crate::claude_types::ToolDefinition {
        crate::claude_types::ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.parameters.clone(),
        }
    }

    // ── Usage mapping ───────────────────────────────────────────────────

    fn usage_to_ir(usage: &Usage) -> IrUsage {
        let input = usage.input_tokens;
        let output = usage.output_tokens;
        IrUsage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
            cache_write_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
        }
    }

    fn usage_from_ir(usage: &IrUsage) -> Usage {
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

    // ── Stop reason mapping ─────────────────────────────────────────────

    fn stop_reason_to_ir(reason: &str) -> IrStopReason {
        match reason {
            "end_turn" => IrStopReason::EndTurn,
            "max_tokens" => IrStopReason::MaxTokens,
            "stop_sequence" => IrStopReason::StopSequence,
            "tool_use" => IrStopReason::ToolUse,
            other => IrStopReason::Other(other.to_string()),
        }
    }

    fn stop_reason_from_ir(reason: &IrStopReason) -> String {
        match reason {
            IrStopReason::EndTurn => "end_turn".into(),
            IrStopReason::MaxTokens => "max_tokens".into(),
            IrStopReason::StopSequence => "stop_sequence".into(),
            IrStopReason::ToolUse => "tool_use".into(),
            IrStopReason::ContentFilter => "content_filter".into(),
            IrStopReason::Other(s) => s.clone(),
        }
    }

    // ── Public API ──────────────────────────────────────────────────────

    /// Convert a Claude [`MessagesRequest`] to an [`IrRequest`].
    ///
    /// The system prompt is extracted from the top-level `system` field
    /// (not embedded as a message). Tool definitions, generation config
    /// (model, max_tokens, temperature, top_p, top_k, stop_sequences),
    /// and vendor-specific extras (tool_choice, thinking) are carried
    /// through metadata.
    pub fn claude_request_to_ir(req: &MessagesRequest) -> IrRequest {
        let messages: Vec<IrMessage> = req.messages.iter().map(message_to_ir).collect();

        let tools: Vec<IrToolDefinition> = req
            .tools
            .as_ref()
            .map(|t| t.iter().map(tool_def_to_ir).collect())
            .unwrap_or_default();

        let system_prompt = req.system.as_ref().map(|sys| match sys {
            SystemMessage::Text(t) => t.clone(),
            SystemMessage::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    crate::claude_types::SystemBlock::Text { text, .. } => text.as_str(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        });

        let stop_sequences = req.stop_sequences.clone().unwrap_or_default();

        let config = IrGenerationConfig {
            max_tokens: Some(req.max_tokens as u64),
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: req.top_k,
            stop_sequences,
            extra: BTreeMap::new(),
        };

        let mut metadata = BTreeMap::new();
        if let Some(ref tc) = req.tool_choice {
            if let Ok(v) = serde_json::to_value(tc) {
                metadata.insert("tool_choice".into(), v);
            }
        }
        if let Some(ref th) = req.thinking {
            if let Ok(v) = serde_json::to_value(th) {
                metadata.insert("thinking".into(), v);
            }
        }
        if let Some(true) = req.stream {
            metadata.insert("stream".into(), serde_json::Value::Bool(true));
        }

        IrRequest {
            model: Some(req.model.clone()),
            system_prompt,
            messages,
            tools,
            config,
            metadata,
        }
    }

    /// Convert an [`IrRequest`] to a Claude [`MessagesRequest`].
    ///
    /// The system prompt is placed in the top-level `system` field.
    /// `max_tokens` defaults to 4096 if not set in the IR config.
    pub fn ir_to_claude_request(ir: &IrRequest) -> MessagesRequest {
        let messages: Vec<Message> = ir.messages.iter().map(message_from_ir).collect();

        let tools = if ir.tools.is_empty() {
            None
        } else {
            Some(ir.tools.iter().map(tool_def_from_ir).collect())
        };

        let system = ir
            .system_prompt
            .as_ref()
            .map(|s| SystemMessage::Text(s.clone()));

        let stop_sequences = if ir.config.stop_sequences.is_empty() {
            None
        } else {
            Some(ir.config.stop_sequences.clone())
        };

        let max_tokens = ir.config.max_tokens.unwrap_or(4096) as u32;

        let tool_choice = ir
            .metadata
            .get("tool_choice")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let thinking = ir
            .metadata
            .get("thinking")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let stream = ir
            .metadata
            .get("stream")
            .and_then(|v| v.as_bool())
            .filter(|b| *b);

        MessagesRequest {
            model: ir
                .model
                .clone()
                .unwrap_or_else(|| "claude-sonnet-4-20250514".into()),
            messages,
            max_tokens,
            system,
            tools,
            metadata: None,
            stream: stream.map(|_| true),
            stop_sequences,
            temperature: ir.config.temperature,
            top_p: ir.config.top_p,
            top_k: ir.config.top_k,
            tool_choice,
            thinking,
        }
    }

    /// Convert a Claude [`MessagesResponse`] to an [`IrResponse`].
    ///
    /// Content blocks, stop reason, usage, model, and id are all mapped.
    pub fn claude_response_to_ir(resp: &MessagesResponse) -> IrResponse {
        let content: Vec<IrContentBlock> = resp.content.iter().map(content_block_to_ir).collect();

        let stop_reason = resp.stop_reason.as_deref().map(stop_reason_to_ir);
        let usage = Some(usage_to_ir(&resp.usage));

        IrResponse {
            id: Some(resp.id.clone()),
            model: Some(resp.model.clone()),
            content,
            stop_reason,
            usage,
            metadata: BTreeMap::new(),
        }
    }

    /// Convert an [`IrResponse`] to a Claude [`MessagesResponse`].
    ///
    /// Missing fields are filled with reasonable defaults.
    pub fn ir_to_claude_response(ir: &IrResponse) -> MessagesResponse {
        let content: Vec<ContentBlock> = ir.content.iter().map(content_block_from_ir).collect();

        let stop_reason = ir.stop_reason.as_ref().map(stop_reason_from_ir);

        let usage = ir.usage.as_ref().map(usage_from_ir).unwrap_or_default();

        MessagesResponse {
            id: ir.id.clone().unwrap_or_default(),
            response_type: "message".into(),
            role: "assistant".into(),
            content,
            model: ir.model.clone().unwrap_or_default(),
            stop_reason,
            stop_sequence: None,
            usage,
        }
    }

    /// Convert a Claude [`StreamEvent`] to a `Vec<IrStreamEvent>`.
    ///
    /// Most events produce a single IR event; some (like `message_start`)
    /// may produce multiple (stream start + usage). Events that carry no
    /// meaningful data for the IR (like `ping`) return an empty vec.
    pub fn claude_stream_to_ir(event: &StreamEvent) -> Vec<IrStreamEvent> {
        match event {
            StreamEvent::MessageStart { message } => {
                let mut events = vec![IrStreamEvent::StreamStart {
                    id: Some(message.id.clone()),
                    model: Some(message.model.clone()),
                }];
                let usage = usage_to_ir(&message.usage);
                if usage.input_tokens > 0 || usage.output_tokens > 0 {
                    events.push(IrStreamEvent::Usage { usage });
                }
                events
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let block = content_block_to_ir(content_block);
                vec![IrStreamEvent::ContentBlockStart {
                    index: *index as usize,
                    block,
                }]
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                let idx = *index as usize;
                match delta {
                    StreamDelta::TextDelta { text } => {
                        vec![IrStreamEvent::TextDelta {
                            index: idx,
                            text: text.clone(),
                        }]
                    }
                    StreamDelta::InputJsonDelta { partial_json } => {
                        vec![IrStreamEvent::ToolCallDelta {
                            index: idx,
                            arguments_delta: partial_json.clone(),
                        }]
                    }
                    StreamDelta::ThinkingDelta { thinking } => {
                        vec![IrStreamEvent::ThinkingDelta {
                            index: idx,
                            text: thinking.clone(),
                        }]
                    }
                    StreamDelta::SignatureDelta { .. } => {
                        // Signatures are Claude-specific; no IR equivalent.
                        vec![]
                    }
                }
            }
            StreamEvent::ContentBlockStop { index } => {
                vec![IrStreamEvent::ContentBlockStop {
                    index: *index as usize,
                }]
            }
            StreamEvent::MessageDelta { delta, usage } => {
                let mut events = Vec::new();
                if let Some(u) = usage {
                    events.push(IrStreamEvent::Usage {
                        usage: usage_to_ir(u),
                    });
                }
                let stop_reason = delta.stop_reason.as_deref().map(stop_reason_to_ir);
                events.push(IrStreamEvent::StreamEnd { stop_reason });
                events
            }
            StreamEvent::MessageStop {} => vec![],
            StreamEvent::Ping {} => vec![],
            StreamEvent::Error { error } => {
                vec![IrStreamEvent::Error {
                    code: error.error_type.clone(),
                    message: error.message.clone(),
                }]
            }
        }
    }
}

#[cfg(feature = "ir")]
pub use inner::*;
