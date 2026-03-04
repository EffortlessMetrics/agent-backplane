// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive tests for the mapper validation pipeline verifying
//! cross-dialect mapping correctness.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_dialect::Dialect;
use abp_mapper::validation::{
    DefaultMappingValidator, MappingValidator, ValidationPipeline, ValidationSeverity,
};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, DialectRequest,
    GeminiKimiIrMapper, IdentityMapper, IrIdentityMapper, IrMapper, MapError, Mapper, MappingError,
    OpenAiClaudeIrMapper, OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper,
    OpenAiKimiIrMapper, OpenAiToClaudeMapper,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn v() -> DefaultMappingValidator {
    DefaultMappingValidator::new()
}

fn system_msg(text: &str) -> IrMessage {
    IrMessage::text(IrRole::System, text)
}

fn user_msg(text: &str) -> IrMessage {
    IrMessage::text(IrRole::User, text)
}

fn assistant_msg(text: &str) -> IrMessage {
    IrMessage::text(IrRole::Assistant, text)
}

fn tool_use_block(id: &str, name: &str, input: serde_json::Value) -> IrContentBlock {
    IrContentBlock::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }
}

fn tool_result_block(tool_use_id: &str, text: &str, is_error: bool) -> IrContentBlock {
    IrContentBlock::ToolResult {
        tool_use_id: tool_use_id.into(),
        content: vec![IrContentBlock::Text { text: text.into() }],
        is_error,
    }
}

fn thinking_block(text: &str) -> IrContentBlock {
    IrContentBlock::Thinking { text: text.into() }
}

fn image_block() -> IrContentBlock {
    IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    }
}

fn conv(messages: Vec<IrMessage>) -> IrConversation {
    IrConversation::from_messages(messages)
}

fn simple_conversation() -> IrConversation {
    conv(vec![
        system_msg("You are helpful."),
        user_msg("Hello"),
        assistant_msg("Hi there!"),
    ])
}

fn tool_conversation() -> IrConversation {
    conv(vec![
        user_msg("What's the weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block(
                "tu_1",
                "get_weather",
                json!({"location": "NYC"}),
            )],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![tool_result_block("tu_1", "72°F, sunny", false)],
        ),
        assistant_msg("It's 72°F and sunny in NYC."),
    ])
}

fn make_tool_def(name: &str, desc: &str) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            },
            "required": ["location"]
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Mapper Registry (factory tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn registry_identity_for_all_dialects() {
    for &d in Dialect::all() {
        let mapper = abp_mapper::default_ir_mapper(d, d);
        assert!(mapper.is_some(), "identity mapper missing for {d}");
    }
}

#[test]
fn registry_openai_claude_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
}

#[test]
fn registry_openai_gemini_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).is_some());
}

#[test]
fn registry_claude_gemini_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Gemini, Dialect::Claude).is_some());
}

#[test]
fn registry_openai_codex_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::OpenAi, Dialect::Codex).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Codex, Dialect::OpenAi).is_some());
}

#[test]
fn registry_openai_kimi_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Kimi, Dialect::OpenAi).is_some());
}

#[test]
fn registry_openai_copilot_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Copilot, Dialect::OpenAi).is_some());
}

#[test]
fn registry_claude_kimi_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::Claude, Dialect::Kimi).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Kimi, Dialect::Claude).is_some());
}

#[test]
fn registry_gemini_kimi_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::Gemini, Dialect::Kimi).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Kimi, Dialect::Gemini).is_some());
}

#[test]
fn registry_codex_claude_pair() {
    assert!(abp_mapper::default_ir_mapper(Dialect::Codex, Dialect::Claude).is_some());
    assert!(abp_mapper::default_ir_mapper(Dialect::Claude, Dialect::Codex).is_some());
}

#[test]
fn registry_unsupported_pair_returns_none() {
    // Codex ↔ Gemini has no direct mapper
    assert!(abp_mapper::default_ir_mapper(Dialect::Codex, Dialect::Gemini).is_none());
    assert!(abp_mapper::default_ir_mapper(Dialect::Codex, Dialect::Kimi).is_none());
}

#[test]
fn registry_supported_pairs_includes_identity() {
    let pairs = abp_mapper::supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "missing identity pair for {d}");
    }
}

