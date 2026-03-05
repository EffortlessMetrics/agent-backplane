// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translation between OpenAI Chat Completions API types and the
//! `abp-sdk-types` Intermediate Representation (IR).
//!
//! This module is gated behind the `ir` feature because it depends on
//! `abp-sdk-types` for the IR types.

#[cfg(feature = "ir")]
mod inner {
    use std::collections::BTreeMap;

    use abp_sdk_types::ir::{
        IrContentPart, IrMessage, IrRole, IrToolCall, IrToolDefinition, IrUsage,
    };
    use abp_sdk_types::ir_request::{IrChatRequest, IrSamplingParams, IrStreamConfig};
    use abp_sdk_types::ir_response::{IrChatResponse, IrChoice, IrFinishReason, IrStreamChunk};

    use crate::openai_types::{
        ChatCompletionChoice, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse,
        ChatMessage, ChatMessageRole, FunctionCall, FunctionDefinition, ToolCall, ToolDefinition,
        Usage,
    };

    // ── Role mapping ────────────────────────────────────────────────────

    fn role_to_ir(role: ChatMessageRole) -> IrRole {
        match role {
            ChatMessageRole::System => IrRole::System,
            ChatMessageRole::User => IrRole::User,
            ChatMessageRole::Assistant => IrRole::Assistant,
            ChatMessageRole::Tool => IrRole::Tool,
        }
    }

    fn role_from_ir(role: IrRole) -> ChatMessageRole {
        match role {
            IrRole::System => ChatMessageRole::System,
            IrRole::User => ChatMessageRole::User,
            IrRole::Assistant => ChatMessageRole::Assistant,
            IrRole::Tool => ChatMessageRole::Tool,
        }
    }

    // ── Message mapping ─────────────────────────────────────────────────

    fn message_to_ir(msg: &ChatMessage) -> IrMessage {
        let role = role_to_ir(msg.role);
        let mut content = Vec::new();
        let mut tool_calls = Vec::new();

        // Tool-result messages → ToolResult content part
        if msg.role == ChatMessageRole::Tool {
            if let Some(ref call_id) = msg.tool_call_id {
                return IrMessage {
                    role: IrRole::Tool,
                    content: vec![IrContentPart::ToolResult {
                        call_id: call_id.clone(),
                        content: msg.content.clone().unwrap_or_default(),
                        is_error: false,
                    }],
                    tool_calls: Vec::new(),
                    metadata: BTreeMap::new(),
                };
            }
        }

        if let Some(ref text) = msg.content {
            if !text.is_empty() {
                content.push(IrContentPart::Text { text: text.clone() });
            }
        }

        if let Some(ref tcs) = msg.tool_calls {
            for tc in tcs {
                let arguments =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
                tool_calls.push(IrToolCall {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments,
                });
            }
        }

        IrMessage {
            role,
            content,
            tool_calls,
            metadata: BTreeMap::new(),
        }
    }

