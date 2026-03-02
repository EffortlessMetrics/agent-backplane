// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for Gemini SDK dialect types, lowering, and serde roundtrips.

use serde_json::json;

use abp_gemini_sdk::dialect::*;
use abp_gemini_sdk::lowering;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, WorkOrderBuilder};

// =========================================================================
// Helpers
// =========================================================================

fn user_text(text: &str) -> GeminiContent {
    GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

fn model_text(text: &str) -> GeminiContent {
    GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

fn make_response(parts: Vec<GeminiPart>) -> GeminiResponse {
    GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts,
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    }
}

fn make_stream_chunk(parts: Vec<GeminiPart>) -> GeminiStreamChunk {
    GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts,
            },
            finish_reason: None,
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: None,
    }
}

// =========================================================================
// 1. Roles
// =========================================================================

#[test]
fn role_user_maps_to_ir_user() {
    let conv = lowering::to_ir(&[user_text("hi")], None);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn role_model_maps_to_ir_assistant() {
    let conv = lowering::to_ir(&[model_text("hello")], None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn unknown_role_maps_to_ir_user() {
    let c = GeminiContent {
        role: "unknown_role".into(),
        parts: vec![GeminiPart::Text("x".into())],
    };
    let conv = lowering::to_ir(&[c], None);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn ir_user_maps_to_gemini_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

#[test]
fn ir_assistant_maps_to_gemini_model() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "hi")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "model");
}

#[test]
fn ir_tool_maps_to_gemini_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Tool, "result")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

#[test]
fn ir_system_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

// =========================================================================
// 2. GeminiRequest construction
// =========================================================================

#[test]
fn request_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.5-flash");
}

#[test]
fn request_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("gemini-2.5-pro")
        .build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gemini-2.5-pro");
}

#[test]
fn request_has_user_content() {
    let wo = WorkOrderBuilder::new("Do something").build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.contents.len(), 1);
    assert_eq!(req.contents[0].role, "user");
    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => assert!(t.contains("Do something")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn request_generation_config_from_defaults() {
    let cfg = GeminiConfig::default();
    let wo = WorkOrderBuilder::new("task").build();
    let req = map_work_order(&wo, &cfg);
    let gc = req.generation_config.unwrap();
    assert_eq!(gc.max_output_tokens, Some(4096));
    assert_eq!(gc.temperature, None);
}

#[test]
fn request_generation_config_with_temperature() {
    let cfg = GeminiConfig {
        temperature: Some(0.7),
        ..GeminiConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = map_work_order(&wo, &cfg);
    let gc = req.generation_config.unwrap();
    assert_eq!(gc.temperature, Some(0.7));
}

#[test]
fn request_no_generation_config_when_none() {
    let cfg = GeminiConfig {
        max_output_tokens: None,
        temperature: None,
        ..GeminiConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = map_work_order(&wo, &cfg);
    assert!(req.generation_config.is_none());
}

#[test]
fn request_no_system_instruction_by_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(req.system_instruction.is_none());
}

#[test]
fn request_no_tools_by_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(req.tools.is_none());
    assert!(req.tool_config.is_none());
}

#[test]
fn request_no_safety_settings_by_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(req.safety_settings.is_none());
}

// =========================================================================
// 3. GeminiResponse parsing / map_response
// =========================================================================