#[test]
fn registry_supported_pairs_includes_cross_dialect() {
    let pairs = abp_mapper::supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Identity Mapper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn identity_mapper_passthrough_request() {
    let mapper = IdentityMapper;
    let body = json!({"model": "gpt-4", "messages": []});
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: body.clone(),
    };
    assert_eq!(mapper.map_request(&req).unwrap(), body);
}

#[test]
fn identity_mapper_passthrough_response() {
    let mapper = IdentityMapper;
    let body = json!({"choices": [{"message": {"content": "hi"}}]});
    let resp = mapper.map_response(&body).unwrap();
    assert_eq!(resp.body, body);
}

#[test]
fn ir_identity_mapper_passthrough_conversation() {
    let mapper = IrIdentityMapper;
    let ir = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.messages.len(), ir.messages.len());
    assert_eq!(result, ir);
}

#[test]
fn ir_identity_mapper_supported_pairs_all_self() {
    let mapper = IrIdentityMapper;
    let pairs = mapper.supported_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)));
    }
}

#[test]
fn ir_identity_preserves_tool_calls() {
    let mapper = IrIdentityMapper;
    let ir = tool_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.tool_calls().len(), 1);
}

#[test]
fn ir_identity_preserves_metadata() {
    let mapper = IrIdentityMapper;
    let mut msg = user_msg("hello");
    msg.metadata
        .insert("custom_key".into(), json!("custom_value"));
    let ir = conv(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("custom_key"),
        Some(&json!("custom_value"))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Lossy Detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lossy_openai_to_codex_drops_system() {
    let mapper = OpenAiCodexIrMapper;
    let ir = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    assert!(result.system_message().is_none());
}

#[test]
fn lossy_openai_to_codex_drops_tool_blocks() {
    let mapper = OpenAiCodexIrMapper;
    let ir = tool_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    assert!(result.tool_calls().is_empty());
}

#[test]
fn lossy_openai_to_codex_drops_images() {
    let mapper = OpenAiCodexIrMapper;
    let ir = conv(vec![IrMessage::new(IrRole::User, vec![image_block()])]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn lossy_openai_to_codex_preserves_text() {
    let mapper = OpenAiCodexIrMapper;
    let ir = conv(vec![user_msg("hello"), assistant_msg("world")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result.messages[0].text_content(), "hello");
    assert_eq!(result.messages[1].text_content(), "world");
}

#[test]
fn lossy_claude_to_codex_drops_thinking() {
    let mapper = CodexClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            thinking_block("Let me think..."),
            IrContentBlock::Text {
                text: "answer".into(),
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Codex, &ir)
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 1);
    assert!(result.messages[0].is_text_only());
}

#[test]
fn lossy_claude_to_openai_drops_thinking() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            thinking_block("reasoning..."),
            IrContentBlock::Text {
                text: "result".into(),
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 1);
    assert!(matches!(
        &result.messages[0].content[0],
        IrContentBlock::Text { text } if text == "result"
    ));
}

#[test]
fn lossy_claude_to_gemini_drops_thinking() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            thinking_block("hmm"),
            IrContentBlock::Text {
                text: "done".into(),
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 1);
}

#[test]
fn lossy_detection_codex_to_openai_is_lossless() {
    let mapper = OpenAiCodexIrMapper;
    let ir = conv(vec![user_msg("hello"), assistant_msg("world")]);
    let result = mapper
        .map_request(Dialect::Codex, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result, ir);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Content Block Mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_text_block_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![user_msg("hello world")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.messages[0].text_content(), "hello world");
}

#[test]
fn content_text_block_claude_to_openai() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![user_msg("bonjour")]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.messages[0].text_content(), "bonjour");
}

#[test]
fn content_tool_use_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![tool_use_block("tu_1", "bash", json!({"cmd": "ls"}))],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let calls = result.tool_calls();
    assert_eq!(calls.len(), 1);
    if let IrContentBlock::ToolUse { id, name, input } = calls[0] {
        assert_eq!(id, "tu_1");
        assert_eq!(name, "bash");
        assert_eq!(input, &json!({"cmd": "ls"}));
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn content_tool_result_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Tool,
        vec![tool_result_block("tu_1", "output data", false)],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // Tool-role becomes User-role in Claude
    assert_eq!(result.messages[0].role, IrRole::User);
}

