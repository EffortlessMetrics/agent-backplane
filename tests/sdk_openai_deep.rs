#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_update)]
//! Deep tests for OpenAI SDK dialect types and lowering.
//!
//! Covers all message roles, request/response construction, streaming deltas,
//! tool calling, IR lowering/lifting roundtrips, serde roundtrips, edge cases,
//! configuration, tool_choice, response_format, and validation.

use serde_json::json;

use abp_openai_sdk::dialect::{
    self, CanonicalToolDef, OpenAIChoice, OpenAIConfig, OpenAIFunctionCall, OpenAIFunctionDef,
    OpenAIMessage, OpenAIRequest, OpenAIResponse, OpenAIToolCall, OpenAIToolDef, OpenAIUsage,
    ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode,
};
use abp_openai_sdk::lowering;
use abp_openai_sdk::response_format::{JsonSchemaSpec, ResponseFormat};
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall, ChunkUsage,
    ToolCallAccumulator,
};
use abp_openai_sdk::validation::{self, ExtendedRequestFields, UnmappableParam, ValidationErrors};

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, WorkOrderBuilder};

// =========================================================================
// Helpers
// =========================================================================

fn msg(role: &str, content: Option<&str>) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: content.map(Into::into),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn tool_result_msg(content: Option<&str>, tool_call_id: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: "tool".into(),
        content: content.map(Into::into),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.into()),
    }
}

fn assistant_with_tool_calls(content: Option<&str>, calls: Vec<OpenAIToolCall>) -> OpenAIMessage {
    OpenAIMessage {
        role: "assistant".into(),
        content: content.map(Into::into),
        tool_calls: Some(calls),
        tool_call_id: None,
    }
}

fn make_tool_call(id: &str, name: &str, args: &str) -> OpenAIToolCall {
    OpenAIToolCall {
        id: id.into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
    }
}

fn simple_response(text: &str) -> OpenAIResponse {
    OpenAIResponse {
        id: "chatcmpl-test".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: msg("assistant", Some(text)),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    }
}

fn make_chunk(id: &str, delta: ChunkDelta, finish_reason: Option<&str>) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: id.into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta,
            finish_reason: finish_reason.map(Into::into),
        }],
        usage: None,
    }
}

// =========================================================================
// 1. Message roles — to_ir mapping
// =========================================================================

#[test]
fn role_system_maps_to_ir_system() {
    let conv = lowering::to_ir(&[msg("system", Some("You are helpful."))]);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn role_user_maps_to_ir_user() {
    let conv = lowering::to_ir(&[msg("user", Some("Hello"))]);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn role_assistant_maps_to_ir_assistant() {
    let conv = lowering::to_ir(&[msg("assistant", Some("Hi"))]);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn role_tool_maps_to_ir_tool() {
    let conv = lowering::to_ir(&[tool_result_msg(Some("ok"), "c1")]);
    assert_eq!(conv.messages[0].role, IrRole::Tool);
}

#[test]
fn unknown_role_maps_to_user() {
    let conv = lowering::to_ir(&[msg("developer", Some("hi"))]);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn another_unknown_role_maps_to_user() {
    let conv = lowering::to_ir(&[msg("function", Some("data"))]);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

// =========================================================================
// 2. Message roles — from_ir mapping
// =========================================================================

#[test]
fn ir_system_maps_to_system_role() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "sys")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn ir_user_maps_to_user_role() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "u")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

#[test]
fn ir_assistant_maps_to_assistant_role() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "a")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
}

#[test]
fn ir_tool_with_tool_result_maps_to_tool_role() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text { text: "ok".into() }],
            is_error: false,
        }],
    )]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].tool_call_id.as_deref(), Some("c1"));
}

// =========================================================================
// 3. Text roundtrips for every role
// =========================================================================

#[test]
fn system_text_roundtrip() {
    let msgs = [msg("system", Some("Be concise"))];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("Be concise"));
}

#[test]
fn user_text_roundtrip() {
    let msgs = [msg("user", Some("Hello"))];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].content.as_deref(), Some("Hello"));
}

#[test]
fn assistant_text_roundtrip() {
    let msgs = [msg("assistant", Some("Sure!"))];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].content.as_deref(), Some("Sure!"));
}

#[test]
fn tool_result_text_roundtrip() {
    let msgs = [tool_result_msg(Some("file data"), "call_1")];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].content.as_deref(), Some("file data"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_1"));
}

// =========================================================================
// 4. Tool calling — assistant tool calls
// =========================================================================

#[test]
fn single_tool_call_to_ir() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "read_file", r#"{"path":"main.rs"}"#)],
    )];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "c1");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "main.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn multiple_tool_calls_to_ir() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![
            make_tool_call("c1", "read", r#"{"p":"a"}"#),
            make_tool_call("c2", "write", r#"{"p":"b","d":"x"}"#),
            make_tool_call("c3", "exec", r#"{"cmd":"ls"}"#),
        ],
    )];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 3);
    let ids: Vec<&str> = conv.messages[0]
        .content
        .iter()
        .filter_map(|b| match b {
            IrContentBlock::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["c1", "c2", "c3"]);
}