#[test]
fn response_text_produces_assistant_message() {
    let resp = make_response(vec![GeminiPart::Text("Hello!".into())]);
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn response_function_call_produces_tool_call() {
    let resp = make_response(vec![GeminiPart::FunctionCall {
        name: "search".into(),
        args: json!({"q": "rust"}),
    }]);
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "search");
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn response_function_response_produces_tool_result() {
    let resp = make_response(vec![GeminiPart::FunctionResponse {
        name: "read".into(),
        response: json!("file contents"),
    }]);
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            tool_name,
            output,
            is_error,
            ..
        } => {
            assert_eq!(tool_name, "read");
            assert_eq!(output, &json!("file contents"));
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn response_inline_data_produces_no_events() {
    let resp = make_response(vec![GeminiPart::InlineData(GeminiInlineData {
        mime_type: "image/png".into(),
        data: "abc".into(),
    })]);
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn response_multiple_parts_produce_multiple_events() {
    let resp = make_response(vec![
        GeminiPart::Text("Thinking...".into()),
        GeminiPart::FunctionCall {
            name: "run".into(),
            args: json!({}),
        },
    ]);
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn response_multiple_candidates() {
    let resp = GeminiResponse {
        candidates: vec![
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("A".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
            GeminiCandidate {
                content: GeminiContent {
                    role: "model".into(),
                    parts: vec![GeminiPart::Text("B".into())],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
                citation_metadata: None,
            },
        ],
        usage_metadata: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
}

#[test]
fn response_empty_candidates() {
    let resp = GeminiResponse {
        candidates: vec![],
        usage_metadata: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn response_with_usage_metadata() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("hi".into())],
            },
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 20,
            total_token_count: 30,
        }),
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
}

// =========================================================================
// 4. Stream chunk mapping
// =========================================================================

#[test]
fn stream_chunk_text_produces_delta() {
    let chunk = make_stream_chunk(vec![GeminiPart::Text("Hello".into())]);
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn stream_chunk_function_call_produces_tool_call() {
    let chunk = make_stream_chunk(vec![GeminiPart::FunctionCall {
        name: "exec".into(),
        args: json!({"cmd": "ls"}),
    }]);
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "exec"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_chunk_function_response_produces_tool_result() {
    let chunk = make_stream_chunk(vec![GeminiPart::FunctionResponse {
        name: "exec".into(),
        response: json!("output"),
    }]);
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn stream_chunk_inline_data_ignored() {
    let chunk = make_stream_chunk(vec![GeminiPart::InlineData(GeminiInlineData {
        mime_type: "image/jpeg".into(),
        data: "data".into(),
    })]);
    let events = map_stream_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn stream_chunk_empty_candidates() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: None,
    };
    let events = map_stream_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn map_stream_event_is_alias_for_map_stream_chunk() {
    let chunk = make_stream_chunk(vec![GeminiPart::Text("x".into())]);
    let a = map_stream_chunk(&chunk);
    let b = map_stream_event(&chunk);
    assert_eq!(a.len(), b.len());
}

// =========================================================================
// 5. Content/parts handling
// =========================================================================

#[test]
fn text_part_to_ir_and_back() {
    let contents = vec![user_text("hello world")];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].text_content(), "hello world");
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "hello world"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn inline_data_to_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/webp".into(),
            data: "encodeddata".into(),
        })],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/webp");
            assert_eq!(data, "encodeddata");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn inline_data_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/gif".into(),
            data: "R0lGODlh".into(),
        })],
    }];
    let conv = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/gif");
            assert_eq!(d.data, "R0lGODlh");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn multiple_text_parts_in_one_content() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::Text("first".into()),
            GeminiPart::Text("second".into()),
        ],
    }];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].content.len(), 2);
    assert_eq!(conv.messages[0].text_content(), "firstsecond");
}

#[test]
fn mixed_text_and_image_parts() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::Text("Look at this:".into()),
            GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/png".into(),
                data: "imgdata".into(),
            }),
        ],
    }];
    let conv = lowering::to_ir(&contents, None);
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

#[test]
fn empty_parts_produces_empty_content_blocks() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![],
    }];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.messages.len(), 1);
    assert!(conv.messages[0].content.is_empty());
}

// =========================================================================
// 6. Tool calling (function declarations, calls, responses)
// =========================================================================

#[test]
fn function_declaration_to_gemini() {
    let canonical = CanonicalToolDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let decl = tool_def_to_gemini(&canonical);
    assert_eq!(decl.name, "search");
    assert_eq!(decl.description, "Search the web");
    assert_eq!(decl.parameters, canonical.parameters_schema);
}

#[test]
fn function_declaration_from_gemini() {
    let decl = GeminiFunctionDeclaration {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object"}),
    };
    let canonical = tool_def_from_gemini(&decl);
    assert_eq!(canonical.name, "read_file");
    assert_eq!(canonical.description, "Read a file");
    assert_eq!(canonical.parameters_schema, json!({"type": "object"}));
}

#[test]
fn tool_def_roundtrip_canonical_to_gemini_to_canonical() {
    let original = CanonicalToolDef {
        name: "write_file".into(),
        description: "Write to a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let gemini = tool_def_to_gemini(&original);
    let back = tool_def_from_gemini(&gemini);
    assert_eq!(original, back);
}

#[test]
fn function_call_to_ir_synthesizes_id() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "my_func".into(),
            args: json!({"a": 1}),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "gemini_my_func");
            assert_eq!(name, "my_func");
            assert_eq!(input, &json!({"a": 1}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn function_call_roundtrip_preserves_name_and_args() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "compile".into(),
            args: json!({"flags": ["-O2"]}),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "compile");
            assert_eq!(args, &json!({"flags": ["-O2"]}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn function_response_string_to_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("found it"),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "gemini_search");
            assert!(!is_error);
            match &content[0] {
                IrContentBlock::Text { text } => assert_eq!(text, "found it"),
                other => panic!("expected text block, got {other:?}"),
            }
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn function_response_object_to_ir_serializes_json() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "api".into(),
            response: json!({"status": 200}),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => match &content[0] {
            IrContentBlock::Text { text } => {
                assert!(text.contains("200"));
                assert!(text.contains("status"));
            }
            other => panic!("expected Text, got {other:?}"),
        },
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn function_response_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "read".into(),
            response: json!("file contents here"),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "read");
            assert_eq!(response, &json!("file contents here"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn function_call_with_empty_args() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "noop".into(),
            args: json!({}),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "noop");
            assert_eq!(args, &json!({}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn function_call_with_nested_args() {
    let args = json!({"config": {"nested": {"deep": true}}, "list": [1, 2, 3]});
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "complex".into(),
            args: args.clone(),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall {
            name,
            args: rt_args,
        } => {
            assert_eq!(name, "complex");
            assert_eq!(rt_args, &args);
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

// =========================================================================
// 7. Gemini → IR lowering (all types)
// =========================================================================

#[test]
fn lowering_user_text() {
    let conv = lowering::to_ir(&[user_text("hello")], None);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert!(
        matches!(&conv.messages[0].content[0], IrContentBlock::Text { text } if text == "hello")
    );
}

#[test]
fn lowering_model_text() {
    let conv = lowering::to_ir(&[model_text("world")], None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn lowering_system_instruction() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("You are helpful".into())],
    };
    let conv = lowering::to_ir(&[user_text("hi")], Some(&sys));
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "You are helpful");
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn lowering_system_with_multiple_text_parts() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::Text("Be ".into()),
            GeminiPart::Text("concise.".into()),
        ],
    };
    let conv = lowering::to_ir(&[], Some(&sys));
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be concise.");
}

#[test]
fn lowering_system_with_non_text_parts_ignored() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "x".into(),
        })],
    };
    let conv = lowering::to_ir(&[], Some(&sys));
    // non-text parts in system instruction produce empty text → no system message
    assert!(conv.is_empty());
}