#[test]
fn content_image_block_openai_to_claude_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(IrRole::User, vec![image_block()])]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert!(matches!(
        &result.messages[0].content[0],
        IrContentBlock::Image { .. }
    ));
}

#[test]
fn content_image_block_claude_to_gemini_preserved() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = conv(vec![IrMessage::new(IrRole::User, vec![image_block()])]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    assert!(matches!(
        &result.messages[0].content[0],
        IrContentBlock::Image { .. }
    ));
}

#[test]
fn content_thinking_block_preserved_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            thinking_block("step 1"),
            IrContentBlock::Text {
                text: "result".into(),
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // Thinking is preserved when going to Claude
    assert_eq!(result.messages[0].content.len(), 2);
    assert!(matches!(
        &result.messages[0].content[0],
        IrContentBlock::Thinking { .. }
    ));
}

#[test]
fn content_mixed_blocks_assistant() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check.".into(),
            },
            tool_use_block("tu_2", "read_file", json!({"path": "main.rs"})),
        ],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 2);
}

#[test]
fn content_error_tool_result_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Tool,
        vec![tool_result_block("tu_1", "Error: not found", true)],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    if let IrContentBlock::ToolResult { is_error, .. } = &result.messages[0].content[0] {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Tool Definition Mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_json_schema_preserved() {
    let tool = make_tool_def("get_weather", "Get the weather");
    assert_eq!(tool.name, "get_weather");
    assert_eq!(tool.description, "Get the weather");
    assert!(tool.parameters["properties"]["location"].is_object());
}

#[test]
fn tool_def_serde_roundtrip() {
    let tool = make_tool_def("search", "Search the web");
    let json = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn tool_def_complex_parameters() {
    let tool = IrToolDefinition {
        name: "create_file".into(),
        description: "Create a new file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"},
                "permissions": {
                    "type": "object",
                    "properties": {
                        "read": {"type": "boolean"},
                        "write": {"type": "boolean"}
                    }
                }
            },
            "required": ["path", "content"]
        }),
    };
    let json_val = serde_json::to_value(&tool).unwrap();
    assert!(json_val["parameters"]["properties"]["permissions"]["properties"]["read"].is_object());
}

#[test]
fn tool_def_openai_to_claude_json_mapping() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        },
                        "required": ["path"]
                    }
                }
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools[0]["name"], "read_file");
    assert_eq!(tools[0]["description"], "Read a file");
    // Claude uses input_schema instead of parameters
    assert!(tools[0]["input_schema"]["properties"]["path"].is_object());
}

#[test]
fn tool_def_multiple_tools_mapped() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [
                {"type": "function", "function": {"name": "tool_a", "description": "A", "parameters": {"type": "object"}}},
                {"type": "function", "function": {"name": "tool_b", "description": "B", "parameters": {"type": "object"}}}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "tool_a");
    assert_eq!(tools[1]["name"], "tool_b");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. System Message Handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_msg_openai_to_claude_extracted() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "hi"}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["system"], "Be helpful");
    let msgs = result["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
}

#[test]
fn system_msg_ir_openai_to_claude_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![system_msg("You are an assistant."), user_msg("hi")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert!(result.system_message().is_some());
    assert_eq!(
        result.system_message().unwrap().text_content(),
        "You are an assistant."
    );
}

#[test]
fn system_msg_ir_claude_to_openai_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![system_msg("Be concise"), user_msg("hi")]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    assert!(result.system_message().is_some());
}

#[test]
fn system_msg_openai_to_codex_dropped() {
    let mapper = OpenAiCodexIrMapper;
    let ir = conv(vec![system_msg("You are helpful"), user_msg("hi")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    assert!(result.system_message().is_none());
}

#[test]
fn system_msg_claude_to_gemini_preserved() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = conv(vec![system_msg("Follow rules"), user_msg("hi")]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    assert!(result.system_message().is_some());
}

#[test]
fn system_msg_with_image_claude_to_gemini_fails() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = conv(vec![
        IrMessage::new(IrRole::System, vec![image_block()]),
        user_msg("hi"),
    ]);
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableContent { .. }));
}