#[test]
fn tool_call_roundtrip_preserves_id_and_name() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("call_42", "search", r#"{"q":"rust"}"#)],
    )];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_42");
    assert_eq!(tc.function.name, "search");
    assert_eq!(tc.call_type, "function");
}

#[test]
fn tool_call_arguments_roundtrip_as_json() {
    let args = r#"{"path":"src/lib.rs","line":42}"#;
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "edit", args)],
    )];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    let result_args = &back[0].tool_calls.as_ref().unwrap()[0].function.arguments;
    let parsed: serde_json::Value = serde_json::from_str(result_args).unwrap();
    assert_eq!(parsed, json!({"path": "src/lib.rs", "line": 42}));
}

#[test]
fn assistant_text_and_tool_calls_roundtrip() {
    let msgs = [assistant_with_tool_calls(
        Some("Let me check."),
        vec![make_tool_call("c7", "ls", "{}")],
    )];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Let me check."));
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 1);
}

#[test]
fn malformed_tool_arguments_preserved_as_string() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c_bad", "foo", "not-valid-json")],
    )];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-valid-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn empty_object_arguments_roundtrip() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "noop", "{}")],
    )];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    let args = &back[0].tool_calls.as_ref().unwrap()[0].function.arguments;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(args).unwrap(),
        json!({})
    );
}

// =========================================================================
// 5. Tool results — lowering
// =========================================================================

#[test]
fn tool_result_to_ir_creates_tool_result_block() {
    let msgs = [tool_result_msg(Some("file contents"), "call_1")];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "call_1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_without_content() {
    let msgs = [tool_result_msg(None, "c_empty")];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_roundtrip_preserves_tool_call_id() {
    let msgs = [tool_result_msg(Some("data"), "call_99")];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_99"));
    assert_eq!(back[0].content.as_deref(), Some("data"));
    assert_eq!(back[0].role, "tool");
}

#[test]
fn tool_result_empty_string_content() {
    let msgs = [tool_result_msg(Some(""), "c1")];
    let conv = lowering::to_ir(&msgs);
    // Empty string content still generates a ToolResult with text blocks
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
            // Empty string produces a text block with empty text
            assert_eq!(content.len(), 1);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// =========================================================================
// 6. Multi-turn conversations
// =========================================================================

#[test]
fn multi_turn_system_user_assistant() {
    let msgs = [
        msg("system", Some("Be helpful")),
        msg("user", Some("Hi")),
        msg("assistant", Some("Hello!")),
        msg("user", Some("Bye")),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[3].role, IrRole::User);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[3].content.as_deref(), Some("Bye"));
}

#[test]
fn multi_turn_with_tool_call_and_result() {
    let msgs = [
        msg("user", Some("Read main.rs")),
        assistant_with_tool_calls(
            None,
            vec![make_tool_call("c1", "read_file", r#"{"path":"main.rs"}"#)],
        ),
        tool_result_msg(Some("fn main() {}"), "c1"),
        msg("assistant", Some("Done.")),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
    assert_eq!(conv.messages[2].role, IrRole::Tool);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[2].tool_call_id.as_deref(), Some("c1"));
}

#[test]
fn multi_turn_multiple_tool_rounds() {
    let msgs = [
        msg("user", Some("List and read")),
        assistant_with_tool_calls(None, vec![make_tool_call("c1", "ls", "{}")]),
        tool_result_msg(Some("a.rs b.rs"), "c1"),
        assistant_with_tool_calls(None, vec![make_tool_call("c2", "read", r#"{"f":"a.rs"}"#)]),
        tool_result_msg(Some("contents of a"), "c2"),
        msg("assistant", Some("Here it is.")),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.len(), 6);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 6);
    assert_eq!(back[4].tool_call_id.as_deref(), Some("c2"));
}

#[test]
fn conversation_preserves_message_order() {
    let msgs: Vec<OpenAIMessage> = (0..50)
        .map(|i| {
            msg(
                if i % 2 == 0 { "user" } else { "assistant" },
                Some(&format!("msg-{i}")),
            )
        })
        .collect();
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    for (i, m) in back.iter().enumerate() {
        assert_eq!(m.content.as_deref(), Some(&*format!("msg-{i}")));
    }
}

// =========================================================================
// 7. Edge cases
// =========================================================================

#[test]
fn empty_messages_roundtrip() {
    let conv = lowering::to_ir(&[]);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn empty_content_string_produces_no_blocks() {
    let conv = lowering::to_ir(&[msg("user", Some(""))]);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn none_content_produces_no_blocks() {
    let conv = lowering::to_ir(&[msg("assistant", None)]);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn unicode_text_preserved() {
    let text = "こんにちは 🌍 مرحبا Привет";
    let back = lowering::from_ir(&lowering::to_ir(&[msg("user", Some(text))]));
    assert_eq!(back[0].content.as_deref(), Some(text));
}

#[test]
fn newlines_and_whitespace_preserved() {
    let text = "line1\n  line2\n\ttabbed\r\nwindows";
    let conv = lowering::to_ir(&[msg("user", Some(text))]);
    assert_eq!(conv.messages[0].text_content(), text);
}

#[test]
fn very_long_text_roundtrip() {
    let long = "x".repeat(200_000);
    let back = lowering::from_ir(&lowering::to_ir(&[msg("user", Some(&long))]));
    assert_eq!(back[0].content.as_ref().unwrap().len(), 200_000);
}

#[test]
fn special_characters_in_tool_name() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "my-tool_v2.0", "{}")],
    )];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(
        back[0].tool_calls.as_ref().unwrap()[0].function.name,
        "my-tool_v2.0"
    );
}

#[test]
fn tool_call_with_nested_json_arguments() {
    let args = r#"{"config":{"nested":{"deep":true},"list":[1,2,3]}}"#;
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "complex", args)],
    )];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input["config"]["nested"]["deep"], true);
            assert_eq!(input["config"]["list"], json!([1, 2, 3]));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_call_with_string_arguments() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "echo", r#""hello""#)],
    )];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &json!("hello"));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_call_with_array_arguments() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "multi", r#"[1, "two", 3]"#)],
    )];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &json!([1, "two", 3]));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// =========================================================================