#[test]
fn lowering_empty_system_instruction_skipped() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![],
    };
    let conv = lowering::to_ir(&[user_text("hi")], Some(&sys));
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_inline_data_to_ir_image() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/jpeg".into(),
            data: "base64==".into(),
        })],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "base64==");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn lowering_function_call_to_ir_tool_use() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "grep".into(),
            args: json!({"pattern": "TODO"}),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "gemini_grep");
            assert_eq!(name, "grep");
            assert_eq!(input, &json!({"pattern": "TODO"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn lowering_function_response_to_ir_tool_result() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "grep".into(),
            response: json!("match found"),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "gemini_grep");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// =========================================================================
// 8. IR → Gemini lifting (all types)
// =========================================================================

#[test]
fn lifting_text_to_gemini_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "hi"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn lifting_image_to_gemini_inline_data() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/svg+xml".into(),
            data: "svgdata".into(),
        }],
    );
    let conv = IrConversation::from_messages(vec![msg]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/svg+xml");
            assert_eq!(d.data, "svgdata");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn lifting_tool_use_to_gemini_function_call() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "gemini_search".into(),
            name: "search".into(),
            input: json!({"q": "test"}),
        }],
    );
    let conv = IrConversation::from_messages(vec![msg]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "test"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn lifting_tool_result_to_gemini_function_response() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "gemini_read".into(),
            content: vec![IrContentBlock::Text {
                text: "file data".into(),
            }],
            is_error: false,
        }],
    );
    let conv = IrConversation::from_messages(vec![msg]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "read");
            assert_eq!(response, &json!("file data"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn lifting_tool_result_strips_gemini_prefix() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "gemini_my_tool".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        }],
    );
    let conv = IrConversation::from_messages(vec![msg]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, .. } => assert_eq!(name, "my_tool"),
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn lifting_tool_result_without_prefix_uses_raw_id() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "custom_id".into(),
            content: vec![IrContentBlock::Text {
                text: "data".into(),
            }],
            is_error: false,
        }],
    );
    let conv = IrConversation::from_messages(vec![msg]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, .. } => assert_eq!(name, "custom_id"),
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn lifting_thinking_to_gemini_text() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Let me think...".into(),
        }],
    );
    let conv = IrConversation::from_messages(vec![msg]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Let me think..."),
        other => panic!("expected Text, got {other:?}"),
    }
}

// =========================================================================
// 9. Roundtrip: Gemini → IR → Gemini preserves semantics
// =========================================================================

#[test]
fn roundtrip_user_text() {
    let contents = vec![user_text("Hello there")];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello there"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn roundtrip_model_text() {
    let contents = vec![model_text("Sure!")];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    assert_eq!(back[0].role, "model");
}

#[test]
fn roundtrip_multi_turn() {
    let contents = vec![
        user_text("hi"),
        model_text("hello"),
        user_text("bye"),
        model_text("goodbye"),
    ];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    assert_eq!(back.len(), 4);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[1].role, "model");
    assert_eq!(back[2].role, "user");
    assert_eq!(back[3].role, "model");
}

#[test]
fn roundtrip_function_call() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "tool_a".into(),
            args: json!({"x": 42}),
        }],
    }];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "tool_a");
            assert_eq!(args, &json!({"x": 42}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn roundtrip_function_response_string() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "tool_a".into(),
            response: json!("done"),
        }],
    }];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "tool_a");
            assert_eq!(response, &json!("done"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn roundtrip_inline_data() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "audio/wav".into(),
            data: "RIFF".into(),
        })],
    }];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "audio/wav");
            assert_eq!(d.data, "RIFF");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn roundtrip_mixed_parts() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![
            GeminiPart::Text("I'll search".into()),
            GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "rust"}),
            },
        ],
    }];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    assert_eq!(back[0].parts.len(), 2);
    assert!(matches!(&back[0].parts[0], GeminiPart::Text(t) if t == "I'll search"));
    assert!(matches!(&back[0].parts[1], GeminiPart::FunctionCall { name, .. } if name == "search"));
}