#[test]
fn system_msg_gemini_to_claude_preserved() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = conv(vec![system_msg("instructions"), user_msg("hi")]);
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &ir)
        .unwrap();
    assert!(result.system_message().is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Streaming Chunk Mapping (partial messages)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_delta_openai_to_claude_event() {
    let mapper = OpenAiToClaudeMapper;
    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: abp_core::AgentEventKind::AssistantDelta {
            text: "partial ".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "content_block_delta");
    assert_eq!(result["delta"]["text"], "partial ");
}

#[test]
fn streaming_tool_call_event() {
    let mapper = OpenAiToClaudeMapper;
    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: abp_core::AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: Some("tu_5".into()),
            parent_tool_use_id: None,
            input: json!({"query": "rust"}),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["content_block"]["name"], "search");
}

#[test]
fn streaming_identity_preserves_events() {
    let mapper = IdentityMapper;
    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: abp_core::AgentEventKind::AssistantDelta {
            text: "chunk".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["text"], "chunk");
}

#[test]
fn streaming_tool_result_event() {
    let mapper = OpenAiToClaudeMapper;
    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: abp_core::AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_7".into()),
            output: json!("output"),
            is_error: false,
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "tu_7");
}

#[test]
fn streaming_full_message_event() {
    let mapper = OpenAiToClaudeMapper;
    let event = abp_core::AgentEvent {
        ts: chrono::Utc::now(),
        kind: abp_core::AgentEventKind::AssistantMessage {
            text: "Full response".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert_eq!(result["type"], "message");
    assert_eq!(result["content"][0]["text"], "Full response");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Error Paths
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_unsupported_pair_openai_claude_mapper() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = simple_conversation();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_unsupported_pair_openai_gemini_mapper() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = simple_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_unsupported_pair_claude_gemini_mapper() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = simple_conversation();
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_codex_to_claude_unmappable_tool() {
    let mapper = CodexClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![tool_use_block("tu_1", "apply_patch", json!({}))],
    )]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { .. }));
}

#[test]
fn error_codex_to_claude_apply_diff_rejected() {
    let mapper = CodexClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![tool_use_block("tu_1", "apply_diff", json!({}))],
    )]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { name, .. } if name == "apply_diff"));
}

#[test]
fn error_wrong_dialect_json_mapper() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({"model": "claude-3"}),
    };
    assert!(matches!(
        mapper.map_request(&req).unwrap_err(),
        MappingError::UnmappableRequest { .. }
    ));
}

#[test]
fn error_non_object_body() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!([1, 2, 3]),
    };
    assert!(matches!(
        mapper.map_request(&req).unwrap_err(),
        MappingError::UnmappableRequest { .. }
    ));
}

#[test]
fn error_map_error_serde_roundtrip() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Codex,
        to: Dialect::Copilot,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn error_unmappable_content_display() {
    let err = MapError::UnmappableContent {
        field: "system".into(),
        reason: "images not allowed".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("system"));
    assert!(msg.contains("images not allowed"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Round-Trip Fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_claude_openai_text_only() {
    let oa = OpenAiClaudeIrMapper;
    let ir = conv(vec![user_msg("hello"), assistant_msg("world")]);
    let to_claude = oa
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let back = oa
        .map_request(Dialect::Claude, Dialect::OpenAi, &to_claude)
        .unwrap();
    assert_eq!(back.messages.len(), ir.messages.len());
    assert_eq!(back.messages[0].text_content(), "hello");
    assert_eq!(back.messages[1].text_content(), "world");
}

#[test]
fn roundtrip_openai_gemini_openai_text_only() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = conv(vec![user_msg("test"), assistant_msg("response")]);
    let to_gem = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &to_gem)
        .unwrap();
    assert_eq!(back.messages.len(), ir.messages.len());
    assert_eq!(back.messages[0].text_content(), "test");
}

#[test]
fn roundtrip_claude_gemini_claude_text_only() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = conv(vec![user_msg("q"), assistant_msg("a")]);
    let to_gem = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &to_gem)
        .unwrap();
    assert_eq!(back.messages.len(), ir.messages.len());
}

#[test]
fn roundtrip_openai_kimi_openai_text_only() {
    let mapper = OpenAiKimiIrMapper;
    let ir = conv(vec![user_msg("ping"), assistant_msg("pong")]);
    let to_kimi = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Kimi, Dialect::OpenAi, &to_kimi)
        .unwrap();
    assert_eq!(back, to_kimi); // near-identity
}