// 8. OpenAI request construction & serde
// =========================================================================

#[test]
fn request_serde_roundtrip_minimal() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![msg("user", Some("Hi"))],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "gpt-4o");
    assert_eq!(parsed.messages.len(), 1);
    assert!(parsed.tools.is_none());
}

#[test]
fn request_serde_roundtrip_all_fields() {
    let req = OpenAIRequest {
        model: "gpt-4-turbo".into(),
        messages: vec![msg("system", Some("Be brief")), msg("user", Some("Hello"))],
        tools: Some(vec![OpenAIToolDef {
            tool_type: "function".into(),
            function: OpenAIFunctionDef {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        temperature: Some(0.7),
        max_tokens: Some(4096),
        response_format: Some(ResponseFormat::JsonObject),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "gpt-4-turbo");
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.tools.as_ref().unwrap().len(), 1);
    assert_eq!(parsed.temperature, Some(0.7));
    assert_eq!(parsed.max_tokens, Some(4096));
}

#[test]
fn request_skip_serializing_none_fields() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![msg("user", Some("Hi"))],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("tools"));
    assert!(!json.contains("tool_choice"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("response_format"));
}

#[test]
fn request_with_tools_includes_tools_field() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: Some(vec![]),
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("tools"));
}

// =========================================================================
// 9. OpenAI response parsing & serde
// =========================================================================

#[test]
fn response_serde_roundtrip() {
    let resp = OpenAIResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: msg("assistant", Some("Hello!")),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(OpenAIUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: OpenAIResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "chatcmpl-abc");
    assert_eq!(parsed.choices.len(), 1);
    assert_eq!(parsed.usage.as_ref().unwrap().total_tokens, 15);
}

#[test]
fn response_without_usage() {
    let resp = simple_response("Hi");
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: OpenAIResponse = serde_json::from_str(&json).unwrap();
    assert!(parsed.usage.is_none());
}