#[test]
fn roundtrip_tool_call_then_response_conversation() {
    let contents = vec![
        user_text("Search for rust"),
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "rust"}),
            }],
        },
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("results"),
            }],
        },
        model_text("Here are the results."),
    ];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    assert_eq!(back.len(), 4);
    assert!(matches!(&back[1].parts[0], GeminiPart::FunctionCall { .. }));
    assert!(matches!(
        &back[2].parts[0],
        GeminiPart::FunctionResponse { .. }
    ));
}

#[test]
fn roundtrip_system_instruction_extracted() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be brief".into())],
    };
    let contents = vec![user_text("hi")];
    let conv = lowering::to_ir(&contents, Some(&sys));
    let extracted = lowering::extract_system_instruction(&conv);
    assert!(extracted.is_some());
    match &extracted.unwrap().parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be brief"),
        other => panic!("expected Text, got {other:?}"),
    }
}

// =========================================================================
// 10. Serde roundtrip for all Gemini types
// =========================================================================

#[test]
fn serde_gemini_content_text() {
    let c = user_text("hello");
    let json = serde_json::to_string(&c).unwrap();
    let back: GeminiContent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert!(matches!(&back.parts[0], GeminiPart::Text(t) if t == "hello"));
}

#[test]
fn serde_gemini_inline_data() {
    let d = GeminiInlineData {
        mime_type: "image/png".into(),
        data: "abc123".into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: GeminiInlineData = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}

#[test]
fn serde_gemini_part_text() {
    let p = GeminiPart::Text("hi".into());
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, GeminiPart::Text(t) if t == "hi"));
}

#[test]
fn serde_gemini_part_inline_data() {
    let p = GeminiPart::InlineData(GeminiInlineData {
        mime_type: "image/jpeg".into(),
        data: "data".into(),
    });
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    match back {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/jpeg");
            assert_eq!(d.data, "data");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn serde_gemini_part_function_call() {
    let p = GeminiPart::FunctionCall {
        name: "fn1".into(),
        args: json!({"a": 1}),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    match back {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "fn1");
            assert_eq!(args, json!({"a": 1}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn serde_gemini_part_function_response() {
    let p = GeminiPart::FunctionResponse {
        name: "fn1".into(),
        response: json!({"ok": true}),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: GeminiPart = serde_json::from_str(&json).unwrap();
    match back {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "fn1");
            assert_eq!(response, json!({"ok": true}));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn serde_gemini_generation_config() {
    let gc = GeminiGenerationConfig {
        max_output_tokens: Some(1024),
        temperature: Some(0.5),
        top_p: Some(0.9),
        top_k: Some(40),
        stop_sequences: Some(vec!["END".into()]),
        response_mime_type: Some("application/json".into()),
        response_schema: Some(json!({"type": "object"})),
    };
    let json = serde_json::to_string(&gc).unwrap();
    let back: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_output_tokens, Some(1024));
    assert_eq!(back.temperature, Some(0.5));
    assert_eq!(back.top_p, Some(0.9));
    assert_eq!(back.top_k, Some(40));
    assert_eq!(back.stop_sequences, Some(vec!["END".to_string()]));
    assert_eq!(back.response_mime_type, Some("application/json".into()));
}

#[test]
fn serde_gemini_generation_config_defaults() {
    let gc = GeminiGenerationConfig::default();
    let json = serde_json::to_string(&gc).unwrap();
    // all None fields should be skipped
    assert_eq!(json, "{}");
    let back: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
    assert!(back.max_output_tokens.is_none());
}

#[test]
fn serde_gemini_request_full() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![user_text("hi")],
        system_instruction: Some(GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be helpful".into())],
        }),
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: Some(2048),
            temperature: Some(1.0),
            ..Default::default()
        }),
        safety_settings: Some(vec![GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockNone,
        }]),
        tools: Some(vec![GeminiTool {
            function_declarations: vec![GeminiFunctionDeclaration {
                name: "search".into(),
                description: "Search".into(),
                parameters: json!({"type": "object"}),
            }],
        }]),
        tool_config: Some(GeminiToolConfig {
            function_calling_config: GeminiFunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        }),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GeminiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gemini-2.5-flash");
    assert!(back.system_instruction.is_some());
    assert!(back.generation_config.is_some());
    assert!(back.safety_settings.is_some());
    assert!(back.tools.is_some());
    assert!(back.tool_config.is_some());
}

#[test]
fn serde_gemini_request_minimal() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![user_text("hello")],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    // Optional fields should be absent
    assert!(!json.contains("system_instruction"));
    assert!(!json.contains("generation_config"));
    let back: GeminiRequest = serde_json::from_str(&json).unwrap();
    assert!(back.system_instruction.is_none());
}

#[test]
fn serde_gemini_response() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: model_text("hi"),
            finish_reason: Some("STOP".into()),
            safety_ratings: Some(vec![GeminiSafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            }]),
            citation_metadata: None,
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 5,
            candidates_token_count: 10,
            total_token_count: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GeminiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.candidates.len(), 1);
    assert_eq!(back.usage_metadata.as_ref().unwrap().total_token_count, 15);
}

