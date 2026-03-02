// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for SDK dialect lowering: dialect → IR → dialect roundtrips.
//!
//! Covers OpenAI, Claude, and Gemini lowering across messages, tool calls,
//! tool results, images, streaming, system prompts, usage, stop reasons,
//! model names, sampling params, and edge cases.

use serde_json::json;

// ─── OpenAI imports ─────────────────────────────────────────────────────
use abp_openai_sdk::dialect::{
    self as openai_dialect, CanonicalToolDef as OaiCanonical, OpenAIChoice, OpenAIConfig,
    OpenAIFunctionCall, OpenAIMessage, OpenAIResponse,
    OpenAIToolCall, OpenAIUsage,
};
use abp_openai_sdk::lowering as openai_lowering;
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall, ChunkUsage,
    ToolCallAccumulator,
};
use abp_openai_sdk::validation::{self, ExtendedRequestFields};

// ─── Claude imports ─────────────────────────────────────────────────────
use abp_claude_sdk::dialect::{
    self as claude_dialect, CanonicalToolDef as ClaudeCanonical, ClaudeConfig, ClaudeContentBlock,
    ClaudeImageSource, ClaudeMessage, ClaudeResponse, ClaudeStopReason, ClaudeStreamDelta,
    ClaudeStreamEvent, ClaudeUsage,
};
use abp_claude_sdk::lowering as claude_lowering;

// ─── Gemini imports ─────────────────────────────────────────────────────
use abp_gemini_sdk::dialect::{
    self as gemini_dialect, CanonicalToolDef as GeminiCanonical, GeminiCandidate, GeminiConfig,
    GeminiContent, GeminiInlineData, GeminiPart, GeminiResponse,
    GeminiStreamChunk, GeminiUsageMetadata,
};
use abp_gemini_sdk::lowering as gemini_lowering;

// ─── IR imports ─────────────────────────────────────────────────────────
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEventKind, WorkOrderBuilder};

// =========================================================================
// 1. OpenAI messages → IR → back preserves semantics
// =========================================================================

#[test]
fn openai_user_text_roundtrip() {
    let msgs = vec![oai_msg("user", Some("Hello world"), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("Hello world"));
}

#[test]
fn openai_assistant_text_roundtrip() {
    let msgs = vec![oai_msg("assistant", Some("Sure thing"), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content.as_deref(), Some("Sure thing"));
}

#[test]
fn openai_system_text_roundtrip() {
    let msgs = vec![oai_msg("system", Some("You are helpful"), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("You are helpful"));
}

#[test]
fn openai_preserves_unicode_text() {
    let text = "こんにちは 🌍 مرحبا";
    let msgs = vec![oai_msg("user", Some(text), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some(text));
}

#[test]
fn openai_preserves_newlines_and_whitespace() {
    let text = "line1\n  line2\n\ttabbed";
    let msgs = vec![oai_msg("user", Some(text), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].text_content(), text);
}

// =========================================================================
// 2. Claude content blocks → IR → back
// =========================================================================

#[test]
fn claude_user_text_roundtrip() {
    let msgs = vec![claude_msg("user", "Hello")];
    let conv = claude_lowering::to_ir(&msgs, None);
    let back = claude_lowering::from_ir(&conv);
    assert_eq!(back[0].content, "Hello");
}

#[test]
fn claude_assistant_text_roundtrip() {
    let msgs = vec![claude_msg("assistant", "Response here")];
    let conv = claude_lowering::to_ir(&msgs, None);
    let back = claude_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content, "Response here");
}

#[test]
fn claude_tool_use_block_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_abc".into(),
        name: "read_file".into(),
        input: json!({"path": "lib.rs"}),
    }];
    let msgs = vec![claude_msg(
        "assistant",
        &serde_json::to_string(&blocks).unwrap(),
    )];
    let conv = claude_lowering::to_ir(&msgs, None);
    let back = claude_lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tu_abc");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "lib.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn claude_tool_result_block_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("file data".into()),
        is_error: None,
    }];
    let msgs = vec![claude_msg("user", &serde_json::to_string(&blocks).unwrap())];
    let conv = claude_lowering::to_ir(&msgs, None);
    let back = claude_lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            assert_eq!(tool_use_id, "tu_1");
            assert_eq!(content.as_deref(), Some("file data"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn claude_thinking_block_roundtrip() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "Let me think...".into(),
        signature: Some("sig".into()),
    }];
    let msgs = vec![claude_msg(
        "assistant",
        &serde_json::to_string(&blocks).unwrap(),
    )];
    let conv = claude_lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "Let me think..."),
        other => panic!("expected Thinking, got {other:?}"),
    }
    let back = claude_lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::Thinking { thinking, .. } => assert_eq!(thinking, "Let me think..."),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn claude_mixed_text_and_tool_use() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "Let me check.".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "t1".into(),
            name: "ls".into(),
            input: json!({}),
        },
    ];
    let msgs = vec![claude_msg(
        "assistant",
        &serde_json::to_string(&blocks).unwrap(),
    )];
    let conv = claude_lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].content.len(), 2);
}