#[test]
fn response_multiple_choices() {
    let resp = OpenAIResponse {
        id: "cmpl-multi".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![
            OpenAIChoice {
                index: 0,
                message: msg("assistant", Some("Option A")),
                finish_reason: Some("stop".into()),
            },
            OpenAIChoice {
                index: 1,
                message: msg("assistant", Some("Option B")),
                finish_reason: Some("stop".into()),
            },
        ],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
}

#[test]
fn response_with_tool_calls_produces_tool_call_events() {
    let resp = OpenAIResponse {
        id: "cmpl-tc".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: assistant_with_tool_calls(
                None,
                vec![make_tool_call(
                    "call_abc",
                    "read_file",
                    r#"{"path":"src/main.rs"}"#,
                )],
            ),
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("call_abc"));
            assert_eq!(input, &json!({"path": "src/main.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn response_text_and_tool_calls_produce_multiple_events() {
    let resp = OpenAIResponse {
        id: "cmpl-mixed".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: assistant_with_tool_calls(
                Some("I'll check."),
                vec![make_tool_call("c1", "ls", "{}")],
            ),
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn response_empty_text_not_emitted() {
    let resp = OpenAIResponse {
        id: "cmpl-empty".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: msg("assistant", Some("")),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn response_none_content_no_events() {
    let resp = OpenAIResponse {
        id: "cmpl-none".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: msg("assistant", None),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn response_malformed_tool_args_in_event() {
    let resp = OpenAIResponse {
        id: "cmpl-bad".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: assistant_with_tool_calls(None, vec![make_tool_call("c1", "fn", "bad-json")]),
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &json!("bad-json"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// =========================================================================
// 10. Streaming delta handling
// =========================================================================

#[test]
fn chunk_text_delta_produces_assistant_delta() {
    let chunk = make_chunk(
        "cmpl-1",
        ChunkDelta {
            role: Some("assistant".into()),
            content: Some("Hello".into()),
            tool_calls: None,
        },
        None,
    );
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn chunk_empty_content_no_events() {
    let chunk = make_chunk(
        "cmpl-2",
        ChunkDelta {
            role: Some("assistant".into()),
            content: Some("".into()),
            tool_calls: None,
        },
        None,
    );
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn chunk_none_content_no_events() {
    let chunk = make_chunk(
        "cmpl-3",
        ChunkDelta {
            role: Some("assistant".into()),
            content: None,
            tool_calls: None,
        },
        None,
    );
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn chunk_with_finish_reason_stop() {
    let chunk = make_chunk(
        "cmpl-4",
        ChunkDelta {
            role: None,
            content: None,
            tool_calls: None,
        },
        Some("stop"),
    );
    assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn chunk_serde_roundtrip() {
    let chunk = ChatCompletionChunk {
        id: "cmpl-serde".into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".into()),
                content: Some("hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "cmpl-serde");
    assert_eq!(parsed.choices[0].delta.content.as_deref(), Some("hi"));
}

#[test]
fn chunk_with_usage_serde() {
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
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.usage.unwrap().total_tokens, 15);
}

#[test]
fn chunk_no_choices_produces_no_events() {
    let chunk = ChatCompletionChunk {
        id: "cmpl-empty".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

// =========================================================================
// 11. Tool call accumulator
// =========================================================================

#[test]
fn accumulator_single_tool_call() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("read_file".into()),
            arguments: Some(r#"{"path":"main.rs"}"#.into()),
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
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_use_id.as_deref(), Some("c1"));
            assert_eq!(input, &json!({"path": "main.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_incremental_arguments() {
    let mut acc = ToolCallAccumulator::new();
    // First fragment: id, name, partial args
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"#.into()),
        }),
    }]);
    // Second fragment: rest of args
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#""rust"}"#.into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_multiple_tool_calls() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        ChunkToolCall {
            index: 0,
            id: Some("c1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("a".into()),
                arguments: Some("{}".into()),
            }),
        },
        ChunkToolCall {
            index: 1,
            id: Some("c2".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("b".into()),
                arguments: Some("{}".into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn accumulator_skips_empty_name_entries() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: None,
        function: None,
    }]);
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn accumulator_finish_as_openai() {
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
fn accumulator_empty_id_produces_none_tool_use_id() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("fn".into()),
            arguments: Some("{}".into()),
        }),
    }]);
    let events = acc.finish();
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_use_id, .. } => {
            assert!(tool_use_id.is_none());
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_malformed_args_as_string() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("fn".into()),
            arguments: Some("not-json".into()),
        }),
    }]);
    let events = acc.finish();
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &json!("not-json"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// =========================================================================
// 12. OpenAI → IR → OpenAI roundtrip semantics
// =========================================================================

#[test]
fn roundtrip_preserves_system_message() {
    let msgs = [msg("system", Some("Instructions here"))];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("Instructions here"));
}

#[test]
fn roundtrip_preserves_tool_call_structure() {
    let msgs = [
        msg("user", Some("Do it")),
        assistant_with_tool_calls(
            Some("Sure"),
            vec![
                make_tool_call("c1", "read", r#"{"p":"a"}"#),
                make_tool_call("c2", "write", r#"{"p":"b"}"#),
            ],
        ),
        tool_result_msg(Some("contents"), "c1"),
        tool_result_msg(Some("ok"), "c2"),
    ];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back.len(), 4);
    // Assistant message
    assert_eq!(back[1].content.as_deref(), Some("Sure"));
    assert_eq!(back[1].tool_calls.as_ref().unwrap().len(), 2);
    assert_eq!(
        back[1].tool_calls.as_ref().unwrap()[0].function.name,
        "read"
    );
    assert_eq!(
        back[1].tool_calls.as_ref().unwrap()[1].function.name,
        "write"
    );
    // Tool results
    assert_eq!(back[2].tool_call_id.as_deref(), Some("c1"));
    assert_eq!(back[3].tool_call_id.as_deref(), Some("c2"));
}

#[test]
fn roundtrip_no_content_assistant_stays_none() {
    let msgs = [assistant_with_tool_calls(
        None,
        vec![make_tool_call("c1", "fn", "{}")],
    )];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert!(back[0].content.is_none());
    assert!(back[0].tool_calls.is_some());
}

#[test]
fn roundtrip_tool_result_no_content() {
    let msgs = [tool_result_msg(None, "c1")];
    let back = lowering::from_ir(&lowering::to_ir(&msgs));
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].tool_call_id.as_deref(), Some("c1"));
    // Empty ToolResult content → empty string join
    assert_eq!(back[0].content.as_deref(), Some(""));
}

// =========================================================================
// 13. Serde roundtrip for all OpenAI types
// =========================================================================

#[test]
fn message_serde_roundtrip_user() {
    let m = msg("user", Some("Hi"));
    let json = serde_json::to_value(&m).unwrap();
    let parsed: OpenAIMessage = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.role, "user");
    assert_eq!(parsed.content.as_deref(), Some("Hi"));
}

#[test]
fn message_serde_roundtrip_tool_calls() {
    let m = assistant_with_tool_calls(None, vec![make_tool_call("c1", "fn", r#"{"a":1}"#)]);
    let json = serde_json::to_value(&m).unwrap();
    let parsed: OpenAIMessage = serde_json::from_value(json).unwrap();
    let tc = &parsed.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "c1");
}

#[test]
fn message_serde_roundtrip_tool_result() {
    let m = tool_result_msg(Some("ok"), "call_1");
    let json = serde_json::to_value(&m).unwrap();
    let parsed: OpenAIMessage = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.role, "tool");
    assert_eq!(parsed.tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn tool_call_serde_roundtrip() {
    let tc = make_tool_call("c1", "read_file", r#"{"path":"a.rs"}"#);
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json["type"], "function");
    let parsed: OpenAIToolCall = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.id, "c1");
    assert_eq!(parsed.call_type, "function");
}

#[test]
fn function_call_serde_roundtrip() {
    let fc = OpenAIFunctionCall {
        name: "test".into(),
        arguments: r#"{"x":1}"#.into(),
    };
    let json = serde_json::to_value(&fc).unwrap();
    let parsed: OpenAIFunctionCall = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.name, "test");
    assert_eq!(parsed.arguments, r#"{"x":1}"#);
}

#[test]
fn usage_serde_roundtrip() {
    let u = OpenAIUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_value(&u).unwrap();
    let parsed: OpenAIUsage = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.prompt_tokens, 100);
    assert_eq!(parsed.total_tokens, 150);
}

#[test]
fn choice_serde_roundtrip() {
    let c = OpenAIChoice {
        index: 0,
        message: msg("assistant", Some("ok")),
        finish_reason: Some("stop".into()),
    };
    let json = serde_json::to_value(&c).unwrap();
    let parsed: OpenAIChoice = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.index, 0);
    assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
}

#[test]
fn chunk_delta_default_is_all_none() {
    let d = ChunkDelta::default();
    assert!(d.role.is_none());
    assert!(d.content.is_none());
    assert!(d.tool_calls.is_none());
}

#[test]
fn chunk_tool_call_serde_roundtrip() {
    let ctc = ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("test".into()),
            arguments: Some("{}".into()),
        }),
    };
    let json = serde_json::to_value(&ctc).unwrap();
    let parsed: ChunkToolCall = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.id.as_deref(), Some("c1"));
}

#[test]
fn chunk_function_call_serde_roundtrip() {
    let cfc = ChunkFunctionCall {
        name: Some("fn".into()),
        arguments: Some(r#"{"a":1}"#.into()),
    };
    let json = serde_json::to_value(&cfc).unwrap();
    let parsed: ChunkFunctionCall = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.name.as_deref(), Some("fn"));
}

#[test]
fn chunk_usage_serde_roundtrip() {
    let u = ChunkUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
    };
    let json = serde_json::to_value(&u).unwrap();
    let parsed: ChunkUsage = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.total_tokens, 15);
}