#[test]
fn serde_gemini_usage_metadata() {
    let u = GeminiUsageMetadata {
        prompt_token_count: 100,
        candidates_token_count: 200,
        total_token_count: 300,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: GeminiUsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.prompt_token_count, 100);
    assert_eq!(back.candidates_token_count, 200);
    assert_eq!(back.total_token_count, 300);
}

#[test]
fn serde_gemini_stream_chunk() {
    let chunk = make_stream_chunk(vec![GeminiPart::Text("delta".into())]);
    let json = serde_json::to_string(&chunk).unwrap();
    let back: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back.candidates.len(), 1);
}

#[test]
fn serde_harm_category_all_variants() {
    let categories = vec![
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ];
    for cat in &categories {
        let json = serde_json::to_string(cat).unwrap();
        let back: HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, cat);
    }
}

#[test]
fn serde_harm_block_threshold_all_variants() {
    let thresholds = vec![
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockLowAndAbove,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for t in &thresholds {
        let json = serde_json::to_string(t).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, t);
    }
}

#[test]
fn serde_harm_probability_all_variants() {
    let probs = vec![
        HarmProbability::Negligible,
        HarmProbability::Low,
        HarmProbability::Medium,
        HarmProbability::High,
    ];
    for p in &probs {
        let json = serde_json::to_string(p).unwrap();
        let back: HarmProbability = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, p);
    }
}

#[test]
fn serde_function_calling_mode_all_variants() {
    let modes = vec![
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ];
    for m in &modes {
        let json = serde_json::to_string(m).unwrap();
        let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, m);
    }
}

#[test]
fn serde_gemini_safety_setting() {
    let s = GeminiSafetySetting {
        category: HarmCategory::HarmCategoryHateSpeech,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
}

#[test]
fn serde_gemini_safety_rating() {
    let r = GeminiSafetyRating {
        category: HarmCategory::HarmCategoryDangerousContent,
        probability: HarmProbability::Low,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: GeminiSafetyRating = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
}

#[test]
fn serde_gemini_tool() {
    let tool = GeminiTool {
        function_declarations: vec![GeminiFunctionDeclaration {
            name: "exec".into(),
            description: "Execute command".into(),
            parameters: json!({"type": "object", "properties": {"cmd": {"type": "string"}}}),
        }],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn serde_gemini_tool_config_with_allowed_names() {
    let tc = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into(), "read".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn serde_gemini_tool_config_without_allowed_names() {
    let tc = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::Auto,
            allowed_function_names: None,
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    assert!(!json.contains("allowedFunctionNames"));
    let back: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn serde_gemini_grounding_config() {
    let gc = GeminiGroundingConfig {
        google_search_retrieval: Some(GoogleSearchRetrieval {
            dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                mode: "MODE_DYNAMIC".into(),
                dynamic_threshold: Some(0.5),
            }),
        }),
    };
    let json = serde_json::to_string(&gc).unwrap();
    let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, gc);
}

#[test]
fn serde_gemini_citation_metadata() {
    let cm = GeminiCitationMetadata {
        citation_sources: vec![GeminiCitationSource {
            start_index: Some(0),
            end_index: Some(10),
            uri: Some("https://example.com".into()),
            license: Some("MIT".into()),
        }],
    };
    let json = serde_json::to_string(&cm).unwrap();
    let back: GeminiCitationMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cm);
}

#[test]
fn serde_gemini_citation_source_minimal() {
    let cs = GeminiCitationSource {
        start_index: None,
        end_index: None,
        uri: None,
        license: None,
    };
    let json = serde_json::to_string(&cs).unwrap();
    assert_eq!(json, "{}");
    let back: GeminiCitationSource = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cs);
}

#[test]
fn serde_gemini_function_declaration() {
    let fd = GeminiFunctionDeclaration {
        name: "calc".into(),
        description: "Calculator".into(),
        parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&fd).unwrap();
    let back: GeminiFunctionDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(back, fd);
}

#[test]
fn serde_canonical_tool_def() {
    let ct = CanonicalToolDef {
        name: "lint".into(),
        description: "Run linter".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&ct).unwrap();
    let back: CanonicalToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ct);
}

// =========================================================================
// 11. Gemini-specific features
// =========================================================================

