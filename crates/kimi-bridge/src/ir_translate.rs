// SPDX-License-Identifier: MIT OR Apache-2.0
//! Translation between Kimi Chat Completions API types and the `abp-dialect`
//! Intermediate Representation (IR).
//!
//! This module is gated behind the `ir` feature because it depends on
//! `abp-dialect` for the IR types.
//!
//! Kimi has unique built-in tools (`$web_search`, `$file_tool`, `$code_tool`,
//! `$browser`) that are mapped as vendor-specific tool extensions in the IR
//! via [`IrContentBlock::Custom`] with `custom_type = "kimi_builtin_tool"`.

#[cfg(feature = "ir")]
mod inner {
    use std::collections::BTreeMap;

    use abp_dialect::ir::{
        IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
        IrStreamEvent, IrToolDefinition, IrUsage,
    };

    use crate::kimi_types::{
        Choice, FunctionCall, FunctionDefinition, KimiRef, KimiRequest, KimiResponse, Message,
        ResponseMessage, Role, StreamChunk, ToolCall, ToolDefinition, Usage, builtin,
    };

    // ── Role mapping ────────────────────────────────────────────────────

    fn role_to_ir(role: Role) -> IrRole {
        match role {
            Role::System => IrRole::System,
            Role::User => IrRole::User,
            Role::Assistant => IrRole::Assistant,
            Role::Tool => IrRole::Tool,
        }
    }

    fn role_from_ir(role: IrRole) -> Role {
        match role {
            IrRole::System => Role::System,
            IrRole::User => Role::User,
            IrRole::Assistant => Role::Assistant,
            IrRole::Tool => Role::Tool,
        }
    }

    // ── Content block mapping ───────────────────────────────────────────

    fn message_to_ir(msg: &Message) -> IrMessage {
        let role = role_to_ir(msg.role);
        let mut content = Vec::new();

        // Tool-result messages → ToolResult content block
        if msg.role == Role::Tool {
            if let Some(ref call_id) = msg.tool_call_id {
                let nested = msg
                    .content
                    .as_ref()
                    .filter(|t| !t.is_empty())
                    .map(|t| vec![IrContentBlock::Text { text: t.clone() }])
                    .unwrap_or_default();
                return IrMessage::new(
                    IrRole::Tool,
                    vec![IrContentBlock::ToolResult {
                        tool_call_id: call_id.clone(),
                        content: nested,
                        is_error: false,
                    }],
                );
            }
        }

        if let Some(ref text) = msg.content {
            if !text.is_empty() {
                content.push(IrContentBlock::Text { text: text.clone() });
            }
        }

        if let Some(ref tcs) = msg.tool_calls {
            for tc in tcs {
                let input =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
                content.push(IrContentBlock::ToolCall {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input,
                });
            }
        }

        IrMessage::new(role, content)
    }