// =========================================================================
// 3. Gemini parts → IR → back
// =========================================================================

#[test]
fn gemini_user_text_roundtrip() {
    let contents = vec![gemini_content("user", vec![GeminiPart::Text("Hi".into())])];
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hi"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_model_text_roundtrip() {
    let contents = vec![gemini_content(
        "model",
        vec![GeminiPart::Text("Sure!".into())],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    let back = gemini_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "model");
}

#[test]
fn gemini_function_call_roundtrip() {
    let contents = vec![gemini_content(
        "model",
        vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"q": "rust"}),
        }],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "rust"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn gemini_function_response_roundtrip() {
    let contents = vec![gemini_content(
        "user",
        vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("results here"),
        }],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    let back = gemini_lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response, &json!("results here"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn gemini_multiple_parts_roundtrip() {
    let contents = vec![gemini_content(
        "model",
        vec![
            GeminiPart::Text("Searching...".into()),
            GeminiPart::FunctionCall {
                name: "grep".into(),
                args: json!({"pattern": "fn"}),
            },
        ],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = gemini_lowering::from_ir(&conv);
    assert_eq!(back[0].parts.len(), 2);
}

// =========================================================================
// 4. System message handling per SDK
// =========================================================================

#[test]
fn openai_system_as_separate_message() {
    let msgs = vec![
        oai_msg("system", Some("Be concise"), None, None),
        oai_msg("user", Some("Hi"), None, None),
    ];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be concise");
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "Be concise");
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn claude_system_as_separate_param() {
    let msgs = vec![claude_msg("user", "Hello")];
    let conv = claude_lowering::to_ir(&msgs, Some("Be helpful"));
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be helpful");
    // from_ir skips system messages for Claude
    let back = claude_lowering::from_ir(&conv);
    assert!(back.iter().all(|m| m.role != "system"));
    // extract system prompt separately
    let sys = claude_lowering::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("Be helpful"));
}

#[test]
fn gemini_system_as_system_instruction() {
    let sys_content = gemini_content("user", vec![GeminiPart::Text("Be helpful".into())]);
    let contents = vec![gemini_content(
        "user",
        vec![GeminiPart::Text("Hello".into())],
    )];
    let conv = gemini_lowering::to_ir(&contents, Some(&sys_content));
    assert_eq!(conv.messages[0].role, IrRole::System);
    // from_ir skips system messages
    let back = gemini_lowering::from_ir(&conv);
    assert!(back.iter().all(|c| c.role != "system"));
    // extract system instruction
    let sys = gemini_lowering::extract_system_instruction(&conv).unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be helpful"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn claude_empty_system_prompt_skipped() {
    let msgs = vec![claude_msg("user", "hi")];
    let conv = claude_lowering::to_ir(&msgs, Some(""));
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn gemini_empty_system_instruction_skipped() {
    let sys = gemini_content("user", vec![]);
    let contents = vec![gemini_content("user", vec![GeminiPart::Text("hi".into())])];
    let conv = gemini_lowering::to_ir(&contents, Some(&sys));
    assert_eq!(conv.len(), 1);
}

// =========================================================================
// 5. Tool definitions lowered correctly per SDK
// =========================================================================

#[test]
fn openai_tool_def_roundtrip() {
    let canonical = OaiCanonical {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let oai = openai_dialect::tool_def_to_openai(&canonical);
    assert_eq!(oai.tool_type, "function");
    assert_eq!(oai.function.name, "read_file");
    let back = openai_dialect::tool_def_from_openai(&oai);
    assert_eq!(back, canonical);
}

#[test]
fn claude_tool_def_roundtrip() {
    let canonical = ClaudeCanonical {
        name: "write_file".into(),
        description: "Write a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
    };
    let claude = claude_dialect::tool_def_to_claude(&canonical);
    assert_eq!(claude.name, "write_file");
    assert_eq!(claude.input_schema, canonical.parameters_schema);
    let back = claude_dialect::tool_def_from_claude(&claude);
    assert_eq!(back, canonical);
}

#[test]
fn gemini_tool_def_roundtrip() {
    let canonical = GeminiCanonical {
        name: "search".into(),
        description: "Search code".into(),
        parameters_schema: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
    };
    let gemini = gemini_dialect::tool_def_to_gemini(&canonical);
    assert_eq!(gemini.name, "search");
    assert_eq!(gemini.parameters, canonical.parameters_schema);
    let back = gemini_dialect::tool_def_from_gemini(&gemini);
    assert_eq!(back, canonical);
}

#[test]
fn openai_tool_def_preserves_complex_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "path": {"type": "string"},
            "encoding": {"type": "string", "enum": ["utf-8", "ascii"]},
            "line_range": {
                "type": "array",
                "items": {"type": "integer"},
                "minItems": 2,
                "maxItems": 2
            }
        },
        "required": ["path"]
    });
    let canonical = OaiCanonical {
        name: "read_lines".into(),
        description: "Read specific lines".into(),
        parameters_schema: schema.clone(),
    };
    let oai = openai_dialect::tool_def_to_openai(&canonical);
    let back = openai_dialect::tool_def_from_openai(&oai);
    assert_eq!(back.parameters_schema, schema);
}

// =========================================================================
// 6. Tool call/result roundtrip per SDK
// =========================================================================

#[test]
fn openai_tool_call_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_42".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_42");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "main.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
    let back = openai_lowering::from_ir(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_42");
    assert_eq!(tc.function.name, "read_file");
}