#[test]
fn generation_config_all_fields_serde() {
    let gc = GeminiGenerationConfig {
        max_output_tokens: Some(8192),
        temperature: Some(1.5),
        top_p: Some(0.95),
        top_k: Some(64),
        stop_sequences: Some(vec!["<END>".into(), "STOP".into()]),
        response_mime_type: Some("text/plain".into()),
        response_schema: Some(json!({"type": "string"})),
    };
    let json = serde_json::to_string(&gc).unwrap();
    assert!(json.contains("maxOutputTokens"));
    assert!(json.contains("topP"));
    assert!(json.contains("topK"));
    assert!(json.contains("stopSequences"));
    assert!(json.contains("responseMimeType"));
    assert!(json.contains("responseSchema"));
    let back: GeminiGenerationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_output_tokens, Some(8192));
    assert_eq!(back.top_k, Some(64));
}

#[test]
fn gemini_config_default_values() {
    let cfg = GeminiConfig::default();
    assert!(cfg.api_key.is_empty());
    assert!(cfg.base_url.contains("googleapis.com"));
    assert_eq!(cfg.model, "gemini-2.5-flash");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
}

#[test]
fn gemini_config_serde() {
    let cfg = GeminiConfig {
        api_key: "test-key".into(),
        base_url: "https://example.com".into(),
        model: "gemini-2.5-pro".into(),
        max_output_tokens: Some(2048),
        temperature: Some(0.3),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: GeminiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gemini-2.5-pro");
    assert_eq!(back.max_output_tokens, Some(2048));
    assert_eq!(back.temperature, Some(0.3));
}

// =========================================================================
// 12. Model name mapping
// =========================================================================

#[test]
fn to_canonical_model_adds_prefix() {
    assert_eq!(
        to_canonical_model("gemini-2.5-flash"),
        "google/gemini-2.5-flash"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        from_canonical_model("google/gemini-2.5-flash"),
        "gemini-2.5-flash"
    );
}

#[test]
fn from_canonical_model_no_prefix_returns_as_is() {
    assert_eq!(from_canonical_model("gemini-2.5-flash"), "gemini-2.5-flash");
}

#[test]
fn is_known_model_true_for_known() {
    assert!(is_known_model("gemini-2.5-flash"));
    assert!(is_known_model("gemini-2.5-pro"));
    assert!(is_known_model("gemini-2.0-flash"));
    assert!(is_known_model("gemini-2.0-flash-lite"));
    assert!(is_known_model("gemini-1.5-flash"));
    assert!(is_known_model("gemini-1.5-pro"));
}

#[test]
fn is_known_model_false_for_unknown() {
    assert!(!is_known_model("gpt-4"));
    assert!(!is_known_model("claude-3"));
    assert!(!is_known_model("gemini-99"));
}

// =========================================================================
// 13. Capability manifest
// =========================================================================

#[test]
fn capability_manifest_has_streaming() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(matches!(m.get(&Capability::Streaming), Some(s) if format!("{s:?}") == "Native"));
}

#[test]
fn capability_manifest_has_tool_read() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(matches!(m.get(&Capability::ToolRead), Some(s) if format!("{s:?}") == "Native"));
}

#[test]
fn capability_manifest_has_structured_output() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(s) if format!("{s:?}") == "Native"
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(matches!(m.get(&Capability::McpClient), Some(s) if format!("{s:?}") == "Unsupported"));
    assert!(matches!(m.get(&Capability::McpServer), Some(s) if format!("{s:?}") == "Unsupported"));
}

// =========================================================================
// 14. Edge cases
// =========================================================================

#[test]
fn empty_contents_to_ir() {
    let conv = lowering::to_ir(&[], None);
    assert!(conv.is_empty());
}

#[test]
fn empty_ir_to_gemini() {
    let conv = IrConversation::from_messages(vec![]);
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn empty_contents_roundtrip() {
    let back = lowering::from_ir(&lowering::to_ir(&[], None));
    assert!(back.is_empty());
}

#[test]
fn content_with_empty_parts() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![],
    }];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.len(), 1);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn multiple_function_calls_in_one_content() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![
            GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "a"}),
            },
            GeminiPart::FunctionCall {
                name: "read".into(),
                args: json!({"f": "b"}),
            },
        ],
    }];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].content.len(), 2);
    assert!(
        matches!(&conv.messages[0].content[0], IrContentBlock::ToolUse { name, .. } if name == "search")
    );
    assert!(
        matches!(&conv.messages[0].content[1], IrContentBlock::ToolUse { name, .. } if name == "read")
    );
}

#[test]
fn multiple_function_responses_in_one_content() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("a"),
            },
            GeminiPart::FunctionResponse {
                name: "read".into(),
                response: json!("b"),
            },
        ],
    }];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].content.len(), 2);
}

#[test]
fn very_long_text_roundtrip() {
    let long_text = "x".repeat(100_000);
    let contents = vec![user_text(&long_text)];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t.len(), 100_000),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn unicode_text_roundtrip() {
    let text = "こんにちは世界 🌍 Ñoño café ü";
    let contents = vec![user_text(text)];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, text),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn empty_string_text_roundtrip() {
    let contents = vec![user_text("")];
    let back = lowering::from_ir(&lowering::to_ir(&contents, None));
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, ""),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn function_response_null_payload() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "noop".into(),
            response: serde_json::Value::Null,
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::ToolResult { .. }
    ));
}