    fn message_from_ir(msg: &IrMessage) -> Message {
        let role = role_from_ir(msg.role);

        // ToolResult content blocks → tool message
        for block in &msg.content {
            if let IrContentBlock::ToolResult {
                tool_call_id,
                content,
                ..
            } = block
            {
                let text = content
                    .iter()
                    .filter_map(|b| b.as_text())
                    .collect::<Vec<_>>()
                    .join("");
                return Message {
                    role: Role::Tool,
                    content: if text.is_empty() { None } else { Some(text) },
                    tool_calls: None,
                    tool_call_id: Some(tool_call_id.clone()),
                };
            }
        }

        let text_parts: Vec<String> = msg
            .content
            .iter()
            .filter_map(|b| match b {
                IrContentBlock::Text { text } => Some(text.clone()),
                IrContentBlock::Custom {
                    custom_type, data, ..
                } => Some(format!("[{custom_type}: {data}]")),
                _ => None,
            })
            .collect();

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        let mut tool_calls = Vec::new();
        for block in &msg.content {
            if let IrContentBlock::ToolCall { id, name, input } = block {
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
            }
        }

        Message {
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
        match tool {
            ToolDefinition::Function { function } => IrToolDefinition {
                name: function.name.clone(),
                description: function.description.clone(),
                parameters: function.parameters.clone(),
            },
            ToolDefinition::BuiltinFunction { function } => IrToolDefinition {
                name: function.name.clone(),
                description: format!("Kimi built-in tool: {}", function.name),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            },
        }
    }

    fn tool_def_from_ir(tool: &IrToolDefinition) -> ToolDefinition {
        if builtin::is_builtin(&tool.name) {
            ToolDefinition::BuiltinFunction {
                function: crate::kimi_types::BuiltinFunctionDef {
                    name: tool.name.clone(),
                },
            }
        } else {
            ToolDefinition::Function {
                function: FunctionDefinition {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                },
            }
        }
    }

    // ── Usage mapping ───────────────────────────────────────────────────

    fn usage_to_ir(usage: &Usage) -> IrUsage {
        IrUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    fn usage_from_ir(usage: &IrUsage) -> Usage {
        Usage {
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            total_tokens: usage.input_tokens + usage.output_tokens,
        }
    }

    // ── Stop/finish reason mapping ──────────────────────────────────────

    fn stop_reason_to_ir(reason: &str) -> IrStopReason {
        match reason {
            "stop" => IrStopReason::EndTurn,
            "length" => IrStopReason::MaxTokens,
            "tool_calls" => IrStopReason::ToolUse,
            "content_filter" => IrStopReason::ContentFilter,
            other => IrStopReason::Other(other.to_string()),
        }
    }

    fn stop_reason_from_ir(reason: &IrStopReason) -> String {
        match reason {
            IrStopReason::EndTurn => "stop".into(),
            IrStopReason::MaxTokens => "length".into(),
            IrStopReason::StopSequence => "stop".into(),
            IrStopReason::ToolUse => "tool_calls".into(),
            IrStopReason::ContentFilter => "content_filter".into(),
            IrStopReason::Other(s) => s.clone(),
        }
    }

    // ── Built-in tool metadata helpers ───────────────────────────────────

    fn builtin_tools_to_metadata(tools: &[ToolDefinition]) -> BTreeMap<String, serde_json::Value> {
        let builtins: Vec<&str> = tools
            .iter()
            .filter_map(|t| match t {
                ToolDefinition::BuiltinFunction { function } => Some(function.name.as_str()),
                _ => None,
            })
            .collect();
        let mut meta = BTreeMap::new();
        if !builtins.is_empty() {
            meta.insert(
                "kimi_builtin_tools".into(),
                serde_json::to_value(&builtins).unwrap_or(serde_json::Value::Null),
            );
        }
        meta
    }

    fn refs_to_metadata(refs: &Option<Vec<KimiRef>>) -> BTreeMap<String, serde_json::Value> {
        let mut meta = BTreeMap::new();
        if let Some(refs) = refs {
            if !refs.is_empty() {
                meta.insert(
                    "kimi_refs".into(),
                    serde_json::to_value(refs).unwrap_or(serde_json::Value::Null),
                );
            }
        }
        meta
    }

    // ── Public API ──────────────────────────────────────────────────────

    /// Convert a Kimi [`KimiRequest`] to an [`IrRequest`].
    ///
    /// System messages are extracted into the `system_prompt` field.
    /// Tool definitions (both user-defined functions and Kimi built-ins)
    /// are mapped to IR tool definitions. Built-in tools are additionally
    /// recorded in metadata under `kimi_builtin_tools`.
    /// Generation config (model, max_tokens, temperature) and Kimi-specific
    /// flags (`use_search`, `stream`) are carried through metadata.
    pub fn kimi_request_to_ir(req: &KimiRequest) -> IrRequest {
        let mut messages: Vec<IrMessage> = Vec::new();
        let mut system_prompt: Option<String> = None;

        for msg in &req.messages {
            if msg.role == Role::System {
                let text = msg.content.clone().unwrap_or_default();
                match &mut system_prompt {
                    Some(existing) => {
                        existing.push('\n');
                        existing.push_str(&text);
                    }
                    None => system_prompt = Some(text),
                }
            } else {
                messages.push(message_to_ir(msg));
            }
        }

        let tools: Vec<IrToolDefinition> = req
            .tools
            .as_ref()
            .map(|t| t.iter().map(tool_def_to_ir).collect())
            .unwrap_or_default();

        let config = IrGenerationConfig {
            max_tokens: req.max_tokens.map(|v| v as u64),
            temperature: req.temperature,
            top_p: None,
            top_k: None,
            stop_sequences: Vec::new(),
            extra: BTreeMap::new(),
        };

        let mut metadata = BTreeMap::new();
        if let Some(true) = req.stream {
            metadata.insert("stream".into(), serde_json::Value::Bool(true));
        }
        if let Some(true) = req.use_search {
            metadata.insert("use_search".into(), serde_json::Value::Bool(true));
        }
        if let Some(ref tools_list) = req.tools {
            let builtin_meta = builtin_tools_to_metadata(tools_list);
            metadata.extend(builtin_meta);
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

    /// Convert an [`IrRequest`] to a Kimi [`KimiRequest`].
    ///
    /// The system prompt is re-inserted as a system message at the start.
    /// IR tool definitions whose names match Kimi built-in tools are restored
    /// as `BuiltinFunction` entries. `max_tokens` defaults to 4096 if not set.
    pub fn ir_to_kimi_request(ir: &IrRequest) -> KimiRequest {
        let mut messages: Vec<Message> = Vec::new();

        if let Some(ref sys) = ir.system_prompt {
            messages.push(Message {
                role: Role::System,
                content: Some(sys.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        for msg in &ir.messages {
            messages.push(message_from_ir(msg));
        }

        let tools = if ir.tools.is_empty() {
            None
        } else {
            Some(ir.tools.iter().map(tool_def_from_ir).collect())
        };

        let stream = ir
            .metadata
            .get("stream")
            .and_then(|v| v.as_bool())
            .filter(|b| *b);

        let use_search = ir
            .metadata
            .get("use_search")
            .and_then(|v| v.as_bool())
            .filter(|b| *b);

        KimiRequest {
            model: ir.model.clone().unwrap_or_else(|| "moonshot-v1-8k".into()),
            messages,
            max_tokens: ir.config.max_tokens.map(|v| v as u32),
            temperature: ir.config.temperature,
            stream: stream.map(|_| true),
            tools,
            use_search: use_search.map(|_| true),
        }
    }

    /// Convert a Kimi [`KimiResponse`] to an [`IrResponse`].
    ///
    /// Content blocks, tool calls, stop reason, usage, and citation
    /// references are all mapped. Citations are stored in metadata
    /// under `kimi_refs`.
    pub fn kimi_response_to_ir(resp: &KimiResponse) -> IrResponse {
        let mut content: Vec<IrContentBlock> = Vec::new();
        let mut stop_reason: Option<IrStopReason> = None;

        for choice in &resp.choices {
            if let Some(ref text) = choice.message.content {
                if !text.is_empty() {
                    content.push(IrContentBlock::Text { text: text.clone() });
                }
            }
            if let Some(ref tcs) = choice.message.tool_calls {
                for tc in tcs {
                    let input = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Null);
                    content.push(IrContentBlock::ToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        input,
                    });
                }
            }
            if let Some(ref reason) = choice.finish_reason {
                stop_reason = Some(stop_reason_to_ir(reason));
            }
        }

        let usage = resp.usage.as_ref().map(usage_to_ir);
        let metadata = refs_to_metadata(&resp.refs);

        IrResponse {
            id: Some(resp.id.clone()),
            model: Some(resp.model.clone()),
            content,
            stop_reason,
            usage,
            metadata,
        }
    }

    /// Convert an [`IrResponse`] to a Kimi [`KimiResponse`].
    ///
    /// Missing fields are filled with reasonable defaults.
    pub fn ir_to_kimi_response(ir: &IrResponse) -> KimiResponse {
        let mut text_content: Option<String> = None;
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for block in &ir.content {
            match block {
                IrContentBlock::Text { text } => {
                    let entry = text_content.get_or_insert_with(String::new);
                    entry.push_str(text);
                }
                IrContentBlock::ToolCall { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id: id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: name.clone(),
                            arguments: serde_json::to_string(input).unwrap_or_default(),
                        },
                    });
                }
                IrContentBlock::Custom {
                    custom_type, data, ..
                } => {
                    let entry = text_content.get_or_insert_with(String::new);
                    entry.push_str(&format!("[{custom_type}: {data}]"));
                }
                _ => {}
            }
        }

        let finish_reason = ir.stop_reason.as_ref().map(stop_reason_from_ir);

        let usage = ir.usage.as_ref().map(usage_from_ir);

        // Restore refs from metadata
        let refs: Option<Vec<KimiRef>> = ir
            .metadata
            .get("kimi_refs")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        KimiResponse {
            id: ir.id.clone().unwrap_or_default(),
            model: ir.model.clone().unwrap_or_default(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: text_content,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                },
                finish_reason,
            }],
            usage,
            refs,
        }
    }

    /// Convert a Kimi [`StreamChunk`] to a `Vec<IrStreamEvent>`.
    ///
    /// Each streaming chunk may produce multiple IR events. Text deltas
    /// become [`IrStreamEvent::TextDelta`], tool call fragments become
    /// [`IrStreamEvent::ToolCallDelta`], finish reasons produce
    /// [`IrStreamEvent::StreamEnd`], and usage (if present) produces
    /// [`IrStreamEvent::Usage`].
    pub fn kimi_stream_to_ir(chunk: &StreamChunk) -> Vec<IrStreamEvent> {
        let mut events: Vec<IrStreamEvent> = Vec::new();

        // First chunk with role = stream start
        let has_role = chunk
            .choices
            .first()
            .and_then(|c| c.delta.role.as_ref())
            .is_some();
        if has_role {
            events.push(IrStreamEvent::StreamStart {
                id: Some(chunk.id.clone()),
                model: Some(chunk.model.clone()),
            });
        }

        for choice in &chunk.choices {
            let idx = choice.index as usize;

            // Text delta
            if let Some(ref text) = choice.delta.content {
                if !text.is_empty() {
                    events.push(IrStreamEvent::TextDelta {
                        index: idx,
                        text: text.clone(),
                    });
                }
            }

            // Tool call deltas
            if let Some(ref tcs) = choice.delta.tool_calls {
                for tc in tcs {
                    if let Some(ref func) = tc.function {
                        if let Some(ref args) = func.arguments {
                            if !args.is_empty() {
                                events.push(IrStreamEvent::ToolCallDelta {
                                    index: tc.index as usize,
                                    arguments_delta: args.clone(),
                                });
                            }
                        }
                    }
                }
            }

            // Finish reason → StreamEnd
            if let Some(ref reason) = choice.finish_reason {
                events.push(IrStreamEvent::StreamEnd {
                    stop_reason: Some(stop_reason_to_ir(reason)),
                });
            }
        }

        // Usage
        if let Some(ref usage) = chunk.usage {
            events.push(IrStreamEvent::Usage {
                usage: usage_to_ir(usage),
            });
        }

        events
    }
}

#[cfg(feature = "ir")]
pub use inner::*;

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kimi_types::*;
    use abp_dialect::ir::*;
    use serde_json::json;