// =========================================================================
// 14. Tool definitions — canonical ↔ OpenAI
// =========================================================================

#[test]
fn tool_def_to_openai_format() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let oai = dialect::tool_def_to_openai(&canonical);
    assert_eq!(oai.tool_type, "function");
    assert_eq!(oai.function.name, "read_file");
    assert_eq!(oai.function.description, "Read a file");
    assert_eq!(oai.function.parameters, canonical.parameters_schema);
}

#[test]
fn tool_def_from_openai_format() {
    let oai = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = dialect::tool_def_from_openai(&oai);
    assert_eq!(canonical.name, "write_file");
    assert_eq!(canonical.description, "Write a file");
    assert_eq!(canonical.parameters_schema, json!({"type": "object"}));
}

#[test]
fn tool_def_roundtrip_canonical_to_openai_back() {
    let original = CanonicalToolDef {
        name: "search".into(),
        description: "Search codebase".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer"}
            }
        }),
    };
    let oai = dialect::tool_def_to_openai(&original);
    let back = dialect::tool_def_from_openai(&oai);
    assert_eq!(back, original);
}

#[test]
fn tool_def_serde_roundtrip() {
    let td = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
            name: "test".into(),
            description: "A test tool".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json = serde_json::to_value(&td).unwrap();
    assert_eq!(json["type"], "function");
    let parsed: OpenAIToolDef = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, td);
}