#[test]
fn function_response_array_payload() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "list".into(),
            response: json!([1, 2, 3]),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => match &content[0] {
            IrContentBlock::Text { text } => assert!(text.contains("[1,2,3]")),
            other => panic!("expected Text, got {other:?}"),
        },
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn function_response_number_payload() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "count".into(),
            response: json!(42),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => match &content[0] {
            IrContentBlock::Text { text } => assert_eq!(text, "42"),
            other => panic!("expected Text, got {other:?}"),
        },
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn function_response_boolean_payload() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "check".into(),
            response: json!(true),
        }],
    }];
    let conv = lowering::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => match &content[0] {
            IrContentBlock::Text { text } => assert_eq!(text, "true"),
            other => panic!("expected Text, got {other:?}"),
        },
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn extract_system_instruction_returns_none_when_no_system() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    assert!(lowering::extract_system_instruction(&conv).is_none());
}

#[test]
fn extract_system_instruction_role_is_user() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let sys = lowering::extract_system_instruction(&conv).unwrap();
    assert_eq!(sys.role, "user");
}

#[test]
fn dialect_version_is_set() {
    assert_eq!(DIALECT_VERSION, "gemini/v0.1");
}

#[test]
fn default_model_is_set() {
    assert_eq!(DEFAULT_MODEL, "gemini-2.5-flash");
}

#[test]
fn multiple_inline_data_parts() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![
            GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/png".into(),
                data: "img1".into(),
            }),
            GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/jpeg".into(),
                data: "img2".into(),
            }),
        ],
    }];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].parts.len(), 2);
}

#[test]
fn lifting_tool_result_joins_multiple_text_blocks() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "gemini_multi".into(),
            content: vec![
                IrContentBlock::Text {
                    text: "first ".into(),
                },
                IrContentBlock::Text {
                    text: "second".into(),
                },
            ],
            is_error: false,
        }],
    );
    let conv = IrConversation::from_messages(vec![msg]);
    let back = lowering::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { response, .. } => {
            assert_eq!(response, &json!("first second"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn stream_chunk_with_usage_metadata() {
    let chunk = GeminiStreamChunk {
        candidates: vec![GeminiCandidate {
            content: model_text("done"),
            finish_reason: Some("STOP".into()),
            safety_ratings: None,
            citation_metadata: None,
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 50,
            candidates_token_count: 100,
            total_token_count: 150,
        }),
    };
    let events = map_stream_chunk(&chunk);
    assert_eq!(events.len(), 1);
}

#[test]
fn candidate_with_citation_metadata_serde() {
    let candidate = GeminiCandidate {
        content: model_text("cited text"),
        finish_reason: Some("STOP".into()),
        safety_ratings: None,
        citation_metadata: Some(GeminiCitationMetadata {
            citation_sources: vec![
                GeminiCitationSource {
                    start_index: Some(0),
                    end_index: Some(5),
                    uri: Some("https://example.com".into()),
                    license: None,
                },
                GeminiCitationSource {
                    start_index: Some(6),
                    end_index: Some(10),
                    uri: None,
                    license: Some("Apache-2.0".into()),
                },
            ],
        }),
    };
    let json = serde_json::to_string(&candidate).unwrap();
    let back: GeminiCandidate = serde_json::from_str(&json).unwrap();
    let cm = back.citation_metadata.unwrap();
    assert_eq!(cm.citation_sources.len(), 2);
    assert_eq!(
        cm.citation_sources[0].uri.as_deref(),
        Some("https://example.com")
    );
    assert_eq!(
        cm.citation_sources[1].license.as_deref(),
        Some("Apache-2.0")
    );
}

#[test]
fn candidate_with_safety_ratings_serde() {
    let candidate = GeminiCandidate {
        content: model_text("safe text"),
        finish_reason: Some("STOP".into()),
        safety_ratings: Some(vec![
            GeminiSafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            },
            GeminiSafetyRating {
                category: HarmCategory::HarmCategorySexuallyExplicit,
                probability: HarmProbability::Low,
            },
        ]),
        citation_metadata: None,
    };
    let json = serde_json::to_string(&candidate).unwrap();
    let back: GeminiCandidate = serde_json::from_str(&json).unwrap();
    let ratings = back.safety_ratings.unwrap();
    assert_eq!(ratings.len(), 2);
}

#[test]
fn grounding_config_empty_serde() {
    let gc = GeminiGroundingConfig {
        google_search_retrieval: None,
    };
    let json = serde_json::to_string(&gc).unwrap();
    let back: GeminiGroundingConfig = serde_json::from_str(&json).unwrap();
    assert!(back.google_search_retrieval.is_none());
}

#[test]
fn dynamic_retrieval_config_serde() {
    let drc = DynamicRetrievalConfig {
        mode: "MODE_DYNAMIC".into(),
        dynamic_threshold: Some(0.3),
    };
    let json = serde_json::to_string(&drc).unwrap();
    let back: DynamicRetrievalConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.mode, "MODE_DYNAMIC");
    assert_eq!(back.dynamic_threshold, Some(0.3));
}

#[test]
fn dynamic_retrieval_config_no_threshold_serde() {
    let drc = DynamicRetrievalConfig {
        mode: "MODE_UNSPECIFIED".into(),
        dynamic_threshold: None,
    };
    let json = serde_json::to_string(&drc).unwrap();
    assert!(!json.contains("dynamicThreshold"));
    let back: DynamicRetrievalConfig = serde_json::from_str(&json).unwrap();
    assert!(back.dynamic_threshold.is_none());
}

#[test]
fn work_order_with_context_snippets() {
    use abp_core::{ContextPacket, ContextSnippet};
    let ctx = ContextPacket {
        snippets: vec![ContextSnippet {
            name: "file.rs".into(),
            content: "fn main() {}".into(),
        }],
        ..Default::default()
    };
    let wo = WorkOrderBuilder::new("Analyze code").context(ctx).build();
    let cfg = GeminiConfig::default();
    let req = map_work_order(&wo, &cfg);
    match &req.contents[0].parts[0] {
        GeminiPart::Text(t) => {
            assert!(t.contains("Analyze code"));
            assert!(t.contains("file.rs"));
            assert!(t.contains("fn main() {}"));
        }
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn conversation_ir_methods_work_with_gemini_data() {
    let contents = vec![
        user_text("hi"),
        model_text("hello"),
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({}),
            }],
        },
    ];
    let conv = lowering::to_ir(&contents, None);
    assert_eq!(conv.len(), 3);
    assert!(conv.last_assistant().is_some());
    assert!(!conv.tool_calls().is_empty());
}