#[test]
fn roundtrip_openai_copilot_openai_text_only() {
    let mapper = OpenAiCopilotIrMapper;
    let ir = conv(vec![user_msg("x"), assistant_msg("y")]);
    let to_cop = mapper
        .map_request(Dialect::OpenAi, Dialect::Copilot, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Copilot, Dialect::OpenAi, &to_cop)
        .unwrap();
    assert_eq!(back, to_cop);
}

#[test]
fn roundtrip_claude_kimi_claude_text_only() {
    let mapper = ClaudeKimiIrMapper;
    let ir = conv(vec![user_msg("a"), assistant_msg("b")]);
    let to_kimi = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Kimi, Dialect::Claude, &to_kimi)
        .unwrap();
    assert_eq!(back.messages.len(), ir.messages.len());
}

#[test]
fn roundtrip_gemini_kimi_gemini_text_only() {
    let mapper = GeminiKimiIrMapper;
    let ir = conv(vec![user_msg("q"), assistant_msg("a")]);
    let to_kimi = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Kimi, Dialect::Gemini, &to_kimi)
        .unwrap();
    assert_eq!(back.messages.len(), ir.messages.len());
}

#[test]
fn roundtrip_validation_identical_json() {
    let val = v();
    let orig = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = val.validate_roundtrip(&orig, &orig);
    assert!(r.is_lossless());
}

#[test]
fn roundtrip_validation_detects_lossy_codex() {
    let val = v();
    let orig = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7,
        "tools": []
    });
    let roundtripped = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let r = val.validate_roundtrip(&orig, &roundtripped);
    assert!(!r.is_lossless());
    assert!(!r.lost_fields.is_empty());
}

#[test]
fn roundtrip_openai_claude_tool_conversation() {
    let mapper = OpenAiClaudeIrMapper;
    // Simple tool call that should survive a roundtrip role-mapping
    let ir = conv(vec![
        user_msg("What's 2+2?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("tu_1", "calc", json!({"expr": "2+2"}))],
        ),
    ]);
    let to_claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &to_claude)
        .unwrap();
    assert_eq!(back.tool_calls().len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Metadata Preservation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn metadata_preserved_through_identity() {
    let mapper = IrIdentityMapper;
    let mut msg = user_msg("hi");
    msg.metadata.insert("vendor_id".into(), json!("abc-123"));
    msg.metadata.insert("trace_id".into(), json!(42));
    let ir = conv(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("vendor_id"),
        Some(&json!("abc-123"))
    );
    assert_eq!(
        result.messages[0].metadata.get("trace_id"),
        Some(&json!(42))
    );
}

#[test]
fn metadata_preserved_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let mut msg = user_msg("hi");
    msg.metadata.insert("request_id".into(), json!("req-1"));
    let ir = conv(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("request_id"),
        Some(&json!("req-1"))
    );
}

#[test]
fn metadata_preserved_claude_to_openai() {
    let mapper = OpenAiClaudeIrMapper;
    let mut msg = assistant_msg("response");
    msg.metadata
        .insert("usage_info".into(), json!({"tokens": 50}));
    let ir = conv(vec![msg]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("usage_info"),
        Some(&json!({"tokens": 50}))
    );
}

#[test]
fn metadata_preserved_openai_to_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let mut msg = user_msg("hello");
    msg.metadata.insert("session".into(), json!("s-1"));
    let ir = conv(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("session"),
        Some(&json!("s-1"))
    );
}

#[test]
fn metadata_btreemap_deterministic_order() {
    let mut meta = BTreeMap::new();
    meta.insert("z_key".into(), json!("z"));
    meta.insert("a_key".into(), json!("a"));
    meta.insert("m_key".into(), json!("m"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "test".into(),
        }],
        metadata: meta,
    };
    let json_str = serde_json::to_string(&msg).unwrap();
    // BTreeMap ensures alphabetical key order
    let a_pos = json_str.find("a_key").unwrap();
    let m_pos = json_str.find("m_key").unwrap();
    let z_pos = json_str.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn metadata_empty_serialization() {
    let msg = user_msg("no metadata");
    let json_val = serde_json::to_value(&msg).unwrap();
    // Empty metadata should be omitted (skip_serializing_if)
    assert!(json_val.get("metadata").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-Dialect Pair Tests: OpenAI ↔ Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_claude_message_conversion_fidelity() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![
        system_msg("Be brief"),
        user_msg("Summarize this"),
        assistant_msg("Summary here"),
    ]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[1].role, IrRole::User);
    assert_eq!(result.messages[2].role, IrRole::Assistant);
}