#[test]
fn canonical_tool_def_serde_roundtrip() {
    let cd = CanonicalToolDef {
        name: "grep".into(),
        description: "Search files".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let json = serde_json::to_value(&cd).unwrap();
    let parsed: CanonicalToolDef = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, cd);
}

// =========================================================================
// 15. Tool choice serde
// =========================================================================

#[test]
fn tool_choice_mode_none_serde() {
    let tc = ToolChoice::Mode(ToolChoiceMode::None);
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, json!("none"));
    let parsed: ToolChoice = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn tool_choice_mode_auto_serde() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, json!("auto"));
    let parsed: ToolChoice = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn tool_choice_mode_required_serde() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Required);
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json, json!("required"));
    let parsed: ToolChoice = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn tool_choice_function_serde() {
    let tc = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "read_file".into(),
        },
    };
    let json = serde_json::to_value(&tc).unwrap();
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "read_file");
    let parsed: ToolChoice = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn tool_choice_function_ref_serde() {
    let r = ToolChoiceFunctionRef {
        name: "my_func".into(),
    };
    let json = serde_json::to_value(&r).unwrap();
    let parsed: ToolChoiceFunctionRef = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, r);
}

// =========================================================================
// 16. Response format
// =========================================================================

#[test]
fn response_format_text_serde() {
    let rf = ResponseFormat::text();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "text");
    let parsed: ResponseFormat = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, ResponseFormat::Text);
}

#[test]
fn response_format_json_object_serde() {
    let rf = ResponseFormat::json_object();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_object");
    let parsed: ResponseFormat = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, ResponseFormat::JsonObject);
}

#[test]
fn response_format_json_schema_serde() {
    let rf = ResponseFormat::json_schema("my_schema", json!({"type": "object"}));
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_schema");
    assert_eq!(json["json_schema"]["name"], "my_schema");
    let parsed: ResponseFormat = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, rf);
}

#[test]
fn json_schema_spec_serde_roundtrip() {
    let spec = JsonSchemaSpec {
        name: "result".into(),
        description: Some("Output schema".into()),
        schema: json!({"type": "object", "properties": {"ok": {"type": "boolean"}}}),
        strict: Some(true),
    };
    let json = serde_json::to_value(&spec).unwrap();
    let parsed: JsonSchemaSpec = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, spec);
}

#[test]
fn json_schema_spec_without_optional_fields() {
    let spec = JsonSchemaSpec {
        name: "minimal".into(),
        description: None,
        schema: json!({"type": "string"}),
        strict: None,
    };
    let json = serde_json::to_string(&spec).unwrap();
    assert!(!json.contains("description"));
    assert!(!json.contains("strict"));
}

// =========================================================================
// 17. Configuration
// =========================================================================

#[test]
fn config_default_model() {
    let cfg = OpenAIConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
}

#[test]
fn config_default_base_url() {
    let cfg = OpenAIConfig::default();
    assert_eq!(cfg.base_url, "https://api.openai.com/v1");
}

#[test]
fn config_default_max_tokens() {
    let cfg = OpenAIConfig::default();
    assert_eq!(cfg.max_tokens, Some(4096));
}

#[test]
fn config_default_temperature_none() {
    let cfg = OpenAIConfig::default();
    assert!(cfg.temperature.is_none());
}

#[test]
fn config_default_api_key_empty() {
    let cfg = OpenAIConfig::default();
    assert!(cfg.api_key.is_empty());
}

#[test]
fn map_work_order_uses_task_as_user_message() {
    let wo = WorkOrderBuilder::new("Refactor auth").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(
        req.messages[0]
            .content
            .as_deref()
            .unwrap()
            .contains("Refactor auth")
    );
}

#[test]
fn map_work_order_uses_config_model_as_default() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn map_work_order_preserves_temperature() {
    let cfg = OpenAIConfig {
        temperature: Some(0.3),
        ..OpenAIConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.temperature, Some(0.3));
}

#[test]
fn map_work_order_preserves_max_tokens() {
    let cfg = OpenAIConfig {
        max_tokens: Some(8192),
        ..OpenAIConfig::default()
    };
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_tokens, Some(8192));
}

// =========================================================================
// 18. Model name mapping
// =========================================================================

#[test]
fn canonical_model_roundtrip() {
    let canonical = dialect::to_canonical_model("gpt-4o");
    assert_eq!(canonical, "openai/gpt-4o");
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, "gpt-4o");
}

#[test]
fn from_canonical_no_prefix_passthrough() {
    assert_eq!(
        dialect::from_canonical_model("custom-model"),
        "custom-model"
    );
}

#[test]
fn known_models_recognized() {
    assert!(dialect::is_known_model("gpt-4o"));
    assert!(dialect::is_known_model("gpt-4o-mini"));
    assert!(dialect::is_known_model("gpt-4-turbo"));
    assert!(dialect::is_known_model("o1"));
    assert!(dialect::is_known_model("o1-mini"));
    assert!(dialect::is_known_model("o3-mini"));
    assert!(dialect::is_known_model("gpt-4.1"));
}