#[test]
fn gemini_tool_multiple_declarations() {
    let tool = GeminiTool {
        function_declarations: vec![
            GeminiFunctionDeclaration {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: json!({"type": "object"}),
            },
            GeminiFunctionDeclaration {
                name: "read".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object"}),
            },
        ],
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: GeminiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back.function_declarations.len(), 2);
}

#[test]
fn serde_camel_case_inline_data() {
    let json_str = r#"{"mimeType":"image/png","data":"abc"}"#;
    let d: GeminiInlineData = serde_json::from_str(json_str).unwrap();
    assert_eq!(d.mime_type, "image/png");
    assert_eq!(d.data, "abc");
    let out = serde_json::to_string(&d).unwrap();
    assert!(out.contains("mimeType"));
    assert!(!out.contains("mime_type"));
}

#[test]
fn serde_camel_case_generation_config() {
    let json_str = r#"{"maxOutputTokens":1024,"topP":0.9}"#;
    let gc: GeminiGenerationConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(gc.max_output_tokens, Some(1024));
    assert_eq!(gc.top_p, Some(0.9));
}

#[test]
fn serde_camel_case_usage_metadata() {
    let json_str = r#"{"promptTokenCount":10,"candidatesTokenCount":20,"totalTokenCount":30}"#;
    let u: GeminiUsageMetadata = serde_json::from_str(json_str).unwrap();
    assert_eq!(u.prompt_token_count, 10);
    assert_eq!(u.candidates_token_count, 20);
    assert_eq!(u.total_token_count, 30);
}

#[test]
fn serde_screaming_snake_case_harm_category() {
    let json_str = r#""HARM_CATEGORY_HARASSMENT""#;
    let c: HarmCategory = serde_json::from_str(json_str).unwrap();
    assert_eq!(c, HarmCategory::HarmCategoryHarassment);
}

#[test]
fn serde_screaming_snake_case_harm_block_threshold() {
    let json_str = r#""BLOCK_LOW_AND_ABOVE""#;
    let t: HarmBlockThreshold = serde_json::from_str(json_str).unwrap();
    assert_eq!(t, HarmBlockThreshold::BlockLowAndAbove);
}

#[test]
fn serde_screaming_snake_case_function_calling_mode() {
    let json_str = r#""AUTO""#;
    let m: FunctionCallingMode = serde_json::from_str(json_str).unwrap();
    assert_eq!(m, FunctionCallingMode::Auto);

    let json_str2 = r#""ANY""#;
    let m2: FunctionCallingMode = serde_json::from_str(json_str2).unwrap();
    assert_eq!(m2, FunctionCallingMode::Any);

    let json_str3 = r#""NONE""#;
    let m3: FunctionCallingMode = serde_json::from_str(json_str3).unwrap();
    assert_eq!(m3, FunctionCallingMode::None);
}

#[test]
fn serde_screaming_snake_case_harm_probability() {
    let json_str = r#""NEGLIGIBLE""#;
    let p: HarmProbability = serde_json::from_str(json_str).unwrap();
    assert_eq!(p, HarmProbability::Negligible);
}