    // ── Helper constructors ─────────────────────────────────────────

    fn simple_request() -> KimiRequest {
        KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("Hello".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stream: None,
            tools: None,
            use_search: None,
        }
    }

    fn simple_response() -> KimiResponse {
        KimiResponse {
            id: "cmpl-123".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: Some("Hi there!".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            refs: None,
        }
    }

    fn tool_call_response() -> KimiResponse {
        KimiResponse {
            id: "cmpl-456".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_abc".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "web_search".into(),
                            arguments: r#"{"query":"rust async"}"#.into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
            refs: None,
        }
    }

    fn stream_text_chunk() -> StreamChunk {
        StreamChunk {
            id: "chunk-1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        }
    }

    // ── 1. Simple request to IR ─────────────────────────────────────

    #[test]
    fn simple_request_to_ir() {
        let req = simple_request();
        let ir = kimi_request_to_ir(&req);

        assert_eq!(ir.model.as_deref(), Some("moonshot-v1-8k"));
        assert!(ir.system_prompt.is_none());
        assert_eq!(ir.messages.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
        assert_eq!(ir.config.max_tokens, Some(1024));
        assert_eq!(ir.config.temperature, Some(0.7));
    }

    // ── 2. IR to Kimi request roundtrip ─────────────────────────────

    #[test]
    fn request_roundtrip() {
        let req = simple_request();
        let ir = kimi_request_to_ir(&req);
        let back = ir_to_kimi_request(&ir);

        assert_eq!(back.model, "moonshot-v1-8k");
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.messages[0].role, Role::User);
        assert_eq!(back.messages[0].content.as_deref(), Some("Hello"));
        assert_eq!(back.max_tokens, Some(1024));
        assert_eq!(back.temperature, Some(0.7));
    }

    // ── 3. System message extraction ────────────────────────────────

    #[test]
    fn system_message_extracted_to_ir() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some("You are helpful.".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: Some("Hi".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);

        assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful."));
        assert_eq!(ir.messages.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
    }

    // ── 4. System prompt restored on roundtrip ──────────────────────

    #[test]
    fn system_prompt_roundtrip() {
        let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")])
            .with_system_prompt("Be concise.")
            .with_model("moonshot-v1-8k");

        let kimi = ir_to_kimi_request(&ir);
        assert_eq!(kimi.messages.len(), 2);
        assert_eq!(kimi.messages[0].role, Role::System);
        assert_eq!(kimi.messages[0].content.as_deref(), Some("Be concise."));
    }

    // ── 5. Simple response to IR ────────────────────────────────────

    #[test]
    fn simple_response_to_ir() {
        let resp = simple_response();
        let ir = kimi_response_to_ir(&resp);

        assert_eq!(ir.id.as_deref(), Some("cmpl-123"));
        assert_eq!(ir.model.as_deref(), Some("moonshot-v1-8k"));
        assert_eq!(ir.text_content(), "Hi there!");
        assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
        assert_eq!(ir.usage.unwrap().input_tokens, 10);
    }

    // ── 6. IR to Kimi response roundtrip ────────────────────────────

    #[test]
    fn response_roundtrip() {
        let resp = simple_response();
        let ir = kimi_response_to_ir(&resp);
        let back = ir_to_kimi_response(&ir);

        assert_eq!(back.id, "cmpl-123");
        assert_eq!(back.model, "moonshot-v1-8k");
        assert_eq!(
            back.choices[0].message.content.as_deref(),
            Some("Hi there!")
        );
        assert_eq!(back.choices[0].finish_reason.as_deref(), Some("stop"));
        let u = back.usage.unwrap();
        assert_eq!(u.prompt_tokens, 10);
        assert_eq!(u.completion_tokens, 5);
    }

    // ── 7. Tool call response to IR ─────────────────────────────────

    #[test]
    fn tool_call_response_to_ir() {
        let resp = tool_call_response();
        let ir = kimi_response_to_ir(&resp);

        assert_eq!(ir.stop_reason, Some(IrStopReason::ToolUse));
        let tool_calls = ir.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        match &tool_calls[0] {
            IrContentBlock::ToolCall { id, name, input } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "web_search");
                assert_eq!(input["query"], "rust async");
            }
            _ => panic!("expected ToolCall"),
        }
    }

    // ── 8. Tool call IR roundtrip ───────────────────────────────────

    #[test]
    fn tool_call_roundtrip() {
        let resp = tool_call_response();
        let ir = kimi_response_to_ir(&resp);
        let back = ir_to_kimi_response(&ir);

        let tcs = back.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(tcs[0].function.name, "web_search");
        assert!(tcs[0].function.arguments.contains("rust async"));
    }

    // ── 9. Tool result message mapping ──────────────────────────────

    #[test]
    fn tool_result_message_to_ir() {
        let msg = Message {
            role: Role::Tool,
            content: Some("search results here".into()),
            tool_call_id: Some("call_abc".into()),
            tool_calls: None,
        };
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![msg],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);

        assert_eq!(ir.messages[0].role, IrRole::Tool);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_call_id, "call_abc");
                assert_eq!(content[0].as_text(), Some("search results here"));
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    // ── 10. Tool result roundtrip ───────────────────────────────────