#[test]
fn openai_tool_result_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("file contents here".into()),
        tool_calls: None,
        tool_call_id: Some("call_42".into()),
    }];
    let conv = openai_lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "call_42");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_42"));
    assert_eq!(back[0].content.as_deref(), Some("file contents here"));
}

#[test]
fn openai_multiple_tool_calls_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "a".into(),
                    arguments: r#"{"x":1}"#.into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "b".into(),
                    arguments: r#"{"y":2}"#.into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
}

#[test]
fn claude_tool_call_with_complex_input() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_complex".into(),
        name: "edit".into(),
        input: json!({
            "path": "src/lib.rs",
            "old_str": "fn old() {}",
            "new_str": "fn new() {}"
        }),
    }];
    let msgs = vec![claude_msg(
        "assistant",
        &serde_json::to_string(&blocks).unwrap(),
    )];
    let conv = claude_lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input["path"], "src/lib.rs");
            assert_eq!(input["old_str"], "fn old() {}");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn claude_tool_result_error_flag() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("file not found".into()),
        is_error: Some(true),
    }];
    let msgs = vec![claude_msg("user", &serde_json::to_string(&blocks).unwrap())];
    let conv = claude_lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
    let back = claude_lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => assert_eq!(*is_error, Some(true)),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn gemini_function_call_synthesized_id() {
    let contents = vec![gemini_content(
        "model",
        vec![GeminiPart::FunctionCall {
            name: "do_thing".into(),
            args: json!({}),
        }],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "gemini_do_thing");
            assert_eq!(name, "do_thing");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn gemini_function_response_object_payload() {
    let contents = vec![gemini_content(
        "user",
        vec![GeminiPart::FunctionResponse {
            name: "api".into(),
            response: json!({"status": 200, "body": "ok"}),
        }],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
            assert_eq!(content.len(), 1);
            let text = match &content[0] {
                IrContentBlock::Text { text } => text.as_str(),
                _ => panic!("expected text"),
            };
            assert!(text.contains("200"));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// =========================================================================
// 7. Streaming deltas map to IR events
// =========================================================================

#[test]
fn openai_stream_text_delta() {
    let chunk = ChatCompletionChunk {
        id: "cmpl-1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".into()),
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn openai_stream_empty_delta_no_events() {
    let chunk = ChatCompletionChunk {
        id: "cmpl-2".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".into()),
                content: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn openai_tool_call_accumulator_basic() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("read".into()),
            arguments: Some(r#"{"pa"#.into()),
        }),
    }]);
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#"th":"a.rs"}"#.into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "read");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            assert_eq!(input, &json!({"path": "a.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn openai_tool_call_accumulator_multiple_calls() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        ChunkToolCall {
            index: 0,
            id: Some("c0".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("fn_a".into()),
                arguments: Some("{}".into()),
            }),
        },
        ChunkToolCall {
            index: 1,
            id: Some("c1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("fn_b".into()),
                arguments: Some("{}".into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn claude_stream_text_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "world".into(),
        },
    };
    let events = claude_dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "world"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn claude_stream_thinking_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "hmm...".into(),
        },
    };
    let events = claude_dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hmm..."),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
    assert!(events[0].ext.as_ref().unwrap().contains_key("thinking"));
}

