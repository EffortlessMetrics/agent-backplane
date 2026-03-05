// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translation between Codex Responses API types and the `abp-dialect`
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

    use abp_codex_sdk::dialect::{
        CodexContentPart, CodexFunctionDef, CodexInputItem, CodexRequest, CodexResponse,
        CodexResponseItem, CodexStreamDelta, CodexStreamEvent, CodexTool, CodexUsage,
        ReasoningSummary,
    };

    // ── Role mapping ────────────────────────────────────────────────────

    fn role_to_ir(role: &str) -> IrRole {
        match role {
            "system" => IrRole::System,
            "assistant" => IrRole::Assistant,
            "tool" => IrRole::Tool,
            _ => IrRole::User,
        }
    }

    fn role_from_ir(role: IrRole) -> &'static str {
        match role {
            IrRole::System => "system",
            IrRole::User => "user",
            IrRole::Assistant => "assistant",
            IrRole::Tool => "user",
        }
    }

    // ── Tool definition mapping ─────────────────────────────────────────

    fn tool_def_to_ir(tool: &CodexTool) -> IrToolDefinition {
        match tool {
            CodexTool::Function { function } => IrToolDefinition {
                name: function.name.clone(),
                description: function.description.clone(),
                parameters: function.parameters.clone(),
            },
            CodexTool::CodeInterpreter {} => IrToolDefinition {
                name: "code_interpreter".into(),
                description: "Execute code in a sandboxed environment".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
            CodexTool::FileSearch { .. } => IrToolDefinition {
                name: "file_search".into(),
                description: "Search over uploaded files".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }
    }

    fn tool_def_from_ir(tool: &IrToolDefinition) -> CodexTool {
        CodexTool::Function {
            function: CodexFunctionDef {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
        }
    }

    // ── Usage mapping ───────────────────────────────────────────────────

    fn usage_to_ir(usage: &CodexUsage) -> IrUsage {
        IrUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    fn usage_from_ir(usage: &IrUsage) -> CodexUsage {
        CodexUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        }
    }

    // ── Status / stop-reason mapping ────────────────────────────────────

    fn status_to_ir(status: &str) -> IrStopReason {
        match status {
            "completed" => IrStopReason::EndTurn,
            "incomplete" => IrStopReason::MaxTokens,
            other => IrStopReason::Other(other.to_string()),
        }
    }

    fn stop_reason_to_status(reason: &IrStopReason) -> String {
        match reason {
            IrStopReason::EndTurn => "completed".into(),
            IrStopReason::MaxTokens => "incomplete".into(),
            IrStopReason::ToolUse => "completed".into(),
            IrStopReason::StopSequence => "completed".into(),
            IrStopReason::ContentFilter => "failed".into(),
            IrStopReason::Other(s) => s.clone(),
        }
    }

    // ── Response item → IR content blocks ──────────────────────────────

    fn response_item_to_ir_blocks(item: &CodexResponseItem) -> Vec<IrContentBlock> {
        match item {
            CodexResponseItem::Message { content, .. } => content
                .iter()
                .map(|part| match part {
                    CodexContentPart::OutputText { text } => {
                        IrContentBlock::Text { text: text.clone() }
                    }
                })
                .collect(),
            CodexResponseItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                let input = serde_json::from_str(arguments)
                    .unwrap_or(serde_json::Value::String(arguments.clone()));
                vec![IrContentBlock::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input,
                }]
            }
            CodexResponseItem::FunctionCallOutput { call_id, output } => {
                vec![IrContentBlock::ToolResult {
                    tool_call_id: call_id.clone(),
                    content: vec![IrContentBlock::Text {
                        text: output.clone(),
                    }],
                    is_error: false,
                }]
            }
            CodexResponseItem::Reasoning { summary } => {
                let text = summary
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.is_empty() {
                    vec![]
                } else {
                    vec![IrContentBlock::Thinking { text }]
                }
            }
        }
    }

    // ── IR content blocks → response items ─────────────────────────────

    fn ir_blocks_to_response_items(blocks: &[IrContentBlock]) -> Vec<CodexResponseItem> {
        let mut items: Vec<CodexResponseItem> = Vec::new();
        let mut text_parts: Vec<CodexContentPart> = Vec::new();

        for block in blocks {
            match block {
                IrContentBlock::Text { text } => {
                    text_parts.push(CodexContentPart::OutputText { text: text.clone() });
                }
                IrContentBlock::ToolCall { id, name, input } => {
                    if !text_parts.is_empty() {
                        items.push(CodexResponseItem::Message {
                            role: "assistant".into(),
                            content: std::mem::take(&mut text_parts),
                        });
                    }
                    items.push(CodexResponseItem::FunctionCall {
                        id: id.clone(),
                        call_id: None,
                        name: name.clone(),
                        arguments: input.to_string(),
                    });
                }
                IrContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    if !text_parts.is_empty() {
                        items.push(CodexResponseItem::Message {
                            role: "assistant".into(),
                            content: std::mem::take(&mut text_parts),
                        });
                    }
                    let output = content
                        .iter()
                        .filter_map(|c| c.as_text())
                        .collect::<Vec<_>>()
                        .join("");
                    items.push(CodexResponseItem::FunctionCallOutput {
                        call_id: tool_call_id.clone(),
                        output,
                    });
                }
                IrContentBlock::Thinking { text } => {
                    if !text_parts.is_empty() {
                        items.push(CodexResponseItem::Message {
                            role: "assistant".into(),
                            content: std::mem::take(&mut text_parts),
                        });
                    }
                    items.push(CodexResponseItem::Reasoning {
                        summary: vec![ReasoningSummary { text: text.clone() }],
                    });
                }
                IrContentBlock::Image { media_type, .. } => {
                    text_parts.push(CodexContentPart::OutputText {
                        text: format!("[image: {media_type}]"),
                    });
                }
                IrContentBlock::Audio { media_type, data } => {
                    text_parts.push(CodexContentPart::OutputText {
                        text: format!("[audio: {media_type}, {} bytes]", data.len()),
                    });
                }
                IrContentBlock::Custom { custom_type, data } => {
                    text_parts.push(CodexContentPart::OutputText {
                        text: format!("[custom:{custom_type}: {data}]"),
                    });
                }
            }
        }

        if !text_parts.is_empty() {
            items.push(CodexResponseItem::Message {
                role: "assistant".into(),
                content: text_parts,
            });
        }

        items
    }

    // ── Public API ──────────────────────────────────────────────────────

    /// Convert a Codex Responses API [`CodexRequest`] to an [`IrRequest`].
    ///
    /// System-role input items are extracted into the `system_prompt` field.
    /// Tool definitions, generation config (model, max_output_tokens,
    /// temperature), and vendor-specific extras (text format) are carried
    /// through metadata.
    pub fn codex_request_to_ir(req: &CodexRequest) -> IrRequest {
        let mut system_prompt: Option<String> = None;
        let mut messages: Vec<IrMessage> = Vec::new();

        for item in &req.input {
            match item {
                CodexInputItem::Message { role, content } if role == "system" => {
                    match &mut system_prompt {
                        Some(existing) => {
                            existing.push('\n');
                            existing.push_str(content);
                        }
                        None => {
                            system_prompt = Some(content.clone());
                        }
                    }
                }
                CodexInputItem::Message { role, content } => {
                    let ir_role = role_to_ir(role);
                    messages.push(IrMessage::new(
                        ir_role,
                        vec![IrContentBlock::Text {
                            text: content.clone(),
                        }],
                    ));
                }
            }
        }

        let tools: Vec<IrToolDefinition> = req.tools.iter().map(tool_def_to_ir).collect();

        let config = IrGenerationConfig {
            max_tokens: req.max_output_tokens.map(|t| t as u64),
            temperature: req.temperature,
            top_p: None,
            top_k: None,
            stop_sequences: Vec::new(),
            extra: BTreeMap::new(),
        };

        let mut metadata = BTreeMap::new();
        if let Some(ref text_format) = req.text {
            if let Ok(v) = serde_json::to_value(text_format) {
                metadata.insert("text_format".into(), v);
            }
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

    /// Convert an [`IrRequest`] to a Codex Responses API [`CodexRequest`].
    ///
    /// The system prompt is placed as the first input item with role `"system"`.
    /// `model` defaults to `"codex-mini-latest"` if not set.
    pub fn ir_to_codex_request(ir: &IrRequest) -> CodexRequest {
        let mut input: Vec<CodexInputItem> = Vec::new();

        if let Some(ref prompt) = ir.system_prompt {
            input.push(CodexInputItem::Message {
                role: "system".into(),
                content: prompt.clone(),
            });
        }

        for msg in &ir.messages {
            let role = role_from_ir(msg.role).to_string();
            let content: String = msg
                .content
                .iter()
                .filter_map(|b| match b {
                    IrContentBlock::Text { text } => Some(text.clone()),
                    IrContentBlock::ToolCall { name, input, .. } => {
                        Some(format!("[tool_call: {name} {input}]"))
                    }
                    IrContentBlock::ToolResult { content, .. } => {
                        let text = content
                            .iter()
                            .filter_map(|c| c.as_text())
                            .collect::<Vec<_>>()
                            .join("");
                        Some(text)
                    }
                    IrContentBlock::Thinking { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            if !content.is_empty() {
                input.push(CodexInputItem::Message { role, content });
            }
        }

        let tools: Vec<CodexTool> = ir.tools.iter().map(tool_def_from_ir).collect();

        let text = ir
            .metadata
            .get("text_format")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        CodexRequest {
            model: ir
                .model
                .clone()
                .unwrap_or_else(|| "codex-mini-latest".into()),
            input,
            max_output_tokens: ir.config.max_tokens.map(|t| t as u32),
            temperature: ir.config.temperature,
            tools,
            text,
        }
    }

    /// Convert a Codex Responses API [`CodexResponse`] to an [`IrResponse`].
    ///
    /// Output items (messages, function calls, function call outputs,
    /// reasoning) are flattened into IR content blocks. Status is mapped
    /// to an [`IrStopReason`].
    pub fn codex_response_to_ir(resp: &CodexResponse) -> IrResponse {
        let content: Vec<IrContentBlock> = resp
            .output
            .iter()
            .flat_map(response_item_to_ir_blocks)
            .collect();

        let stop_reason = resp.status.as_deref().map(status_to_ir);
        let usage = resp.usage.as_ref().map(usage_to_ir);

        IrResponse {
            id: Some(resp.id.clone()),
            model: Some(resp.model.clone()),
            content,
            stop_reason,
            usage,
            metadata: BTreeMap::new(),
        }
    }

    /// Convert an [`IrResponse`] to a Codex Responses API [`CodexResponse`].
    ///
    /// IR content blocks are grouped into Codex response items: text blocks
    /// become messages, tool calls become function calls, tool results become
    /// function call outputs, and thinking blocks become reasoning items.
    pub fn ir_to_codex_response(ir: &IrResponse) -> CodexResponse {
        let output = ir_blocks_to_response_items(&ir.content);
        let usage = ir.usage.as_ref().map(usage_from_ir);
        let status = ir.stop_reason.as_ref().map(stop_reason_to_status);

        CodexResponse {
            id: ir.id.clone().unwrap_or_default(),
            model: ir.model.clone().unwrap_or_default(),
            output,
            usage,
            status,
        }
    }

    /// Convert a Codex [`CodexStreamEvent`] to a `Vec<IrStreamEvent>`.
    ///
    /// Most events produce a single IR event; some (like `ResponseCompleted`)
    /// may produce multiple (usage + stream end). Events that carry no
    /// meaningful data for the IR (like `ResponseInProgress`) return an
    /// empty vec.
    pub fn codex_stream_to_ir(event: &CodexStreamEvent) -> Vec<IrStreamEvent> {
        match event {
            CodexStreamEvent::ResponseCreated { response } => {
                vec![IrStreamEvent::StreamStart {
                    id: Some(response.id.clone()),
                    model: Some(response.model.clone()),
                }]
            }
            CodexStreamEvent::ResponseInProgress { .. } => vec![],
            CodexStreamEvent::OutputItemAdded {
                output_index, item, ..
            } => {
                let block = match item {
                    CodexResponseItem::Message { content, .. } => {
                        if let Some(CodexContentPart::OutputText { text }) = content.first() {
                            IrContentBlock::Text { text: text.clone() }
                        } else {
                            IrContentBlock::Text {
                                text: String::new(),
                            }
                        }
                    }
                    CodexResponseItem::FunctionCall {
                        id,
                        name,
                        arguments,
                        ..
                    } => {
                        let input = serde_json::from_str(arguments)
                            .unwrap_or(serde_json::Value::String(arguments.clone()));
                        IrContentBlock::ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input,
                        }
                    }
                    CodexResponseItem::FunctionCallOutput { call_id, output } => {
                        IrContentBlock::ToolResult {
                            tool_call_id: call_id.clone(),
                            content: vec![IrContentBlock::Text {
                                text: output.clone(),
                            }],
                            is_error: false,
                        }
                    }
                    CodexResponseItem::Reasoning { summary } => {
                        let text = summary
                            .iter()
                            .map(|s| s.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        IrContentBlock::Thinking { text }
                    }
                };
                vec![IrStreamEvent::ContentBlockStart {
                    index: *output_index,
                    block,
                }]
            }
            CodexStreamEvent::OutputItemDelta {
                output_index,
                delta,
            } => {
                let idx = *output_index;
                match delta {
                    CodexStreamDelta::OutputTextDelta { text } => {
                        vec![IrStreamEvent::TextDelta {
                            index: idx,
                            text: text.clone(),
                        }]
                    }
                    CodexStreamDelta::FunctionCallArgumentsDelta { delta: args } => {
                        vec![IrStreamEvent::ToolCallDelta {
                            index: idx,
                            arguments_delta: args.clone(),
                        }]
                    }
                    CodexStreamDelta::ReasoningSummaryDelta { text } => {
                        vec![IrStreamEvent::ThinkingDelta {
                            index: idx,
                            text: text.clone(),
                        }]
                    }
                }
            }
            CodexStreamEvent::OutputItemDone { output_index, .. } => {
                vec![IrStreamEvent::ContentBlockStop {
                    index: *output_index,
                }]
            }
            CodexStreamEvent::ResponseCompleted { response } => {
                let mut events = Vec::new();
                if let Some(ref usage) = response.usage {
                    events.push(IrStreamEvent::Usage {
                        usage: usage_to_ir(usage),
                    });
                }
                let stop_reason = response.status.as_deref().map(status_to_ir);
                events.push(IrStreamEvent::StreamEnd { stop_reason });
                events
            }
            CodexStreamEvent::ResponseFailed { response } => {
                let message = response
                    .status
                    .as_deref()
                    .unwrap_or("unknown failure")
                    .to_string();
                vec![IrStreamEvent::Error {
                    code: "response_failed".into(),
                    message,
                }]
            }
            CodexStreamEvent::Error { message, code } => {
                vec![IrStreamEvent::Error {
                    code: code.clone().unwrap_or_else(|| "error".into()),
                    message: message.clone(),
                }]
            }
        }
    }
}

#[cfg(feature = "ir")]
pub use inner::*;