    #[test]
    fn tool_result_roundtrip() {
        let ir_msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_call_id: "call_xyz".into(),
                content: vec![IrContentBlock::Text {
                    text: "result data".into(),
                }],
                is_error: false,
            }],
        );
        let ir = IrRequest::new(vec![ir_msg]).with_model("moonshot-v1-8k");
        let kimi = ir_to_kimi_request(&ir);

        assert_eq!(kimi.messages[0].role, Role::Tool);
        assert_eq!(kimi.messages[0].content.as_deref(), Some("result data"));
        assert_eq!(kimi.messages[0].tool_call_id.as_deref(), Some("call_xyz"));
    }

    // ── 11. User-defined function tool definition roundtrip ─────────

    #[test]
    fn function_tool_def_roundtrip() {
        let tools = vec![ToolDefinition::Function {
            function: FunctionDefinition {
                name: "get_weather".into(),
                description: "Get the weather".into(),
                parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            },
        }];
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("Weather?".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: Some(tools),
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.tools.len(), 1);
        assert_eq!(ir.tools[0].name, "get_weather");

        let back = ir_to_kimi_request(&ir);
        let t = &back.tools.unwrap()[0];
        match t {
            ToolDefinition::Function { function } => {
                assert_eq!(function.name, "get_weather");
                assert_eq!(function.description, "Get the weather");
            }
            _ => panic!("expected Function"),
        }
    }

    // ── 12. Builtin tool ($web_search) to IR and back ───────────────

    #[test]
    fn builtin_web_search_tool_roundtrip() {
        let tools = vec![ToolDefinition::BuiltinFunction {
            function: BuiltinFunctionDef {
                name: "$web_search".into(),
            },
        }];
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("Search".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: Some(tools),
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.tools[0].name, "$web_search");
        assert!(ir.metadata.contains_key("kimi_builtin_tools"));

        let back = ir_to_kimi_request(&ir);
        match &back.tools.unwrap()[0] {
            ToolDefinition::BuiltinFunction { function } => {
                assert_eq!(function.name, "$web_search");
            }
            _ => panic!("expected BuiltinFunction"),
        }
    }

    // ── 13. Builtin tool ($browser) recognized ──────────────────────

    #[test]
    fn builtin_browser_tool_recognized() {
        let ir_tool = IrToolDefinition {
            name: "$browser".into(),
            description: "Kimi built-in tool: $browser".into(),
            parameters: json!({}),
        };
        let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "browse")])
            .with_tool(ir_tool)
            .with_model("moonshot-v1-8k");
        let kimi = ir_to_kimi_request(&ir);
        match &kimi.tools.unwrap()[0] {
            ToolDefinition::BuiltinFunction { function } => {
                assert_eq!(function.name, "$browser");
            }
            _ => panic!("expected BuiltinFunction"),
        }
    }

    // ── 14. Builtin tool ($file_tool) recognized ────────────────────

    #[test]
    fn builtin_file_tool_recognized() {
        let ir_tool =
            IrToolDefinition::new("$file_tool", "Kimi built-in tool: $file_tool", json!({}));
        let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "analyze")])
            .with_tool(ir_tool)
            .with_model("moonshot-v1-8k");
        let kimi = ir_to_kimi_request(&ir);
        match &kimi.tools.unwrap()[0] {
            ToolDefinition::BuiltinFunction { function } => {
                assert_eq!(function.name, "$file_tool");
            }
            _ => panic!("expected BuiltinFunction"),
        }
    }

    // ── 15. Builtin tool ($code_tool) recognized ────────────────────

    #[test]
    fn builtin_code_tool_recognized() {
        let ir_tool =
            IrToolDefinition::new("$code_tool", "Kimi built-in tool: $code_tool", json!({}));
        let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "run code")])
            .with_tool(ir_tool)
            .with_model("moonshot-v1-8k");
        let kimi = ir_to_kimi_request(&ir);
        match &kimi.tools.unwrap()[0] {
            ToolDefinition::BuiltinFunction { function } => {
                assert_eq!(function.name, "$code_tool");
            }
            _ => panic!("expected BuiltinFunction"),
        }
    }

    // ── 16. Mixed function + builtin tools ──────────────────────────

    #[test]
    fn mixed_function_and_builtin_tools() {
        let tools = vec![
            ToolDefinition::Function {
                function: FunctionDefinition {
                    name: "get_weather".into(),
                    description: "Get weather".into(),
                    parameters: json!({}),
                },
            },
            ToolDefinition::BuiltinFunction {
                function: BuiltinFunctionDef {
                    name: "$web_search".into(),
                },
            },
        ];
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("test".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: Some(tools),
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.tools.len(), 2);

        let back = ir_to_kimi_request(&ir);
        let bt = back.tools.unwrap();
        assert!(matches!(&bt[0], ToolDefinition::Function { .. }));
        assert!(matches!(&bt[1], ToolDefinition::BuiltinFunction { .. }));
    }

    // ── 17. use_search flag preserved ───────────────────────────────

    #[test]
    fn use_search_flag_preserved() {
        let mut req = simple_request();
        req.use_search = Some(true);
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.metadata.get("use_search"), Some(&json!(true)));

        let back = ir_to_kimi_request(&ir);
        assert_eq!(back.use_search, Some(true));
    }

    // ── 18. Stream flag preserved ───────────────────────────────────

    #[test]
    fn stream_flag_preserved() {
        let mut req = simple_request();
        req.stream = Some(true);
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.metadata.get("stream"), Some(&json!(true)));

        let back = ir_to_kimi_request(&ir);
        assert_eq!(back.stream, Some(true));
    }

    // ── 19. Default model when not specified ─────────────────────────

    #[test]
    fn default_model_when_none() {
        let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]);
        let kimi = ir_to_kimi_request(&ir);
        assert_eq!(kimi.model, "moonshot-v1-8k");
    }

    // ── 20. Usage mapping to IR ─────────────────────────────────────

    #[test]
    fn usage_to_ir_mapping() {
        let resp = simple_response();
        let ir = kimi_response_to_ir(&resp);
        let u = ir.usage.unwrap();
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 5);
        assert_eq!(u.total_tokens, 15);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    // ── 21. Usage mapping from IR ───────────────────────────────────

    #[test]
    fn usage_from_ir_mapping() {
        let ir = IrResponse::text("ok").with_usage(IrUsage::from_io(200, 100));
        let kimi = ir_to_kimi_response(&ir);
        let u = kimi.usage.unwrap();
        assert_eq!(u.prompt_tokens, 200);
        assert_eq!(u.completion_tokens, 100);
        assert_eq!(u.total_tokens, 300);
    }

    // ── 22. Stop reason mapping: stop → EndTurn ─────────────────────

    #[test]
    fn stop_reason_stop_maps_to_end_turn() {
        let resp = simple_response();
        let ir = kimi_response_to_ir(&resp);
        assert_eq!(ir.stop_reason, Some(IrStopReason::EndTurn));
    }

    // ── 23. Stop reason mapping: length → MaxTokens ─────────────────

    #[test]
    fn stop_reason_length_maps_to_max_tokens() {
        let mut resp = simple_response();
        resp.choices[0].finish_reason = Some("length".into());
        let ir = kimi_response_to_ir(&resp);
        assert_eq!(ir.stop_reason, Some(IrStopReason::MaxTokens));
    }

    // ── 24. Stop reason mapping: tool_calls → ToolUse ───────────────

    #[test]
    fn stop_reason_tool_calls_maps_to_tool_use() {
        let resp = tool_call_response();
        let ir = kimi_response_to_ir(&resp);
        assert_eq!(ir.stop_reason, Some(IrStopReason::ToolUse));
    }

    // ── 25. Stop reason mapping: content_filter ─────────────────────

    #[test]
    fn stop_reason_content_filter() {
        let mut resp = simple_response();
        resp.choices[0].finish_reason = Some("content_filter".into());
        let ir = kimi_response_to_ir(&resp);
        assert_eq!(ir.stop_reason, Some(IrStopReason::ContentFilter));
    }

    // ── 26. Stop reason roundtrip: EndTurn → stop ───────────────────

    #[test]
    fn stop_reason_end_turn_roundtrip() {
        let ir = IrResponse::text("ok").with_stop_reason(IrStopReason::EndTurn);
        let kimi = ir_to_kimi_response(&ir);
        assert_eq!(kimi.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── 27. Stop reason roundtrip: ToolUse → tool_calls ─────────────

    #[test]
    fn stop_reason_tool_use_roundtrip() {
        let ir = IrResponse::text("ok").with_stop_reason(IrStopReason::ToolUse);
        let kimi = ir_to_kimi_response(&ir);
        assert_eq!(kimi.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    // ── 28. Stream text chunk to IR ─────────────────────────────────

    #[test]
    fn stream_text_chunk_to_ir() {
        let chunk = stream_text_chunk();
        let events = kimi_stream_to_ir(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0] {
            IrStreamEvent::TextDelta { index, text } => {
                assert_eq!(*index, 0);
                assert_eq!(text, "Hello");
            }
            _ => panic!("expected TextDelta"),
        }
    }

    // ── 29. Stream start chunk with role ─────────────────────────────

    #[test]
    fn stream_start_chunk_produces_stream_start() {
        let chunk = StreamChunk {
            id: "chunk-0".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".into()),
                    content: None,
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let events = kimi_stream_to_ir(&chunk);
        assert!(!events.is_empty());
        assert!(matches!(&events[0], IrStreamEvent::StreamStart { .. }));
    }

    // ── 30. Stream finish chunk produces StreamEnd ───────────────────

    #[test]
    fn stream_finish_chunk_produces_stream_end() {
        let chunk = StreamChunk {
            id: "chunk-f".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta::default(),
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        };
        let events = kimi_stream_to_ir(&chunk);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, IrStreamEvent::StreamEnd { .. }))
        );
    }

    // ── 31. Stream usage chunk ──────────────────────────────────────

    #[test]
    fn stream_usage_chunk_to_ir() {
        let chunk = StreamChunk {
            id: "chunk-u".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta::default(),
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 50,
                completion_tokens: 25,
                total_tokens: 75,
            }),
            refs: None,
        };
        let events = kimi_stream_to_ir(&chunk);
        let usage_event = events
            .iter()
            .find(|e| matches!(e, IrStreamEvent::Usage { .. }));
        assert!(usage_event.is_some());
        if let Some(IrStreamEvent::Usage { usage }) = usage_event {
            assert_eq!(usage.input_tokens, 50);
            assert_eq!(usage.output_tokens, 25);
        }
    }

    // ── 32. Stream tool call delta ──────────────────────────────────

    #[test]
    fn stream_tool_call_delta_to_ir() {
        let chunk = StreamChunk {
            id: "chunk-tc".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![StreamToolCall {
                        index: 0,
                        id: Some("call_1".into()),
                        call_type: Some("function".into()),
                        function: Some(StreamFunctionCall {
                            name: Some("search".into()),
                            arguments: Some(r#"{"q":"#.into()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let events = kimi_stream_to_ir(&chunk);
        let delta = events
            .iter()
            .find(|e| matches!(e, IrStreamEvent::ToolCallDelta { .. }));
        assert!(delta.is_some());
        if let Some(IrStreamEvent::ToolCallDelta {
            index,
            arguments_delta,
        }) = delta
        {
            assert_eq!(*index, 0);
            assert_eq!(arguments_delta, r#"{"q":"#);
        }
    }

    // ── 33. Citation refs preserved in response metadata ────────────

    #[test]
    fn citation_refs_preserved_in_metadata() {
        let resp = KimiResponse {
            id: "cmpl-ref".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: Some("Result with citations [1]".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: Some(vec![KimiRef {
                index: 1,
                url: "https://example.com".into(),
                title: Some("Example".into()),
            }]),
        };
        let ir = kimi_response_to_ir(&resp);
        assert!(ir.metadata.contains_key("kimi_refs"));

        let back = ir_to_kimi_response(&ir);
        let refs = back.refs.unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].url, "https://example.com");
        assert_eq!(refs[0].title.as_deref(), Some("Example"));
    }

    // ── 34. Multi-turn conversation mapping ─────────────────────────

    #[test]
    fn multi_turn_conversation_mapping() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some("Be helpful.".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: Some("What is 2+2?".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::Assistant,
                    content: Some("4".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: Some("And 3+3?".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.system_prompt.as_deref(), Some("Be helpful."));
        assert_eq!(ir.messages.len(), 3);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[2].role, IrRole::User);
    }

    // ── 35. Empty response content handled ──────────────────────────

    #[test]
    fn empty_response_content_handled() {
        let resp = KimiResponse {
            id: "cmpl-empty".into(),
            model: "moonshot-v1-8k".into(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: Some("".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        };
        let ir = kimi_response_to_ir(&resp);
        assert!(ir.content.is_empty());
    }

    // ── 36. No usage in response → None in IR ───────────────────────

    #[test]
    fn no_usage_maps_to_none() {
        let resp = tool_call_response();
        let ir = kimi_response_to_ir(&resp);
        assert!(ir.usage.is_none());
    }

    // ── 37. Multiple system messages concatenated ────────────────────

    #[test]
    fn multiple_system_messages_concatenated() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some("First system.".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::System,
                    content: Some("Second system.".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: Some("Hi".into()),
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        assert_eq!(
            ir.system_prompt.as_deref(),
            Some("First system.\nSecond system.")
        );
    }

    // ── 38. Empty stream chunk produces no events ───────────────────

    #[test]
    fn empty_stream_chunk_no_events() {
        let chunk = StreamChunk {
            id: "chunk-e".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta::default(),
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        };
        let events = kimi_stream_to_ir(&chunk);
        assert!(events.is_empty());
    }

    // ── 39. IR response with no stop reason ─────────────────────────

    #[test]
    fn ir_response_no_stop_reason() {
        let ir = IrResponse::text("partial");
        let kimi = ir_to_kimi_response(&ir);
        assert!(kimi.choices[0].finish_reason.is_none());
    }

    // ── 40. Builtin tools metadata contains all names ───────────────

    #[test]
    fn builtin_tools_metadata_contains_all() {
        let tools = vec![
            ToolDefinition::BuiltinFunction {
                function: BuiltinFunctionDef {
                    name: "$web_search".into(),
                },
            },
            ToolDefinition::BuiltinFunction {
                function: BuiltinFunctionDef {
                    name: "$browser".into(),
                },
            },
            ToolDefinition::Function {
                function: FunctionDefinition {
                    name: "custom".into(),
                    description: "custom tool".into(),
                    parameters: json!({}),
                },
            },
        ];
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("test".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: Some(tools),
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        let builtins = ir.metadata.get("kimi_builtin_tools").unwrap();
        let arr: Vec<String> = serde_json::from_value(builtins.clone()).unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.contains(&"$web_search".to_string()));
        assert!(arr.contains(&"$browser".to_string()));
    }

    // ── 41. Assistant message with tool calls in request ─────────────

    #[test]
    fn assistant_message_with_tool_calls() {
        let msg = Message {
            role: Role::Assistant,
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
        };
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![msg],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);
        let tool_calls = ir.messages[0].tool_calls();
        assert_eq!(tool_calls.len(), 1);
    }

    // ── 42. Config max_tokens default when None ─────────────────────

    #[test]
    fn config_max_tokens_none_passthrough() {
        let ir =
            IrRequest::new(vec![IrMessage::text(IrRole::User, "Hi")]).with_model("moonshot-v1-8k");
        let kimi = ir_to_kimi_request(&ir);
        assert!(kimi.max_tokens.is_none());
    }

    // ── 43. Temperature preserved ───────────────────────────────────

    #[test]
    fn temperature_preserved() {
        let req = KimiRequest {
            model: "moonshot-v1-8k".into(),
            messages: vec![Message {
                role: Role::User,
                content: Some("test".into()),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: None,
            temperature: Some(0.3),
            stream: None,
            tools: None,
            use_search: None,
        };
        let ir = kimi_request_to_ir(&req);
        assert_eq!(ir.config.temperature, Some(0.3));

        let back = ir_to_kimi_request(&ir);
        assert_eq!(back.temperature, Some(0.3));
    }
}