#[test]
fn claude_stream_message_start() {
    let event = ClaudeStreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_1".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: None,
        },
    };
    let events = claude_dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn claude_stream_message_stop() {
    let event = ClaudeStreamEvent::MessageStop {};
    let events = claude_dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn claude_stream_tool_use_start() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::ToolUse {
            id: "tu_s1".into(),
            name: "bash".into(),
            input: json!({}),
        },
    };
    let events = claude_dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "bash"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn claude_stream_ping_no_events() {
    let event = ClaudeStreamEvent::Ping {};
    let events = claude_dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn gemini_stream_text_delta() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("delta text".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = gemini_dialect::map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "delta text"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn gemini_stream_function_call() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "test"}),
                }],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = gemini_dialect::map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "search"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// =========================================================================
// 8. Multi-turn conversations preserve order
// =========================================================================

#[test]
fn openai_multi_turn_order() {
    let msgs = vec![
        oai_msg("system", Some("Be concise."), None, None),
        oai_msg("user", Some("First"), None, None),
        oai_msg("assistant", Some("Reply 1"), None, None),
        oai_msg("user", Some("Second"), None, None),
        oai_msg("assistant", Some("Reply 2"), None, None),
    ];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 5);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back.len(), 5);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].content.as_deref(), Some("First"));
    assert_eq!(back[2].content.as_deref(), Some("Reply 1"));
    assert_eq!(back[3].content.as_deref(), Some("Second"));
    assert_eq!(back[4].content.as_deref(), Some("Reply 2"));
}

#[test]
fn claude_multi_turn_order() {
    let msgs = vec![
        claude_msg("user", "A"),
        claude_msg("assistant", "B"),
        claude_msg("user", "C"),
        claude_msg("assistant", "D"),
    ];
    let conv = claude_lowering::to_ir(&msgs, Some("sys"));
    assert_eq!(conv.len(), 5); // system + 4 turns
    let back = claude_lowering::from_ir(&conv);
    assert_eq!(back.len(), 4); // system skipped
    assert_eq!(back[0].content, "A");
    assert_eq!(back[1].content, "B");
    assert_eq!(back[2].content, "C");
    assert_eq!(back[3].content, "D");
}

#[test]
fn gemini_multi_turn_order() {
    let contents = vec![
        gemini_content("user", vec![GeminiPart::Text("Q1".into())]),
        gemini_content("model", vec![GeminiPart::Text("A1".into())]),
        gemini_content("user", vec![GeminiPart::Text("Q2".into())]),
        gemini_content("model", vec![GeminiPart::Text("A2".into())]),
    ];
    let conv = gemini_lowering::to_ir(&contents, None);
    assert_eq!(conv.len(), 4);
    let back = gemini_lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[1].role, "model");
}

#[test]
fn openai_tool_call_then_result_turn_order() {
    let msgs = vec![
        oai_msg("user", Some("Do it"), None, None),
        OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read".into(),
                    arguments: "{}".into(),
                },
            }]),
            tool_call_id: None,
        },
        oai_msg("tool", Some("data"), None, Some("c1")),
        oai_msg("assistant", Some("Done"), None, None),
    ];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
    assert_eq!(conv.messages[2].role, IrRole::Tool);
    assert_eq!(conv.messages[3].role, IrRole::Assistant);
}

// =========================================================================
// 9. Image content blocks (vision) handling
// =========================================================================

#[test]
fn claude_base64_image_roundtrip() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        },
    }];
    let msgs = vec![claude_msg("user", &serde_json::to_string(&blocks).unwrap())];
    let conv = claude_lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBORw0KGgo=");
        }
        other => panic!("expected Image, got {other:?}"),
    }
    let back = claude_lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 { media_type, data },
        } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBORw0KGgo=");
        }
        other => panic!("expected Image base64, got {other:?}"),
    }
}

#[test]
fn claude_url_image_to_ir_becomes_text() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    }];
    let msgs = vec![claude_msg("user", &serde_json::to_string(&blocks).unwrap())];
    let conv = claude_lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Text { text } => {
            assert!(text.contains("https://example.com/img.png"));
        }
        other => panic!("expected Text with URL, got {other:?}"),
    }
}

#[test]
fn gemini_inline_data_roundtrip() {
    let contents = vec![gemini_content(
        "user",
        vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/jpeg".into(),
            data: "base64jpeg".into(),
        })],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "base64jpeg");
        }
        other => panic!("expected Image, got {other:?}"),
    }
    let back = gemini_lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/jpeg");
            assert_eq!(d.data, "base64jpeg");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn claude_text_and_image_mixed() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "Look at this:".into(),
        },
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc123".into(),
            },
        },
    ];
    let msgs = vec![claude_msg("user", &serde_json::to_string(&blocks).unwrap())];
    let conv = claude_lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].content.len(), 2);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Text { .. }
    ));
    assert!(matches!(
        &conv.messages[0].content[1],
        IrContentBlock::Image { .. }
    ));
}

