// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translation between GitHub Copilot Chat API types and the
//! `abp-sdk-types` Intermediate Representation (IR).
//!
//! This module is gated behind the `ir` feature because it depends on
//! `abp-sdk-types` for the IR types.
//!
//! Copilot extends the OpenAI-compatible chat completions format with
//! references (file, snippet, repository, web search), confirmations
//! (user approval prompts), and turn history for multi-turn agents.
//! These extensions are carried through IR metadata.

#[cfg(feature = "ir")]
mod inner {
    use std::collections::BTreeMap;

    use abp_sdk_types::ir::{
        IrContentPart, IrMessage, IrRole, IrToolCall, IrToolDefinition, IrUsage,
    };
    use abp_sdk_types::ir_request::{IrChatRequest, IrSamplingParams, IrStreamConfig};
    use abp_sdk_types::ir_response::{IrChatResponse, IrChoice, IrFinishReason, IrStreamChunk};

    use crate::copilot_types::{
        CopilotChatChoice, CopilotChatRequest, CopilotChatResponse, CopilotConfirmation,
        CopilotError, CopilotFunctionCall, CopilotFunctionDef, CopilotMessage, CopilotMessageRole,
        CopilotReference, CopilotStreamEvent, CopilotTool, CopilotToolCall, CopilotToolType,
        CopilotUsage,
    };

    // ── Role mapping ────────────────────────────────────────────────────

    fn role_to_ir(role: CopilotMessageRole) -> IrRole {
        match role {
            CopilotMessageRole::System => IrRole::System,
            CopilotMessageRole::User => IrRole::User,
            CopilotMessageRole::Assistant => IrRole::Assistant,
            CopilotMessageRole::Tool => IrRole::Tool,
        }
    }

    fn role_from_ir(role: IrRole) -> CopilotMessageRole {
        match role {
            IrRole::System => CopilotMessageRole::System,
            IrRole::User => CopilotMessageRole::User,
            IrRole::Assistant => CopilotMessageRole::Assistant,
            IrRole::Tool => CopilotMessageRole::Tool,
        }
    }

    // ── Message mapping ─────────────────────────────────────────────────