#[test]
fn openai_claude_tool_call_fidelity() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            tool_use_block("tu_a", "search", json!({"q": "rust lang"})),
            tool_use_block("tu_b", "read", json!({"path": "/tmp"})),
        ],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.tool_calls().len(), 2);
}

#[test]
fn claude_openai_tool_result_role_change() {
    let mapper = OpenAiClaudeIrMapper;
    // Claude puts tool results in user-role messages
    let ir = conv(vec![IrMessage::new(
        IrRole::User,
        vec![tool_result_block("tu_1", "42", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    // Should become Tool-role in OpenAI convention
    assert_eq!(result.messages[0].role, IrRole::Tool);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-Dialect Pair Tests: Claude ↔ Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_gemini_message_fidelity() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = conv(vec![user_msg("test"), assistant_msg("ok")]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result.messages[0].text_content(), "test");
}

#[test]
fn claude_gemini_tool_role_mapping() {
    let mapper = ClaudeGeminiIrMapper;
    // Claude tool-role → Gemini user-role
    let ir = conv(vec![IrMessage::new(
        IrRole::Tool,
        vec![tool_result_block("tu_1", "data", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::User);
}

#[test]
fn gemini_claude_tool_role_mapping() {
    let mapper = ClaudeGeminiIrMapper;
    // Gemini tool-role → Claude user-role
    let ir = conv(vec![IrMessage::new(
        IrRole::Tool,
        vec![tool_result_block("tu_1", "result", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-Dialect Pair Tests: OpenAI ↔ Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_gemini_system_preserved() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = conv(vec![system_msg("rules"), user_msg("go")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    assert!(result.system_message().is_some());
}

#[test]
fn openai_gemini_tool_role_to_user() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Tool,
        vec![tool_result_block("tu_1", "res", false)],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::User);
}

#[test]
fn gemini_openai_user_tool_results_to_tool_role() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::User,
        vec![tool_result_block("tu_1", "output", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::Tool);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-Dialect Pair Tests: Claude ↔ Kimi
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_kimi_text_fidelity() {
    let mapper = ClaudeKimiIrMapper;
    let ir = conv(vec![user_msg("test"), assistant_msg("ok")]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap();
    assert_eq!(result.messages[0].text_content(), "test");
    assert_eq!(result.messages[1].text_content(), "ok");
}

#[test]
fn claude_kimi_tool_result_user_to_tool() {
    let mapper = ClaudeKimiIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::User,
        vec![tool_result_block("tu_1", "result", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::Tool);
}

#[test]
fn kimi_claude_tool_to_user() {
    let mapper = ClaudeKimiIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Tool,
        vec![tool_result_block("tu_1", "data", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-Dialect Pair Tests: Gemini ↔ Kimi
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_kimi_user_tool_results_to_tool_role() {
    let mapper = GeminiKimiIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::User,
        vec![tool_result_block("tu_1", "val", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::Tool);
}

#[test]
fn kimi_gemini_tool_to_user() {
    let mapper = GeminiKimiIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Tool,
        vec![tool_result_block("tu_1", "data", false)],
    )]);
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::Gemini, &ir)
        .unwrap();
    assert_eq!(result.messages[0].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// Validation Pipeline Integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_openai_to_claude_full_pass() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::Claude);
    let req = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = pipe.run(&req, |_| {
        Ok(json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1024
        }))
    });
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    assert!(result.post.as_ref().unwrap().is_valid());
}

#[test]
fn pipeline_gemini_to_openai_full_pass() {
    let pipe = ValidationPipeline::new(v(), Dialect::Gemini, Dialect::OpenAi);
    let req = json!({"model": "gemini-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let result = pipe.run(&req, |_| {
        Ok(json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        }))
    });
    assert!(result.pre.is_valid());
    assert!(result.post.as_ref().unwrap().is_valid());
}

#[test]
fn pipeline_pre_validation_blocks_missing_fields() {
    let pipe = ValidationPipeline::new(v(), Dialect::Claude, Dialect::OpenAi);
    let req = json!({"model": "claude-3"}); // missing messages and max_tokens
    let result = pipe.run(&req, |_| panic!("should not be called"));
    assert!(!result.pre.is_valid());
    assert!(result.mapped.is_none());
}

#[test]
fn pipeline_post_validation_catches_missing_target_fields() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::Gemini);
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    // Map produces something missing Gemini's "contents"
    let result = pipe.run(&req, |_| Ok(json!({"model": "gemini-pro"})));
    assert!(result.pre.is_valid());
    let post = result.post.unwrap();
    assert!(!post.is_valid());
    assert!(post.issues.iter().any(|i| i.field == "contents"));
}

#[test]
fn pipeline_mapping_error_produces_post_error() {
    let pipe = ValidationPipeline::new(v(), Dialect::OpenAi, Dialect::Claude);
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = pipe.run(&req, |_| Err("mapping explosion".into()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_none());
    let post = result.post.unwrap();
    assert!(!post.is_valid());
    assert!(post.issues[0].message.contains("mapping explosion"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_conversation_maps_without_error() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn conversation_with_only_system_msg() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![system_msg("rules only")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result.messages[0].role, IrRole::System);
}

#[test]
fn mixed_user_content_claude_to_kimi() {
    let mapper = ClaudeKimiIrMapper;
    // User message with text + tool result = split
    let ir = conv(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "context".into(),
            },
            tool_result_block("tu_1", "data", false),
        ],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap();
    // Should be split: user text + tool role
    assert!(result.len() >= 2);
}

#[test]
fn multiple_tool_results_in_user_msg_claude_to_openai() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::User,
        vec![
            tool_result_block("tu_1", "result_1", false),
            tool_result_block("tu_2", "result_2", false),
        ],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    // Each tool result should become separate Tool-role message
    assert_eq!(result.len(), 2);
    assert!(result.messages.iter().all(|m| m.role == IrRole::Tool));
}

#[test]
fn severity_ordering() {
    assert!(ValidationSeverity::Info < ValidationSeverity::Warning);
    assert!(ValidationSeverity::Warning < ValidationSeverity::Error);
}

#[test]
fn validation_issue_display_format() {
    let issue = abp_mapper::validation::ValidationIssue {
        severity: ValidationSeverity::Warning,
        field: "temperature".into(),
        message: "out of range".into(),
        code: "range_check".into(),
    };
    let display = format!("{issue}");
    assert!(display.contains("warning"));
    assert!(display.contains("temperature"));
    assert!(display.contains("out of range"));
    assert!(display.contains("range_check"));
}

#[test]
fn ir_conversation_accessors() {
    let ir = simple_conversation();
    assert_eq!(ir.len(), 3);
    assert!(!ir.is_empty());
    assert!(ir.system_message().is_some());
    assert!(ir.last_assistant().is_some());
    assert_eq!(ir.messages_by_role(IrRole::User).len(), 1);
}

#[test]
fn ir_message_is_text_only() {
    let msg = user_msg("hello");
    assert!(msg.is_text_only());

    let mixed = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Text { text: "hi".into() }, image_block()],
    );
    assert!(!mixed.is_text_only());
}

#[test]
fn all_supported_pairs_have_working_mappers() {
    let pairs = abp_mapper::supported_ir_pairs();
    let ir = conv(vec![user_msg("test")]);
    for (from, to) in &pairs {
        let mapper = abp_mapper::default_ir_mapper(*from, *to)
            .unwrap_or_else(|| panic!("no mapper for {from} → {to}"));
        let result = mapper.map_request(*from, *to, &ir);
        assert!(
            result.is_ok(),
            "mapper {from} → {to} failed: {:?}",
            result.err()
        );
    }
}

#[test]
fn codex_to_openai_lossless() {
    let mapper = OpenAiCodexIrMapper;
    let ir = conv(vec![user_msg("code me"), assistant_msg("fn main() {}")]);
    let result = mapper
        .map_request(Dialect::Codex, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result, ir);
}

#[test]
fn codex_to_claude_normal_tool_allowed() {
    let mapper = CodexClaudeIrMapper;
    let ir = conv(vec![IrMessage::new(
        IrRole::Assistant,
        vec![tool_use_block("tu_1", "bash", json!({"cmd": "ls"}))],
    )]);
    // Normal tools should be allowed
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &ir);
    assert!(result.is_ok());
}