// =========================================================================
// 10. Token usage extraction per SDK
// =========================================================================

#[test]
fn openai_usage_extraction() {
    let usage = OpenAIUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let ir = IrUsage::from_io(usage.prompt_tokens, usage.completion_tokens);
    assert_eq!(ir.input_tokens, 100);
    assert_eq!(ir.output_tokens, 50);
    assert_eq!(ir.total_tokens, 150);
}

#[test]
fn claude_usage_extraction() {
    let usage = ClaudeUsage {
        input_tokens: 200,
        output_tokens: 80,
        cache_creation_input_tokens: Some(50),
        cache_read_input_tokens: Some(30),
    };
    let ir = IrUsage::with_cache(
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens.unwrap_or(0),
        usage.cache_creation_input_tokens.unwrap_or(0),
    );
    assert_eq!(ir.input_tokens, 200);
    assert_eq!(ir.output_tokens, 80);
    assert_eq!(ir.total_tokens, 280);
    assert_eq!(ir.cache_read_tokens, 30);
    assert_eq!(ir.cache_write_tokens, 50);
}

#[test]
fn gemini_usage_extraction() {
    let usage = GeminiUsageMetadata {
        prompt_token_count: 120,
        candidates_token_count: 60,
        total_token_count: 180,
    };
    let ir = IrUsage::from_io(usage.prompt_token_count, usage.candidates_token_count);
    assert_eq!(ir.input_tokens, 120);
    assert_eq!(ir.output_tokens, 60);
    assert_eq!(ir.total_tokens, 180);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::from_io(200, 75);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 125);
    assert_eq!(merged.total_tokens, 425);
}

#[test]
fn ir_usage_default_is_zero() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

// =========================================================================
// 11. Stop reason / finish_reason mapping
// =========================================================================

#[test]
fn openai_finish_reason_stop() {
    let resp = openai_response("gpt-4o", "Hello!", Some("stop"));
    let events = openai_dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "Hello!"
    ));
}

#[test]
fn openai_finish_reason_tool_calls() {
    let resp = OpenAIResponse {
        id: "cmpl-1".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "tc_1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "fn".into(),
                        arguments: "{}".into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = openai_dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn claude_stop_reason_parsing() {
    assert_eq!(
        claude_dialect::parse_stop_reason("end_turn"),
        Some(ClaudeStopReason::EndTurn)
    );
    assert_eq!(
        claude_dialect::parse_stop_reason("tool_use"),
        Some(ClaudeStopReason::ToolUse)
    );
    assert_eq!(
        claude_dialect::parse_stop_reason("max_tokens"),
        Some(ClaudeStopReason::MaxTokens)
    );
    assert_eq!(
        claude_dialect::parse_stop_reason("stop_sequence"),
        Some(ClaudeStopReason::StopSequence)
    );
    assert_eq!(claude_dialect::parse_stop_reason("unknown"), None);
}

#[test]
fn claude_stop_reason_mapping() {
    assert_eq!(
        claude_dialect::map_stop_reason(ClaudeStopReason::EndTurn),
        "end_turn"
    );
    assert_eq!(
        claude_dialect::map_stop_reason(ClaudeStopReason::ToolUse),
        "tool_use"
    );
    assert_eq!(
        claude_dialect::map_stop_reason(ClaudeStopReason::MaxTokens),
        "max_tokens"
    );
    assert_eq!(
        claude_dialect::map_stop_reason(ClaudeStopReason::StopSequence),
        "stop_sequence"
    );
}

#[test]
fn gemini_finish_reason_stop() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Done.".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let events = gemini_dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
}

// =========================================================================
// 12. Model name passthrough
// =========================================================================

#[test]
fn openai_canonical_model_roundtrip() {
    let canonical = openai_dialect::to_canonical_model("gpt-4o");
    assert_eq!(canonical, "openai/gpt-4o");
    let back = openai_dialect::from_canonical_model(&canonical);
    assert_eq!(back, "gpt-4o");
}

#[test]
fn openai_from_canonical_no_prefix() {
    let back = openai_dialect::from_canonical_model("custom-model");
    assert_eq!(back, "custom-model");
}

#[test]
fn claude_canonical_model_roundtrip() {
    let canonical = claude_dialect::to_canonical_model("claude-sonnet-4-20250514");
    assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
    let back = claude_dialect::from_canonical_model(&canonical);
    assert_eq!(back, "claude-sonnet-4-20250514");
}

#[test]
fn gemini_canonical_model_roundtrip() {
    let canonical = gemini_dialect::to_canonical_model("gemini-2.5-flash");
    assert_eq!(canonical, "google/gemini-2.5-flash");
    let back = gemini_dialect::from_canonical_model(&canonical);
    assert_eq!(back, "gemini-2.5-flash");
}

#[test]
fn openai_known_models() {
    assert!(openai_dialect::is_known_model("gpt-4o"));
    assert!(openai_dialect::is_known_model("gpt-4o-mini"));
    assert!(!openai_dialect::is_known_model("not-a-model"));
}

#[test]
fn claude_known_models() {
    assert!(claude_dialect::is_known_model("claude-sonnet-4-20250514"));
    assert!(claude_dialect::is_known_model("claude-opus-4-20250514"));
    assert!(!claude_dialect::is_known_model("not-a-model"));
}

#[test]
fn gemini_known_models() {
    assert!(gemini_dialect::is_known_model("gemini-2.5-flash"));
    assert!(gemini_dialect::is_known_model("gemini-2.5-pro"));
    assert!(!gemini_dialect::is_known_model("not-a-model"));
}

#[test]
fn openai_work_order_model_override() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
    let cfg = OpenAIConfig::default();
    let req = openai_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn claude_work_order_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("claude-opus-4-20250514")
        .build();
    let cfg = ClaudeConfig::default();
    let req = claude_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "claude-opus-4-20250514");
}