#[test]
fn unknown_model_not_recognized() {
    assert!(!dialect::is_known_model("not-a-model"));
    assert!(!dialect::is_known_model("claude-sonnet-4"));
    assert!(!dialect::is_known_model(""));
}

// =========================================================================
// 19. Capability manifest
// =========================================================================

#[test]
fn capability_manifest_has_streaming() {
    use abp_core::Capability;
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn capability_manifest_has_structured_output() {
    use abp_core::Capability;
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn capability_manifest_mcp_entries() {
    use abp_core::Capability;
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::McpClient));
    assert!(m.contains_key(&Capability::McpServer));
}

#[test]
fn capability_manifest_tool_entries() {
    use abp_core::Capability;
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&Capability::ToolRead));
    assert!(m.contains_key(&Capability::ToolWrite));
    assert!(m.contains_key(&Capability::ToolBash));
}

#[test]
fn capability_manifest_nonempty() {
    let m = dialect::capability_manifest();
    assert!(!m.is_empty());
}

// =========================================================================
// 20. Validation (extended fields)
// =========================================================================

#[test]
fn validation_clean_fields_pass() {
    let fields = ExtendedRequestFields::default();
    assert!(validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validation_logprobs_fails() {
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "logprobs"));
}

#[test]
fn validation_top_logprobs_fails() {
    let fields = ExtendedRequestFields {
        top_logprobs: Some(5),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "logprobs"));
}

#[test]
fn validation_logit_bias_fails() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("123".into(), 1.0);
    let fields = ExtendedRequestFields {
        logit_bias: Some(bias),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "logit_bias"));
}

#[test]
fn validation_seed_fails() {
    let fields = ExtendedRequestFields {
        seed: Some(42),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "seed"));
}

#[test]
fn validation_multiple_errors() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("1".into(), 0.5);
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        top_logprobs: None,
        logit_bias: Some(bias),
        seed: Some(99),
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert_eq!(err.errors.len(), 3);
}

#[test]
fn validation_logprobs_false_passes() {
    let fields = ExtendedRequestFields {
        logprobs: Some(false),
        ..Default::default()
    };
    assert!(validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validation_empty_logit_bias_passes() {
    let fields = ExtendedRequestFields {
        logit_bias: Some(std::collections::BTreeMap::new()),
        ..Default::default()
    };
    assert!(validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn unmappable_param_display() {
    let p = UnmappableParam {
        param: "logprobs".into(),
        reason: "not supported".into(),
    };
    let s = format!("{p}");
    assert!(s.contains("logprobs"));
    assert!(s.contains("not supported"));
}

#[test]
fn validation_errors_display() {
    let errs = ValidationErrors {
        errors: vec![
            UnmappableParam {
                param: "a".into(),
                reason: "r1".into(),
            },
            UnmappableParam {
                param: "b".into(),
                reason: "r2".into(),
            },
        ],
    };
    let s = format!("{errs}");
    assert!(s.contains("2 unmappable parameter(s)"));
}

#[test]
fn unmappable_param_serde_roundtrip() {
    let p = UnmappableParam {
        param: "seed".into(),
        reason: "not mappable".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    let parsed: UnmappableParam = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, p);
}

#[test]
fn extended_request_fields_serde_roundtrip() {
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        top_logprobs: Some(3),
        logit_bias: None,
        seed: Some(42),
    };
    let json = serde_json::to_value(&fields).unwrap();
    let parsed: ExtendedRequestFields = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.logprobs, Some(true));
    assert_eq!(parsed.seed, Some(42));
}

// =========================================================================
// 21. Dialect version and constants
// =========================================================================

#[test]
fn dialect_version_correct() {
    assert_eq!(dialect::DIALECT_VERSION, "openai/v0.1");
}

#[test]
fn default_model_correct() {
    assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
}

#[test]
fn backend_name_correct() {
    assert_eq!(abp_openai_sdk::BACKEND_NAME, "sidecar:openai");
}

#[test]
fn host_script_relative_correct() {
    assert_eq!(abp_openai_sdk::HOST_SCRIPT_RELATIVE, "hosts/openai/host.js");
}

// =========================================================================
// 22. IR conversation accessor methods via OpenAI lowering
// =========================================================================

#[test]
fn system_message_accessor() {
    let msgs = [msg("system", Some("instructions")), msg("user", Some("hi"))];
    let conv = lowering::to_ir(&msgs);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "instructions");
}

#[test]
fn last_assistant_accessor() {
    let msgs = [
        msg("user", Some("q1")),
        msg("assistant", Some("a1")),
        msg("user", Some("q2")),
        msg("assistant", Some("a2")),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.last_assistant().unwrap().text_content(), "a2");
}

#[test]
fn messages_by_role_filter() {
    let msgs = [
        msg("user", Some("a")),
        msg("assistant", Some("b")),
        msg("user", Some("c")),
        msg("assistant", Some("d")),
        msg("user", Some("e")),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 3);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 2);
}