    fn message_to_ir(msg: &CopilotMessage) -> IrMessage {
        let role = role_to_ir(msg.role);
        let mut content = Vec::new();
        let mut tool_calls = Vec::new();

        // Tool-result messages → ToolResult content part
        if msg.role == CopilotMessageRole::Tool {
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

        // Carry Copilot references as metadata on the IR message
        if !msg.copilot_references.is_empty() {
            let mut metadata = BTreeMap::new();
            if let Ok(v) = serde_json::to_value(&msg.copilot_references) {
                metadata.insert("copilot_references".into(), v);
            }
            if let Some(ref tcs) = msg.tool_calls {
                for tc in tcs {
                    let arguments = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Null);
                    tool_calls.push(IrToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments,
                    });
                }
            }
            return IrMessage {
                role,
                content,
                tool_calls,
                metadata,
            };
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

    fn message_from_ir(msg: &IrMessage) -> CopilotMessage {
        let role = role_from_ir(msg.role);

        // ToolResult content parts → tool message
        for part in &msg.content {
            if let IrContentPart::ToolResult {
                call_id, content, ..
            } = part
            {
                return CopilotMessage {
                    role: CopilotMessageRole::Tool,
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content.clone())
                    },
                    name: None,
                    copilot_references: Vec::new(),
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

        let tool_calls: Vec<CopilotToolCall> = msg
            .tool_calls
            .iter()
            .map(|tc| CopilotToolCall {
                id: tc.id.clone(),
                call_type: "function".into(),
                function: CopilotFunctionCall {
                    name: tc.name.clone(),
                    arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                },
            })
            .collect();

        // Restore references from metadata
        let copilot_references: Vec<CopilotReference> = msg
            .metadata
            .get("copilot_references")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        CopilotMessage {
            role,
            content,
            name: None,
            copilot_references,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        }
    }

    // ── Tool definition mapping ─────────────────────────────────────────

    fn tool_def_to_ir(tool: &CopilotTool) -> Option<IrToolDefinition> {
        if tool.tool_type != CopilotToolType::Function {
            return None;
        }
        let func = tool.function.as_ref()?;
        Some(IrToolDefinition {
            name: func.name.clone(),
            description: func.description.clone(),
            parameters: func.parameters.clone(),
        })
    }

    fn tool_def_from_ir(tool: &IrToolDefinition) -> CopilotTool {
        CopilotTool {
            tool_type: CopilotToolType::Function,
            function: Some(CopilotFunctionDef {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            }),
        }
    }

    // ── Usage mapping ───────────────────────────────────────────────────

    fn usage_to_ir(usage: &CopilotUsage) -> IrUsage {
        IrUsage::from_counts(usage.prompt_tokens, usage.completion_tokens)
    }

    fn usage_from_ir(usage: &IrUsage) -> CopilotUsage {
        CopilotUsage {
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

    /// Convert a Copilot [`CopilotChatRequest`] to an [`IrChatRequest`].
    ///
    /// References are carried in the `extra` field under `"copilot_references"`.
    /// Turn history is carried under `"copilot_turn_history"`.
    /// Confirmation tools are carried under `"copilot_confirmation_tools"`.
    pub fn copilot_request_to_ir(req: &CopilotChatRequest) -> IrChatRequest {
        let messages: Vec<IrMessage> = req.messages.iter().map(message_to_ir).collect();

        let mut confirmation_tools = Vec::new();
        let tools: Vec<IrToolDefinition> = req
            .tools
            .as_ref()
            .map(|t| {
                t.iter()
                    .filter_map(|tool| {
                        if tool.tool_type == CopilotToolType::Confirmation {
                            if let Ok(v) = serde_json::to_value(tool) {
                                confirmation_tools.push(v);
                            }
                            None
                        } else {
                            tool_def_to_ir(tool)
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let sampling = IrSamplingParams {
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
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
        if !req.copilot_references.is_empty() {
            if let Ok(v) = serde_json::to_value(&req.copilot_references) {
                extra.insert("copilot_references".into(), v);
            }
        }
        if !req.turn_history.is_empty() {
            if let Ok(v) = serde_json::to_value(&req.turn_history) {
                extra.insert("copilot_turn_history".into(), v);
            }
        }
        if !confirmation_tools.is_empty() {
            extra.insert(
                "copilot_confirmation_tools".into(),
                serde_json::Value::Array(confirmation_tools),
            );
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

    /// Convert an [`IrChatRequest`] to a Copilot [`CopilotChatRequest`].
    ///
    /// Restores references, turn history, and confirmation tools from
    /// the IR `extra` fields.
    pub fn ir_to_copilot_request(ir: &IrChatRequest) -> CopilotChatRequest {
        let messages: Vec<CopilotMessage> = ir.messages.iter().map(message_from_ir).collect();

        let mut tools: Vec<CopilotTool> = ir.tools.iter().map(tool_def_from_ir).collect();

        // Restore confirmation tools from extra
        if let Some(ct) = ir.extra.get("copilot_confirmation_tools") {
            if let Ok(confirmation_tools) = serde_json::from_value::<Vec<CopilotTool>>(ct.clone()) {
                tools.extend(confirmation_tools);
            }
        }

        let stop = if ir.stop_sequences.is_empty() {
            None
        } else {
            Some(ir.stop_sequences.clone())
        };

        let n = ir.extra.get("n").and_then(|v| v.as_u64()).map(|v| v as u32);

        let copilot_references: Vec<CopilotReference> = ir
            .extra
            .get("copilot_references")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let turn_history = ir
            .extra
            .get("copilot_turn_history")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        CopilotChatRequest {
            model: ir.model.clone(),
            messages,
            tools: if tools.is_empty() { None } else { Some(tools) },
            temperature: ir.sampling.temperature,
            max_tokens: ir.max_tokens.map(|v| v as u32),
            stream: if ir.stream.enabled { Some(true) } else { None },
            top_p: ir.sampling.top_p,
            stop,
            n,
            tool_choice: ir.tool_choice.clone(),
            copilot_references,
            turn_history,
        }
    }

    /// Convert a Copilot [`CopilotChatResponse`] to an [`IrChatResponse`].
    ///
    /// References, errors, and confirmations are carried through metadata.
    pub fn copilot_response_to_ir(resp: &CopilotChatResponse) -> IrChatResponse {
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

        let mut metadata = BTreeMap::new();
        if !resp.copilot_references.is_empty() {
            if let Ok(v) = serde_json::to_value(&resp.copilot_references) {
                metadata.insert("copilot_references".into(), v);
            }
        }
        if !resp.copilot_errors.is_empty() {
            if let Ok(v) = serde_json::to_value(&resp.copilot_errors) {
                metadata.insert("copilot_errors".into(), v);
            }
        }
        if let Some(ref conf) = resp.copilot_confirmation {
            if let Ok(v) = serde_json::to_value(conf) {
                metadata.insert("copilot_confirmation".into(), v);
            }
        }

        IrChatResponse {
            id: resp.id.clone(),
            model: resp.model.clone(),
            choices,
            usage,
            metadata,
        }
    }

    /// Convert an [`IrChatResponse`] to a Copilot [`CopilotChatResponse`].
    ///
    /// Restores references, errors, and confirmations from IR metadata.
    pub fn ir_to_copilot_response(ir: &IrChatResponse) -> CopilotChatResponse {
        let choices: Vec<CopilotChatChoice> = ir
            .choices
            .iter()
            .map(|c| {
                let message = message_from_ir(&c.message);
                let finish_reason = c.finish_reason.map(finish_reason_from_ir);
                CopilotChatChoice {
                    index: c.index,
                    message,
                    finish_reason,
                }
            })
            .collect();

        let usage = ir.usage.as_ref().map(usage_from_ir);

        let copilot_references: Vec<CopilotReference> = ir
            .metadata
            .get("copilot_references")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let copilot_errors: Vec<CopilotError> = ir
            .metadata
            .get("copilot_errors")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let copilot_confirmation: Option<CopilotConfirmation> = ir
            .metadata
            .get("copilot_confirmation")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        CopilotChatResponse {
            id: ir.id.clone(),
            model: ir.model.clone(),
            choices,
            usage,
            copilot_references,
            copilot_errors,
            copilot_confirmation,
        }
    }

    /// Convert a Copilot [`CopilotStreamEvent`] to a `Vec<IrStreamChunk>`.
    ///
    /// `ChatCompletionChunk` events produce one `IrStreamChunk` per choice.
    /// Reference, error, confirmation, and done events are encoded as
    /// metadata-bearing chunks so no information is lost.
    pub fn copilot_stream_to_ir(event: &CopilotStreamEvent) -> Vec<IrStreamChunk> {
        match event {
            CopilotStreamEvent::ChatCompletionChunk { chunk } => chunk
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
                    let usage = chunk.usage.as_ref().map(usage_to_ir);

                    IrStreamChunk {
                        id: chunk.id.clone(),
                        model: chunk.model.clone(),
                        index: sc.index,
                        delta_content,
                        delta_tool_calls,
                        role,
                        finish_reason,
                        usage,
                        metadata: BTreeMap::new(),
                    }
                })
                .collect(),
            CopilotStreamEvent::CopilotReferences { references } => {
                let mut metadata = BTreeMap::new();
                if let Ok(v) = serde_json::to_value(references) {
                    metadata.insert("copilot_references".into(), v);
                }
                vec![IrStreamChunk {
                    id: None,
                    model: None,
                    index: 0,
                    delta_content: Vec::new(),
                    delta_tool_calls: Vec::new(),
                    role: None,
                    finish_reason: None,
                    usage: None,
                    metadata,
                }]
            }
            CopilotStreamEvent::CopilotErrors { errors } => {
                let mut metadata = BTreeMap::new();
                if let Ok(v) = serde_json::to_value(errors) {
                    metadata.insert("copilot_errors".into(), v);
                }
                vec![IrStreamChunk {
                    id: None,
                    model: None,
                    index: 0,
                    delta_content: Vec::new(),
                    delta_tool_calls: Vec::new(),
                    role: None,
                    finish_reason: Some(IrFinishReason::Error),
                    usage: None,
                    metadata,
                }]
            }
            CopilotStreamEvent::CopilotConfirmation { confirmation } => {
                let mut metadata = BTreeMap::new();
                if let Ok(v) = serde_json::to_value(confirmation) {
                    metadata.insert("copilot_confirmation".into(), v);
                }
                vec![IrStreamChunk {
                    id: None,
                    model: None,
                    index: 0,
                    delta_content: Vec::new(),
                    delta_tool_calls: Vec::new(),
                    role: None,
                    finish_reason: None,
                    usage: None,
                    metadata,
                }]
            }
            CopilotStreamEvent::Done {} => {
                vec![IrStreamChunk {
                    id: None,
                    model: None,
                    index: 0,
                    delta_content: Vec::new(),
                    delta_tool_calls: Vec::new(),
                    role: None,
                    finish_reason: Some(IrFinishReason::Stop),
                    usage: None,
                    metadata: BTreeMap::new(),
                }]
            }
        }
    }
}

#[cfg(feature = "ir")]
pub use inner::*;

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[cfg(feature = "ir")]
mod tests {
    use super::*;
    use crate::copilot_types::*;
    use abp_sdk_types::ir::{IrMessage, IrRole};
    use abp_sdk_types::ir_request::IrChatRequest;
    use abp_sdk_types::ir_response::{IrChatResponse, IrFinishReason};

    // ── Helper builders ─────────────────────────────────────────────

    fn simple_user_msg(text: &str) -> CopilotMessage {
        CopilotMessage {
            role: CopilotMessageRole::User,
            content: Some(text.into()),
            name: None,
            copilot_references: Vec::new(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn simple_assistant_msg(text: &str) -> CopilotMessage {
        CopilotMessage {
            role: CopilotMessageRole::Assistant,
            content: Some(text.into()),
            name: None,
            copilot_references: Vec::new(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn simple_system_msg(text: &str) -> CopilotMessage {
        CopilotMessage {
            role: CopilotMessageRole::System,
            content: Some(text.into()),
            name: None,
            copilot_references: Vec::new(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn tool_result_msg(call_id: &str, content: &str) -> CopilotMessage {
        CopilotMessage {
            role: CopilotMessageRole::Tool,
            content: Some(content.into()),
            name: None,
            copilot_references: Vec::new(),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
        }
    }

    fn minimal_request() -> CopilotChatRequest {
        CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![simple_user_msg("Hello")],
            tools: None,
            temperature: None,
            max_tokens: None,
            stream: None,
            top_p: None,
            stop: None,
            n: None,
            tool_choice: None,
            copilot_references: Vec::new(),
            turn_history: Vec::new(),
        }
    }

    fn file_reference(id: &str, path: &str) -> CopilotReference {
        CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: id.into(),
            data: serde_json::json!({ "path": path }),
            metadata: None,
        }
    }

    fn snippet_reference(id: &str, name: &str, content: &str) -> CopilotReference {
        CopilotReference {
            ref_type: CopilotReferenceType::Snippet,
            id: id.into(),
            data: serde_json::json!({ "name": name, "content": content }),
            metadata: None,
        }
    }

    fn function_tool(name: &str, desc: &str) -> CopilotTool {
        CopilotTool {
            tool_type: CopilotToolType::Function,
            function: Some(CopilotFunctionDef {
                name: name.into(),
                description: desc.into(),
                parameters: serde_json::json!({"type": "object"}),
            }),
        }
    }

    fn confirmation_tool() -> CopilotTool {
        CopilotTool {
            tool_type: CopilotToolType::Confirmation,
            function: None,
        }
    }

    // ── 1. Minimal request roundtrip ────────────────────────────────

    #[test]
    fn minimal_request_roundtrip() {
        let req = minimal_request();
        let ir = copilot_request_to_ir(&req);
        let back = ir_to_copilot_request(&ir);

        assert_eq!(back.model, "gpt-4o");
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.messages[0].content.as_deref(), Some("Hello"));
        assert_eq!(back.messages[0].role, CopilotMessageRole::User);
    }

    // ── 2. Model preserved ──────────────────────────────────────────

    #[test]
    fn model_preserved_in_ir() {
        let mut req = minimal_request();
        req.model = "o3-mini".into();
        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.model, "o3-mini");
        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.model, "o3-mini");
    }

    // ── 3. System message mapping ───────────────────────────────────

    #[test]
    fn system_message_maps_to_ir_system_role() {
        let mut req = minimal_request();
        req.messages.insert(0, simple_system_msg("Be helpful."));
        let ir = copilot_request_to_ir(&req);

        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "Be helpful.");
        assert_eq!(ir.messages[1].role, IrRole::User);
    }

    // ── 4. Multi-turn conversation ──────────────────────────────────

    #[test]
    fn multi_turn_conversation() {
        let mut req = minimal_request();
        req.messages = vec![
            simple_user_msg("What is 2+2?"),
            simple_assistant_msg("4"),
            simple_user_msg("And 3+3?"),
        ];
        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.messages.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[2].role, IrRole::User);
    }

    // ── 5. Tool definitions roundtrip ───────────────────────────────

    #[test]
    fn tool_definitions_roundtrip() {
        let mut req = minimal_request();
        req.tools = Some(vec![function_tool("read_file", "Read a file")]);
        let ir = copilot_request_to_ir(&req);

        assert_eq!(ir.tools.len(), 1);
        assert_eq!(ir.tools[0].name, "read_file");
        assert_eq!(ir.tools[0].description, "Read a file");

        let back = ir_to_copilot_request(&ir);
        let tools = back.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.as_ref().unwrap().name, "read_file");
    }

    // ── 6. Confirmation tools carried through ───────────────────────

    #[test]
    fn confirmation_tools_carried_through_extra() {
        let mut req = minimal_request();
        req.tools = Some(vec![function_tool("search", "Search"), confirmation_tool()]);

        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.tools.len(), 1); // Only function tools in IR tools
        assert!(ir.extra.contains_key("copilot_confirmation_tools"));

        let back = ir_to_copilot_request(&ir);
        let tools = back.tools.unwrap();
        assert_eq!(tools.len(), 2);
    }

    // ── 7. References in request ────────────────────────────────────

    #[test]
    fn request_references_roundtrip() {
        let mut req = minimal_request();
        req.copilot_references = vec![file_reference("f1", "src/main.rs")];

        let ir = copilot_request_to_ir(&req);
        assert!(ir.extra.contains_key("copilot_references"));

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.copilot_references.len(), 1);
        assert_eq!(back.copilot_references[0].id, "f1");
    }

    // ── 8. Turn history roundtrip ───────────────────────────────────

    #[test]
    fn turn_history_roundtrip() {
        let mut req = minimal_request();
        req.turn_history = vec![CopilotTurnEntry {
            request: "Hello".into(),
            response: "Hi there!".into(),
        }];

        let ir = copilot_request_to_ir(&req);
        assert!(ir.extra.contains_key("copilot_turn_history"));

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.turn_history.len(), 1);
        assert_eq!(back.turn_history[0].request, "Hello");
    }

    // ── 9. Sampling parameters ──────────────────────────────────────

    #[test]
    fn sampling_params_roundtrip() {
        let mut req = minimal_request();
        req.temperature = Some(0.7);
        req.top_p = Some(0.95);
        req.max_tokens = Some(2048);

        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.sampling.temperature, Some(0.7));
        assert_eq!(ir.sampling.top_p, Some(0.95));
        assert_eq!(ir.max_tokens, Some(2048));

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.temperature, Some(0.7));
        assert_eq!(back.top_p, Some(0.95));
        assert_eq!(back.max_tokens, Some(2048));
    }

    // ── 10. Stop sequences roundtrip ────────────────────────────────

    #[test]
    fn stop_sequences_roundtrip() {
        let mut req = minimal_request();
        req.stop = Some(vec!["STOP".into(), "END".into()]);

        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.stop_sequences, vec!["STOP", "END"]);

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.stop.unwrap(), vec!["STOP", "END"]);
    }

    // ── 11. Stream flag roundtrip ───────────────────────────────────

    #[test]
    fn stream_flag_roundtrip() {
        let mut req = minimal_request();
        req.stream = Some(true);

        let ir = copilot_request_to_ir(&req);
        assert!(ir.stream.enabled);

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.stream, Some(true));
    }

    // ── 12. Tool choice roundtrip ───────────────────────────────────

    #[test]
    fn tool_choice_roundtrip() {
        let mut req = minimal_request();
        req.tool_choice = Some(serde_json::json!("auto"));

        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.tool_choice, Some(serde_json::json!("auto")));

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.tool_choice, Some(serde_json::json!("auto")));
    }

    // ── 13. Tool call in message ────────────────────────────────────

    #[test]
    fn tool_call_message_roundtrip() {
        let msg = CopilotMessage {
            role: CopilotMessageRole::Assistant,
            content: None,
            name: None,
            copilot_references: Vec::new(),
            tool_calls: Some(vec![CopilotToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: CopilotFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"src/main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        };

        let mut req = minimal_request();
        req.messages = vec![simple_user_msg("Read main"), msg];

        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[1].tool_calls.len(), 1);
        assert_eq!(ir.messages[1].tool_calls[0].name, "read_file");

        let back = ir_to_copilot_request(&ir);
        let tc = back.messages[1].tool_calls.as_ref().unwrap();
        assert_eq!(tc[0].function.name, "read_file");
    }

    // ── 14. Tool result message roundtrip ───────────────────────────

    #[test]
    fn tool_result_message_roundtrip() {
        let mut req = minimal_request();
        req.messages
            .push(tool_result_msg("call_1", "file contents here"));

        let ir = copilot_request_to_ir(&req);
        let tool_msg = &ir.messages[1];
        assert_eq!(tool_msg.role, IrRole::Tool);
        assert!(tool_msg.content[0].is_tool_result());

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.messages[1].role, CopilotMessageRole::Tool);
        assert_eq!(back.messages[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(
            back.messages[1].content.as_deref(),
            Some("file contents here")
        );
    }

    // ── 15. N parameter roundtrip ───────────────────────────────────

    #[test]
    fn n_parameter_roundtrip() {
        let mut req = minimal_request();
        req.n = Some(3);

        let ir = copilot_request_to_ir(&req);
        assert_eq!(ir.extra.get("n"), Some(&serde_json::json!(3)));

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.n, Some(3));
    }

    // ── 16. Simple response roundtrip ───────────────────────────────

    #[test]
    fn simple_response_roundtrip() {
        let resp = CopilotChatResponse {
            id: Some("resp_1".into()),
            model: Some("gpt-4o".into()),
            choices: vec![CopilotChatChoice {
                index: 0,
                message: simple_assistant_msg("Hello!"),
                finish_reason: Some("stop".into()),
            }],
            usage: Some(CopilotUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            copilot_references: Vec::new(),
            copilot_errors: Vec::new(),
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        assert_eq!(ir.id, Some("resp_1".into()));
        assert_eq!(ir.model, Some("gpt-4o".into()));
        assert_eq!(ir.choices.len(), 1);
        assert_eq!(ir.choices[0].message.text_content(), "Hello!");
        assert_eq!(ir.choices[0].finish_reason, Some(IrFinishReason::Stop));
        assert_eq!(ir.usage.unwrap().prompt_tokens, 10);

        let back = ir_to_copilot_response(&ir);
        assert_eq!(back.id, Some("resp_1".into()));
        assert_eq!(back.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(back.usage.unwrap().prompt_tokens, 10);
    }

    // ── 17. Response with references ────────────────────────────────

    #[test]
    fn response_references_roundtrip() {
        let resp = CopilotChatResponse {
            id: None,
            model: None,
            choices: vec![CopilotChatChoice {
                index: 0,
                message: simple_assistant_msg("Here's the file."),
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            copilot_references: vec![file_reference("f1", "src/lib.rs")],
            copilot_errors: Vec::new(),
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        assert!(ir.metadata.contains_key("copilot_references"));

        let back = ir_to_copilot_response(&ir);
        assert_eq!(back.copilot_references.len(), 1);
        assert_eq!(back.copilot_references[0].id, "f1");
    }

    // ── 18. Response with errors ────────────────────────────────────

    #[test]
    fn response_errors_roundtrip() {
        let resp = CopilotChatResponse {
            id: None,
            model: None,
            choices: vec![],
            usage: None,
            copilot_references: Vec::new(),
            copilot_errors: vec![CopilotError {
                error_type: "rate_limit".into(),
                message: "Too many requests".into(),
                code: Some("429".into()),
                identifier: None,
            }],
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        assert!(ir.metadata.contains_key("copilot_errors"));

        let back = ir_to_copilot_response(&ir);
        assert_eq!(back.copilot_errors.len(), 1);
        assert_eq!(back.copilot_errors[0].error_type, "rate_limit");
        assert_eq!(back.copilot_errors[0].code.as_deref(), Some("429"));
    }

    // ── 19. Response with confirmation ──────────────────────────────

    #[test]
    fn response_confirmation_roundtrip() {
        let resp = CopilotChatResponse {
            id: None,
            model: None,
            choices: vec![CopilotChatChoice {
                index: 0,
                message: simple_assistant_msg("I need approval."),
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            copilot_references: Vec::new(),
            copilot_errors: Vec::new(),
            copilot_confirmation: Some(CopilotConfirmation {
                id: "conf_1".into(),
                title: "Delete file?".into(),
                message: "Are you sure you want to delete main.rs?".into(),
                accepted: None,
            }),
        };

        let ir = copilot_response_to_ir(&resp);
        assert!(ir.metadata.contains_key("copilot_confirmation"));

        let back = ir_to_copilot_response(&ir);
        let conf = back.copilot_confirmation.unwrap();
        assert_eq!(conf.id, "conf_1");
        assert_eq!(conf.title, "Delete file?");
    }

    // ── 20. Response with tool calls ────────────────────────────────

    #[test]
    fn response_with_tool_calls() {
        let msg = CopilotMessage {
            role: CopilotMessageRole::Assistant,
            content: None,
            name: None,
            copilot_references: Vec::new(),
            tool_calls: Some(vec![CopilotToolCall {
                id: "call_abc".into(),
                call_type: "function".into(),
                function: CopilotFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
            tool_call_id: None,
        };

        let resp = CopilotChatResponse {
            id: None,
            model: None,
            choices: vec![CopilotChatChoice {
                index: 0,
                message: msg,
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
            copilot_references: Vec::new(),
            copilot_errors: Vec::new(),
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        assert_eq!(ir.choices[0].message.tool_calls.len(), 1);
        assert_eq!(ir.choices[0].message.tool_calls[0].name, "search");
        assert_eq!(ir.choices[0].finish_reason, Some(IrFinishReason::ToolUse));
    }

    // ── 21. Finish reason mapping ───────────────────────────────────

    #[test]
    fn finish_reason_all_variants() {
        let cases = vec![
            ("stop", IrFinishReason::Stop),
            ("length", IrFinishReason::Length),
            ("tool_calls", IrFinishReason::ToolUse),
            ("content_filter", IrFinishReason::ContentFilter),
            ("unknown", IrFinishReason::Stop),
        ];
        for (copilot_reason, expected_ir) in &cases {
            let resp = CopilotChatResponse {
                id: None,
                model: None,
                choices: vec![CopilotChatChoice {
                    index: 0,
                    message: simple_assistant_msg("done"),
                    finish_reason: Some(copilot_reason.to_string()),
                }],
                usage: None,
                copilot_references: Vec::new(),
                copilot_errors: Vec::new(),
                copilot_confirmation: None,
            };
            let ir = copilot_response_to_ir(&resp);
            assert_eq!(ir.choices[0].finish_reason, Some(*expected_ir));
        }
    }

    // ── 22. Usage mapping ───────────────────────────────────────────

    #[test]
    fn usage_mapping_roundtrip() {
        let usage = CopilotUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let resp = CopilotChatResponse {
            id: None,
            model: None,
            choices: vec![CopilotChatChoice {
                index: 0,
                message: simple_assistant_msg("ok"),
                finish_reason: Some("stop".into()),
            }],
            usage: Some(usage),
            copilot_references: Vec::new(),
            copilot_errors: Vec::new(),
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        let ir_usage = ir.usage.unwrap();
        assert_eq!(ir_usage.prompt_tokens, 100);
        assert_eq!(ir_usage.completion_tokens, 50);
        assert_eq!(ir_usage.total_tokens, 150);

        let back = ir_to_copilot_response(&ir);
        let back_usage = back.usage.unwrap();
        assert_eq!(back_usage.prompt_tokens, 100);
        assert_eq!(back_usage.completion_tokens, 50);
        assert_eq!(back_usage.total_tokens, 150);
    }

    // ── 23. Stream text delta ───────────────────────────────────────

    #[test]
    fn stream_text_delta() {
        let event = CopilotStreamEvent::ChatCompletionChunk {
            chunk: CopilotStreamChunk {
                id: Some("chunk_1".into()),
                model: Some("gpt-4o".into()),
                choices: vec![CopilotStreamChoice {
                    index: 0,
                    delta: CopilotStreamDelta {
                        role: None,
                        content: Some("Hello".into()),
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
            },
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].delta_content[0].as_text(), Some("Hello"));
        assert_eq!(chunks[0].id, Some("chunk_1".into()));
    }

    // ── 24. Stream role delta ───────────────────────────────────────

    #[test]
    fn stream_role_delta() {
        let event = CopilotStreamEvent::ChatCompletionChunk {
            chunk: CopilotStreamChunk {
                id: None,
                model: None,
                choices: vec![CopilotStreamChoice {
                    index: 0,
                    delta: CopilotStreamDelta {
                        role: Some("assistant".into()),
                        content: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
            },
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks[0].role, Some(IrRole::Assistant));
    }

    // ── 25. Stream tool call delta ──────────────────────────────────

    #[test]
    fn stream_tool_call_delta() {
        let event = CopilotStreamEvent::ChatCompletionChunk {
            chunk: CopilotStreamChunk {
                id: None,
                model: None,
                choices: vec![CopilotStreamChoice {
                    index: 0,
                    delta: CopilotStreamDelta {
                        role: None,
                        content: None,
                        tool_calls: Some(vec![CopilotStreamToolCall {
                            index: 0,
                            id: Some("call_1".into()),
                            call_type: Some("function".into()),
                            function: Some(CopilotStreamFunctionCall {
                                name: Some("read_file".into()),
                                arguments: Some(r#"{"path":"src"}"#.into()),
                            }),
                        }]),
                    },
                    finish_reason: None,
                }],
                usage: None,
            },
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks[0].delta_tool_calls.len(), 1);
        assert_eq!(chunks[0].delta_tool_calls[0].name, "read_file");
        assert_eq!(chunks[0].delta_tool_calls[0].id, "call_1");
    }

    // ── 26. Stream finish reason ────────────────────────────────────

    #[test]
    fn stream_finish_reason() {
        let event = CopilotStreamEvent::ChatCompletionChunk {
            chunk: CopilotStreamChunk {
                id: None,
                model: None,
                choices: vec![CopilotStreamChoice {
                    index: 0,
                    delta: CopilotStreamDelta::default(),
                    finish_reason: Some("stop".into()),
                }],
                usage: None,
            },
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks[0].finish_reason, Some(IrFinishReason::Stop));
    }

    // ── 27. Stream with usage ───────────────────────────────────────

    #[test]
    fn stream_with_usage() {
        let event = CopilotStreamEvent::ChatCompletionChunk {
            chunk: CopilotStreamChunk {
                id: None,
                model: None,
                choices: vec![CopilotStreamChoice {
                    index: 0,
                    delta: CopilotStreamDelta::default(),
                    finish_reason: Some("stop".into()),
                }],
                usage: Some(CopilotUsage {
                    prompt_tokens: 50,
                    completion_tokens: 20,
                    total_tokens: 70,
                }),
            },
        };

        let chunks = copilot_stream_to_ir(&event);
        let usage = chunks[0].usage.unwrap();
        assert_eq!(usage.prompt_tokens, 50);
        assert_eq!(usage.completion_tokens, 20);
    }

    // ── 28. Stream references event ─────────────────────────────────

    #[test]
    fn stream_references_event() {
        let event = CopilotStreamEvent::CopilotReferences {
            references: vec![file_reference("f1", "src/main.rs")],
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].metadata.contains_key("copilot_references"));
        assert!(chunks[0].finish_reason.is_none());
    }

    // ── 29. Stream errors event ─────────────────────────────────────

    #[test]
    fn stream_errors_event() {
        let event = CopilotStreamEvent::CopilotErrors {
            errors: vec![CopilotError {
                error_type: "api_error".into(),
                message: "Something went wrong".into(),
                code: None,
                identifier: None,
            }],
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].metadata.contains_key("copilot_errors"));
        assert_eq!(chunks[0].finish_reason, Some(IrFinishReason::Error));
    }

    // ── 30. Stream confirmation event ───────────────────────────────

    #[test]
    fn stream_confirmation_event() {
        let event = CopilotStreamEvent::CopilotConfirmation {
            confirmation: CopilotConfirmation {
                id: "conf_1".into(),
                title: "Approve?".into(),
                message: "Do you approve this action?".into(),
                accepted: None,
            },
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].metadata.contains_key("copilot_confirmation"));
    }

    // ── 31. Stream done event ───────────────────────────────────────

    #[test]
    fn stream_done_event() {
        let event = CopilotStreamEvent::Done {};
        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].finish_reason, Some(IrFinishReason::Stop));
    }

    // ── 32. Message references carried through ──────────────────────

    #[test]
    fn message_references_carried_through() {
        let msg = CopilotMessage {
            role: CopilotMessageRole::User,
            content: Some("Explain this file".into()),
            name: None,
            copilot_references: vec![file_reference("f1", "lib.rs")],
            tool_calls: None,
            tool_call_id: None,
        };

        let mut req = minimal_request();
        req.messages = vec![msg];

        let ir = copilot_request_to_ir(&req);
        assert!(ir.messages[0].metadata.contains_key("copilot_references"));

        let back = ir_to_copilot_request(&ir);
        assert_eq!(back.messages[0].copilot_references.len(), 1);
        assert_eq!(back.messages[0].copilot_references[0].id, "f1");
    }

    // ── 33. Empty response ──────────────────────────────────────────

    #[test]
    fn empty_response_choices() {
        let resp = CopilotChatResponse {
            id: None,
            model: None,
            choices: vec![],
            usage: None,
            copilot_references: Vec::new(),
            copilot_errors: Vec::new(),
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        assert!(ir.choices.is_empty());
        assert!(ir.usage.is_none());

        let back = ir_to_copilot_response(&ir);
        assert!(back.choices.is_empty());
    }

    // ── 34. Multiple choices in response ────────────────────────────

    #[test]
    fn multiple_choices_response() {
        let resp = CopilotChatResponse {
            id: Some("resp_multi".into()),
            model: Some("gpt-4o".into()),
            choices: vec![
                CopilotChatChoice {
                    index: 0,
                    message: simple_assistant_msg("First"),
                    finish_reason: Some("stop".into()),
                },
                CopilotChatChoice {
                    index: 1,
                    message: simple_assistant_msg("Second"),
                    finish_reason: Some("stop".into()),
                },
            ],
            usage: None,
            copilot_references: Vec::new(),
            copilot_errors: Vec::new(),
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        assert_eq!(ir.choices.len(), 2);
        assert_eq!(ir.choices[0].message.text_content(), "First");
        assert_eq!(ir.choices[1].message.text_content(), "Second");

        let back = ir_to_copilot_response(&ir);
        assert_eq!(back.choices.len(), 2);
    }

    // ── 35. Snippet reference in message ────────────────────────────

    #[test]
    fn snippet_reference_in_message() {
        let msg = CopilotMessage {
            role: CopilotMessageRole::User,
            content: Some("What does this do?".into()),
            name: None,
            copilot_references: vec![snippet_reference(
                "s1",
                "auth.rs:10-20",
                "fn authenticate() { ... }",
            )],
            tool_calls: None,
            tool_call_id: None,
        };

        let mut req = minimal_request();
        req.messages = vec![msg];

        let ir = copilot_request_to_ir(&req);
        let refs_val = ir.messages[0].metadata.get("copilot_references").unwrap();
        let refs: Vec<CopilotReference> = serde_json::from_value(refs_val.clone()).unwrap();
        assert_eq!(refs[0].ref_type, CopilotReferenceType::Snippet);
        assert_eq!(refs[0].id, "s1");
    }

    // ── 36. Full request–response roundtrip ─────────────────────────

    #[test]
    fn full_request_response_roundtrip() {
        let req = CopilotChatRequest {
            model: "gpt-4o".into(),
            messages: vec![simple_system_msg("Be concise"), simple_user_msg("Hello")],
            tools: Some(vec![function_tool("search", "Search the web")]),
            temperature: Some(0.5),
            max_tokens: Some(1024),
            stream: Some(true),
            top_p: Some(0.9),
            stop: Some(vec!["STOP".into()]),
            n: Some(2),
            tool_choice: Some(serde_json::json!("auto")),
            copilot_references: vec![file_reference("f1", "main.rs")],
            turn_history: vec![CopilotTurnEntry {
                request: "prev".into(),
                response: "prev_resp".into(),
            }],
        };

        let ir = copilot_request_to_ir(&req);
        let back = ir_to_copilot_request(&ir);

        assert_eq!(back.model, "gpt-4o");
        assert_eq!(back.messages.len(), 2);
        assert_eq!(back.tools.as_ref().unwrap().len(), 1);
        assert_eq!(back.temperature, Some(0.5));
        assert_eq!(back.max_tokens, Some(1024));
        assert_eq!(back.stream, Some(true));
        assert_eq!(back.top_p, Some(0.9));
        assert_eq!(back.stop.as_ref().unwrap(), &vec!["STOP".to_string()]);
        assert_eq!(back.n, Some(2));
        assert_eq!(back.tool_choice, Some(serde_json::json!("auto")));
        assert_eq!(back.copilot_references.len(), 1);
        assert_eq!(back.turn_history.len(), 1);
    }

    // ── 37. Stream multiple choices in chunk ────────────────────────

    #[test]
    fn stream_multiple_choices() {
        let event = CopilotStreamEvent::ChatCompletionChunk {
            chunk: CopilotStreamChunk {
                id: Some("multi".into()),
                model: None,
                choices: vec![
                    CopilotStreamChoice {
                        index: 0,
                        delta: CopilotStreamDelta {
                            role: None,
                            content: Some("A".into()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    },
                    CopilotStreamChoice {
                        index: 1,
                        delta: CopilotStreamDelta {
                            role: None,
                            content: Some("B".into()),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    },
                ],
                usage: None,
            },
        };

        let chunks = copilot_stream_to_ir(&event);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].delta_content[0].as_text(), Some("A"));
        assert_eq!(chunks[1].index, 1);
        assert_eq!(chunks[1].delta_content[0].as_text(), Some("B"));
    }

    // ── 38. Response no usage ───────────────────────────────────────

    #[test]
    fn response_no_usage() {
        let resp = CopilotChatResponse {
            id: None,
            model: None,
            choices: vec![CopilotChatChoice {
                index: 0,
                message: simple_assistant_msg("ok"),
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            copilot_references: Vec::new(),
            copilot_errors: Vec::new(),
            copilot_confirmation: None,
        };

        let ir = copilot_response_to_ir(&resp);
        assert!(ir.usage.is_none());

        let back = ir_to_copilot_response(&ir);
        assert!(back.usage.is_none());
    }

    // ── 39. IR to copilot request defaults ──────────────────────────

    #[test]
    fn ir_to_copilot_request_defaults() {
        let ir = IrChatRequest::new("gpt-4o", vec![IrMessage::text(IrRole::User, "Hi")]);
        let req = ir_to_copilot_request(&ir);

        assert_eq!(req.model, "gpt-4o");
        assert!(req.tools.is_none());
        assert!(req.temperature.is_none());
        assert!(req.max_tokens.is_none());
        assert!(req.stream.is_none());
        assert!(req.stop.is_none());
        assert!(req.n.is_none());
        assert!(req.tool_choice.is_none());
        assert!(req.copilot_references.is_empty());
        assert!(req.turn_history.is_empty());
    }

    // ── 40. IR to copilot response defaults ─────────────────────────

    #[test]
    fn ir_to_copilot_response_defaults() {
        let ir = IrChatResponse::text("Hello");
        let resp = ir_to_copilot_response(&ir);

        assert!(resp.id.is_none());
        assert!(resp.model.is_none());
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello"));
        assert!(resp.copilot_references.is_empty());
        assert!(resp.copilot_errors.is_empty());
        assert!(resp.copilot_confirmation.is_none());
    }
}