#[test]
fn gemini_work_order_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("gemini-2.5-pro")
        .build();
    let cfg = GeminiConfig::default();
    let req = gemini_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.5-pro");
}

// =========================================================================
// 13. Temperature / sampling params preserved
// =========================================================================

#[test]
fn openai_temperature_preserved() {
    let cfg = OpenAIConfig {
        temperature: Some(0.7),
        ..OpenAIConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = openai_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.temperature, Some(0.7));
}

#[test]
fn openai_max_tokens_preserved() {
    let cfg = OpenAIConfig {
        max_tokens: Some(8192),
        ..OpenAIConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = openai_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_tokens, Some(8192));
}

#[test]
fn gemini_temperature_and_max_tokens_preserved() {
    let cfg = GeminiConfig {
        temperature: Some(1.5),
        max_output_tokens: Some(2048),
        ..GeminiConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = gemini_dialect::map_work_order(&wo, &cfg);
    let gen_cfg = req.generation_config.unwrap();
    assert_eq!(gen_cfg.temperature, Some(1.5));
    assert_eq!(gen_cfg.max_output_tokens, Some(2048));
}

#[test]
fn gemini_no_generation_config_when_no_params() {
    let cfg = GeminiConfig {
        temperature: None,
        max_output_tokens: None,
        ..GeminiConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = gemini_dialect::map_work_order(&wo, &cfg);
    assert!(req.generation_config.is_none());
}

#[test]
fn claude_max_tokens_preserved() {
    let cfg = ClaudeConfig {
        max_tokens: 16384,
        ..ClaudeConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = claude_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_tokens, 16384);
}

// =========================================================================
// 14. Empty messages handled gracefully
// =========================================================================

#[test]
fn openai_empty_messages_roundtrip() {
    let conv = openai_lowering::to_ir(&[]);
    assert!(conv.is_empty());
    let back = openai_lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn claude_empty_messages_roundtrip() {
    let conv = claude_lowering::to_ir(&[], None);
    assert!(conv.is_empty());
    let back = claude_lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn gemini_empty_contents_roundtrip() {
    let conv = gemini_lowering::to_ir(&[], None);
    assert!(conv.is_empty());
    let back = gemini_lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn openai_empty_content_string() {
    let msgs = vec![oai_msg("user", Some(""), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn openai_none_content() {
    let msgs = vec![oai_msg("assistant", None, None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn claude_empty_content_string() {
    let msgs = vec![claude_msg("user", "")];
    let conv = claude_lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].text_content(), "");
}

#[test]
fn openai_tool_result_without_content() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: None,
        tool_calls: None,
        tool_call_id: Some("c1".into()),
    }];
    let conv = openai_lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn claude_tool_result_without_content() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_empty".into(),
        content: None,
        is_error: None,
    }];
    let msgs = vec![claude_msg("user", &serde_json::to_string(&blocks).unwrap())];
    let conv = claude_lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// =========================================================================
// 15. Large payload roundtrips
// =========================================================================

#[test]
fn openai_large_text_roundtrip() {
    let large_text = "x".repeat(100_000);
    let msgs = vec![oai_msg("user", Some(&large_text), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].text_content().len(), 100_000);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_ref().unwrap().len(), 100_000);
}

#[test]
fn claude_large_text_roundtrip() {
    let large_text = "y".repeat(100_000);
    let msgs = vec![claude_msg("user", &large_text)];
    let conv = claude_lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].text_content().len(), 100_000);
    let back = claude_lowering::from_ir(&conv);
    assert_eq!(back[0].content.len(), 100_000);
}

#[test]
fn gemini_large_text_roundtrip() {
    let large_text = "z".repeat(100_000);
    let contents = vec![gemini_content(
        "user",
        vec![GeminiPart::Text(large_text.clone())],
    )];
    let conv = gemini_lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].text_content().len(), 100_000);
    let back = gemini_lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t.len(), 100_000),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn openai_many_messages_roundtrip() {
    let msgs: Vec<OpenAIMessage> = (0..200)
        .map(|i| {
            oai_msg(
                if i % 2 == 0 { "user" } else { "assistant" },
                Some(&format!("Message {i}")),
                None,
                None,
            )
        })
        .collect();
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 200);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back.len(), 200);
    assert_eq!(back[199].content.as_deref(), Some("Message 199"));
}