    fn message_from_ir(msg: &IrMessage) -> ChatMessage {
        let role = role_from_ir(msg.role);

        // ToolResult content parts → tool message
        for part in &msg.content {
            if let IrContentPart::ToolResult {
                call_id, content, ..
            } = part
            {
                return ChatMessage {
                    role: ChatMessageRole::Tool,
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content.clone())
                    },
                    tool_calls: None,
                    tool_call_id: Some(call_id.clone()),
                };
            }
        }

        let text_parts: Vec<String> = msg
            .content
            .iter()
            .filter_map(|p| match p {
                IrContentPart::Text { text } => Some(text.clone()),
                IrContentPart::Image { url: Some(u), .. } => Some(format!("[image: {u}]")),
                IrContentPart::Image {
                    media_type: Some(mt),
                    base64: Some(b64),
                    ..
                } => Some(format!("[image: {mt}, {} bytes]", b64.len())),
                _ => None,
            })
            .collect();

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        let tool_calls: Vec<ToolCall> = msg
            .tool_calls
            .iter()
            .map(|tc| ToolCall {
                id: tc.id.clone(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                },
            })
            .collect();

        ChatMessage {
            role,
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        }
    }

    // ── Tool definition mapping ─────────────────────────────────────────

    fn tool_def_to_ir(tool: &ToolDefinition) -> IrToolDefinition {
        IrToolDefinition {
            name: tool.function.name.clone(),
            description: tool.function.description.clone(),
            parameters: tool.function.parameters.clone(),
        }
    }

    fn tool_def_from_ir(tool: &IrToolDefinition) -> ToolDefinition {
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

    fn usage_to_ir(usage: &Usage) -> IrUsage {
        IrUsage::from_counts(usage.prompt_tokens, usage.completion_tokens)
    }

    fn usage_from_ir(usage: &IrUsage) -> Usage {
        Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.prompt_tokens + usage.completion_tokens,
        }
    }

    // ── Finish reason mapping ───────────────────────────────────────────

    fn finish_reason_to_ir(reason: &str) -> IrFinishReason {
        match reason {
            "stop" => IrFinishReason::Stop,
            "length" => IrFinishReason::Length,
            "tool_calls" => IrFinishReason::ToolUse,
            "content_filter" => IrFinishReason::ContentFilter,
            _ => IrFinishReason::Stop,
        }
    }

    fn finish_reason_from_ir(reason: IrFinishReason) -> String {
        match reason {
            IrFinishReason::Stop => "stop".into(),
            IrFinishReason::Length => "length".into(),
            IrFinishReason::ToolUse => "tool_calls".into(),
            IrFinishReason::ContentFilter => "content_filter".into(),
            IrFinishReason::Error => "stop".into(),
        }
    }

    // ── Public API ──────────────────────────────────────────────────────

    /// Convert an OpenAI [`ChatCompletionRequest`] to an [`IrChatRequest`].
    pub fn openai_request_to_ir(req: &ChatCompletionRequest) -> IrChatRequest {
        let messages: Vec<IrMessage> = req.messages.iter().map(message_to_ir).collect();

        let tools: Vec<IrToolDefinition> = req
            .tools
            .as_ref()
            .map(|t| t.iter().map(tool_def_to_ir).collect())
            .unwrap_or_default();

        let sampling = IrSamplingParams {
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: None,
            frequency_penalty: req.frequency_penalty,
            presence_penalty: req.presence_penalty,
        };

        let stop_sequences: Vec<String> = req.stop.clone().unwrap_or_default();

        let stream = IrStreamConfig {
            enabled: req.stream.unwrap_or(false),
            include_usage: None,
            extra: BTreeMap::new(),
        };

        let mut extra = BTreeMap::new();
        if let Some(n) = req.n {
            extra.insert("n".into(), serde_json::json!(n));
        }

        IrChatRequest {
            model: req.model.clone(),
            messages,
            max_tokens: req.max_tokens.map(|v| v as u64),
            tools,
            tool_choice: req.tool_choice.clone(),
            sampling,
            stop_sequences,
            stream,
            response_format: None,
            extra,
        }
    }

    /// Convert an [`IrChatRequest`] to an OpenAI [`ChatCompletionRequest`].
    pub fn ir_to_openai_request(ir: &IrChatRequest) -> ChatCompletionRequest {
        let messages: Vec<ChatMessage> = ir.messages.iter().map(message_from_ir).collect();

        let tools = if ir.tools.is_empty() {
            None
        } else {
            Some(ir.tools.iter().map(tool_def_from_ir).collect())
        };

        let stop = if ir.stop_sequences.is_empty() {
            None
        } else {
            Some(ir.stop_sequences.clone())
        };

        let n = ir.extra.get("n").and_then(|v| v.as_u64()).map(|v| v as u32);

        ChatCompletionRequest {
            model: ir.model.clone(),
            messages,
            tools,
            temperature: ir.sampling.temperature,
            max_tokens: ir.max_tokens.map(|v| v as u32),
            stream: if ir.stream.enabled { Some(true) } else { None },
            top_p: ir.sampling.top_p,
            frequency_penalty: ir.sampling.frequency_penalty,
            presence_penalty: ir.sampling.presence_penalty,
            stop,
            n,
            tool_choice: ir.tool_choice.clone(),
        }
    }

    /// Convert an OpenAI [`ChatCompletionResponse`] to an [`IrChatResponse`].
    pub fn openai_response_to_ir(resp: &ChatCompletionResponse) -> IrChatResponse {
        let choices: Vec<IrChoice> = resp
            .choices
            .iter()
            .map(|c| {
                let message = message_to_ir(&c.message);
                let finish_reason = c.finish_reason.as_deref().map(finish_reason_to_ir);
                IrChoice {
                    index: c.index,
                    message,
                    finish_reason,
                }
            })
            .collect();

        let usage = resp.usage.as_ref().map(usage_to_ir);

        IrChatResponse {
            id: Some(resp.id.clone()),
            model: Some(resp.model.clone()),
            choices,
            usage,
            metadata: BTreeMap::new(),
        }
    }

    /// Convert an [`IrChatResponse`] to an OpenAI [`ChatCompletionResponse`].
    pub fn ir_to_openai_response(ir: &IrChatResponse) -> ChatCompletionResponse {
        let choices: Vec<ChatCompletionChoice> = ir
            .choices
            .iter()
            .map(|c| {
                let message = message_from_ir(&c.message);
                let finish_reason = c.finish_reason.map(finish_reason_from_ir);
                ChatCompletionChoice {
                    index: c.index,
                    message,
                    finish_reason,
                }
            })
            .collect();

        let usage = ir.usage.as_ref().map(usage_from_ir);

        ChatCompletionResponse {
            id: ir.id.clone().unwrap_or_default(),
            object: "chat.completion".into(),
            created: 0,
            model: ir.model.clone().unwrap_or_default(),
            choices,
            usage,
        }
    }

    /// Convert an OpenAI [`ChatCompletionChunk`] to a `Vec<IrStreamChunk>`.
    ///
    /// Each `StreamChoice` in the chunk produces one `IrStreamChunk`.
    pub fn openai_stream_to_ir(chunk: &ChatCompletionChunk) -> Vec<IrStreamChunk> {
        chunk
            .choices
            .iter()
            .map(|sc| {
                let mut delta_content = Vec::new();
                let mut delta_tool_calls = Vec::new();

                if let Some(ref text) = sc.delta.content {
                    delta_content.push(IrContentPart::text(text));
                }

                if let Some(ref tcs) = sc.delta.tool_calls {
                    for tc in tcs {
                        let id = tc.id.clone().unwrap_or_default();
                        let name = tc
                            .function
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default();
                        let args_str = tc
                            .function
                            .as_ref()
                            .and_then(|f| f.arguments.clone())
                            .unwrap_or_default();
                        let arguments = serde_json::from_str(&args_str)
                            .unwrap_or(serde_json::Value::String(args_str));
                        delta_tool_calls.push(IrToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                }

                let role = sc.delta.role.as_deref().map(|r| match r {
                    "system" => IrRole::System,
                    "user" => IrRole::User,
                    "assistant" => IrRole::Assistant,
                    "tool" => IrRole::Tool,
                    _ => IrRole::Assistant,
                });

                let finish_reason = sc.finish_reason.as_deref().map(finish_reason_to_ir);

                IrStreamChunk {
                    id: Some(chunk.id.clone()),
                    model: Some(chunk.model.clone()),
                    index: sc.index,
                    delta_content,
                    delta_tool_calls,
                    role,
                    finish_reason,
                    usage: None,
                    metadata: BTreeMap::new(),
                }
            })
            .collect()
    }
}

#[cfg(feature = "ir")]
pub use inner::*;