#[test]
fn tool_calls_across_messages() {
    let msgs = [
        assistant_with_tool_calls(None, vec![make_tool_call("c1", "a", "{}")]),
        tool_result_msg(Some("ok"), "c1"),
        assistant_with_tool_calls(None, vec![make_tool_call("c2", "b", "{}")]),
    ];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn ir_message_is_text_only_check() {
    let msgs = [msg("user", Some("hello"))];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].is_text_only());

    let msgs2 = [assistant_with_tool_calls(
        Some("text"),
        vec![make_tool_call("c1", "fn", "{}")],
    )];
    let conv2 = lowering::to_ir(&msgs2);
    assert!(!conv2.messages[0].is_text_only());
}

// =========================================================================
// 23. Thinking blocks from IR → OpenAI
// =========================================================================

#[test]
fn thinking_block_becomes_text_in_openai() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Let me think...".into(),
            },
            IrContentBlock::Text {
                text: "Here's my answer.".into(),
            },
        ],
    )]);
    let back = lowering::from_ir(&conv);
    // Thinking text is joined with regular text
    let content = back[0].content.as_deref().unwrap();
    assert!(content.contains("Let me think..."));
    assert!(content.contains("Here's my answer."));
}

// =========================================================================
// 24. Config serde
// =========================================================================

#[test]
fn config_serde_roundtrip() {
    let cfg = OpenAIConfig {
        api_key: "sk-test".into(),
        base_url: "https://custom.api/v1".into(),
        model: "gpt-4-turbo".into(),
        max_tokens: Some(8192),
        temperature: Some(0.5),
    };
    let json = serde_json::to_value(&cfg).unwrap();
    let parsed: OpenAIConfig = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.api_key, "sk-test");
    assert_eq!(parsed.base_url, "https://custom.api/v1");
    assert_eq!(parsed.model, "gpt-4-turbo");
    assert_eq!(parsed.max_tokens, Some(8192));
    assert_eq!(parsed.temperature, Some(0.5));
}

// =========================================================================
// 25. Streaming with tool calls delta
// =========================================================================

#[test]
fn chunk_tool_calls_delta_not_emitted_by_map_chunk() {
    let chunk = make_chunk(
        "cmpl-tc",
        ChunkDelta {
            role: Some("assistant".into()),
            content: None,
            tool_calls: Some(vec![ChunkToolCall {
                index: 0,
                id: Some("c1".into()),
                call_type: Some("function".into()),
                function: Some(ChunkFunctionCall {
                    name: Some("test".into()),
                    arguments: Some("{}".into()),
                }),
            }]),
        },
        None,
    );
    // map_chunk only emits text deltas, not tool call fragments
    let events = abp_openai_sdk::streaming::map_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn accumulator_multiple_fragments_over_time() {
    let mut acc = ToolCallAccumulator::new();

    // Fragment 1: start of tool call
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("write_file".into()),
            arguments: Some(r#"{"pa"#.into()),
        }),
    }]);

    // Fragment 2: middle
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#"th":"#.into()),
        }),
    }]);

    // Fragment 3: end
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#""a.rs"}"#.into()),
        }),
    }]);

    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name, input, ..
        } => {
            assert_eq!(tool_name, "write_file");
            assert_eq!(input, &json!({"path": "a.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// =========================================================================
// 26. Sidecar script path
// =========================================================================

#[test]
fn sidecar_script_path() {
    use std::path::Path;
    let root = Path::new("/fake/root");
    let script = abp_openai_sdk::sidecar_script(root);
    assert_eq!(script, root.join("hosts/openai/host.js"));
}

// =========================================================================
// 27. Additional edge cases for from_ir with image blocks
// =========================================================================

#[test]
fn image_block_ignored_in_from_ir() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    )]);
    let back = lowering::from_ir(&conv);
    // Image blocks are not representable in OpenAI text messages; content should be None
    assert!(back[0].content.is_none());
}

#[test]
fn mixed_text_and_image_in_from_ir() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Look at this:".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64".into(),
            },
        ],
    )]);
    let back = lowering::from_ir(&conv);
    // Only text blocks are preserved
    assert_eq!(back[0].content.as_deref(), Some("Look at this:"));
}

// =========================================================================
// 28. IR conversation chain building via OpenAI lowering
// =========================================================================

#[test]
fn conversation_len_and_is_empty() {
    let empty = lowering::to_ir(&[]);
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    let nonempty = lowering::to_ir(&[msg("user", Some("hi"))]);
    assert_eq!(nonempty.len(), 1);
    assert!(!nonempty.is_empty());
}

#[test]
fn last_message_accessor() {
    let msgs = [msg("user", Some("first")), msg("assistant", Some("last"))];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.last_message().unwrap().text_content(), "last");
}