#[test]
fn gemini_many_contents_roundtrip() {
    let contents: Vec<GeminiContent> = (0..200)
        .map(|i| {
            gemini_content(
                if i % 2 == 0 { "user" } else { "model" },
                vec![GeminiPart::Text(format!("Part {i}"))],
            )
        })
        .collect();
    let conv = gemini_lowering::to_ir(&contents, None);
    assert_eq!(conv.len(), 200);
    let back = gemini_lowering::from_ir(&conv);
    assert_eq!(back.len(), 200);
}

// =========================================================================
// Additional cross-cutting tests
// =========================================================================

#[test]
fn openai_unknown_role_defaults_to_user() {
    let msgs = vec![oai_msg("developer", Some("hi"), None, None)];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn openai_malformed_tool_args_preserved_as_string() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_bad".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "foo".into(),
                arguments: "not-json".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn openai_assistant_text_and_tool_calls() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Let me check.".into()),
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_7".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "ls".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = openai_lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Let me check."));
    assert!(back[0].tool_calls.is_some());
}

#[test]
fn openai_conversation_system_accessor() {
    let msgs = vec![
        oai_msg("system", Some("instructions"), None, None),
        oai_msg("user", Some("hi"), None, None),
    ];
    let conv = openai_lowering::to_ir(&msgs);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "instructions");
}

#[test]
fn ir_conversation_last_assistant() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::text(IrRole::Assistant, "hello"),
        IrMessage::text(IrRole::User, "bye"),
        IrMessage::text(IrRole::Assistant, "goodbye"),
    ]);
    let last = conv.last_assistant().unwrap();
    assert_eq!(last.text_content(), "goodbye");
}

#[test]
fn ir_conversation_messages_by_role() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "a"),
        IrMessage::text(IrRole::Assistant, "b"),
        IrMessage::text(IrRole::User, "c"),
    ]);
    let user_msgs = conv.messages_by_role(IrRole::User);
    assert_eq!(user_msgs.len(), 2);
}

#[test]
fn ir_conversation_tool_calls_across_messages() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "a".into(),
                input: json!({}),
            }],
        ),
        IrMessage::text(IrRole::User, "ok"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "b".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn ir_message_is_text_only() {
    let text_msg = IrMessage::text(IrRole::User, "hi");
    assert!(text_msg.is_text_only());

    let mixed = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "hi".into() },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "a".into(),
                input: json!({}),
            },
        ],
    );
    assert!(!mixed.is_text_only());
}

#[test]
fn openai_response_empty_text_ignored() {
    let resp = OpenAIResponse {
        id: "cmpl-1".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: Some("".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = openai_dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn openai_validation_extended_fields() {
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        top_logprobs: None,
        logit_bias: None,
        seed: Some(42),
    };
    let result = validation::validate_for_mapped_mode(&fields);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.errors.len(), 2);
}

#[test]
fn openai_validation_clean_fields_pass() {
    let fields = ExtendedRequestFields::default();
    assert!(validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn claude_passthrough_roundtrip() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "hello".into(),
        },
    };
    let wrapped = claude_dialect::to_passthrough_event(&event);
    let extracted = claude_dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(extracted, event);
}

#[test]
fn claude_passthrough_fidelity_check() {
    let events = vec![
        ClaudeStreamEvent::Ping {},
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "data".into(),
            },
        },
        ClaudeStreamEvent::MessageStop {},
    ];
    assert!(claude_dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn claude_map_tool_result_helper() {
    let msg = claude_dialect::map_tool_result("tu_1", "output data", false);
    assert_eq!(msg.role, "user");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tu_1");
            assert_eq!(content.as_deref(), Some("output data"));
            assert!(is_error.is_none());
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn claude_map_tool_result_error() {
    let msg = claude_dialect::map_tool_result("tu_err", "not found", true);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(*is_error, Some(true));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn openai_tool_call_accumulator_finish_as_openai() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("test_fn".into()),
            arguments: Some(r#"{"a":1}"#.into()),
        }),
    }]);
    let pairs = acc.finish_as_openai();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "c1");
    assert_eq!(pairs[0].1.name, "test_fn");
    assert_eq!(pairs[0].1.arguments, r#"{"a":1}"#);
}

#[test]
fn openai_stream_chunk_with_usage() {
    let chunk = ChatCompletionChunk {
        id: "cmpl-u".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: Some(ChunkUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    assert_eq!(chunk.usage.as_ref().unwrap().total_tokens, 15);
}

#[test]
fn gemini_map_stream_event_alias() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("test".into())],
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    };
    let a = gemini_dialect::map_stream_chunk(&chunk);
    let b = gemini_dialect::map_stream_event(&chunk);
    assert_eq!(a.len(), b.len());
}

#[test]
fn claude_thinking_config() {
    use abp_claude_sdk::dialect::ThinkingConfig;
    let tc = ThinkingConfig::new(4096);
    assert_eq!(tc.thinking_type, "enabled");
    assert_eq!(tc.budget_tokens, 4096);
}

#[test]
fn openai_response_format_variants() {
    use abp_openai_sdk::response_format::ResponseFormat;
    let text = ResponseFormat::text();
    assert!(matches!(text, ResponseFormat::Text));

    let json_obj = ResponseFormat::json_object();
    assert!(matches!(json_obj, ResponseFormat::JsonObject));

    let schema = ResponseFormat::json_schema("test", json!({"type": "object"}));
    assert!(matches!(schema, ResponseFormat::JsonSchema { .. }));
}

#[test]
fn ir_conversation_push_chain() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::Assistant, "b"));
    assert_eq!(conv.len(), 2);
}

#[test]
fn ir_conversation_last_message() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "first"),
        IrMessage::text(IrRole::Assistant, "last"),
    ]);
    assert_eq!(conv.last_message().unwrap().text_content(), "last");
}

#[test]
fn openai_config_defaults() {
    let cfg = OpenAIConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.base_url.contains("openai.com"));
}

#[test]
fn claude_config_defaults() {
    let cfg = ClaudeConfig::default();
    assert_eq!(cfg.model, "claude-sonnet-4-20250514");
    assert_eq!(cfg.max_tokens, 4096);
    assert!(cfg.base_url.contains("anthropic.com"));
}

#[test]
fn gemini_config_defaults() {
    let cfg = GeminiConfig::default();
    assert_eq!(cfg.model, "gemini-2.5-flash");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.base_url.contains("googleapis.com"));
}

#[test]
fn openai_capability_manifest() {
    use abp_core::Capability;
    let m = openai_dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
    assert!(m.contains_key(&Capability::McpClient));
    // Verify manifest is non-empty
    assert!(!m.is_empty());
}

#[test]
fn claude_capability_manifest() {
    use abp_core::Capability;
    let m = claude_dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
    assert!(m.contains_key(&Capability::ToolRead));
    assert!(m.contains_key(&Capability::McpClient));
}

#[test]
fn gemini_capability_manifest() {
    use abp_core::Capability;
    let m = gemini_dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
    assert!(m.contains_key(&Capability::ToolGlob));
}

#[test]
fn openai_dialect_version() {
    assert_eq!(openai_dialect::DIALECT_VERSION, "openai/v0.1");
}

#[test]
fn claude_dialect_version() {
    assert_eq!(claude_dialect::DIALECT_VERSION, "claude/v0.1");
}

#[test]
fn gemini_dialect_version() {
    assert_eq!(gemini_dialect::DIALECT_VERSION, "gemini/v0.1");
}

// =========================================================================
// Helpers
// =========================================================================

fn oai_msg(
    role: &str,
    content: Option<&str>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
    tool_call_id: Option<&str>,
) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: content.map(|s| s.into()),
        tool_calls,
        tool_call_id: tool_call_id.map(|s| s.into()),
    }
}

fn claude_msg(role: &str, content: &str) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: content.into(),
    }
}

fn gemini_content(role: &str, parts: Vec<GeminiPart>) -> GeminiContent {
    GeminiContent {
        role: role.into(),
        parts,
    }
}

fn openai_response(model: &str, text: &str, finish_reason: Option<&str>) -> OpenAIResponse {
    OpenAIResponse {
        id: "cmpl-test".into(),
        object: "chat.completion".into(),
        model: model.into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: Some(text.into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: finish_reason.map(|s| s.into()),
        }],
        usage: None,
    }
}
