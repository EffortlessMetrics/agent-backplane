// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for Kimi SDK dialect types, lowering, and serde roundtrips.

use serde_json::json;

use abp_kimi_sdk::dialect::*;
use abp_kimi_sdk::lowering;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, WorkOrderBuilder};

// =========================================================================
// Helpers
// =========================================================================

fn msg(role: &str, content: Option<&str>) -> KimiMessage {
    KimiMessage {
        role: role.into(),
        content: content.map(Into::into),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn tool_result_msg(content: Option<&str>, tool_call_id: &str) -> KimiMessage {
    KimiMessage {
        role: "tool".into(),
        content: content.map(Into::into),
        tool_call_id: Some(tool_call_id.into()),
        tool_calls: None,
    }
}

fn assistant_with_tool_calls(content: Option<&str>, calls: Vec<KimiToolCall>) -> KimiMessage {
    KimiMessage {
        role: "assistant".into(),
        content: content.map(Into::into),
        tool_call_id: None,
        tool_calls: Some(calls),
    }
}

fn make_tool_call(id: &str, name: &str, args: &str) -> KimiToolCall {
    KimiToolCall {
        id: id.into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
    }
}

fn simple_response(text: &str) -> KimiResponse {
    KimiResponse {
        id: "cmpl-test".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some(text.into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    }
}

fn make_chunk(id: &str, delta: KimiChunkDelta, finish_reason: Option<&str>) -> KimiChunk {
    KimiChunk {
        id: id.into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta,
            finish_reason: finish_reason.map(Into::into),
        }],
        usage: None,
        refs: None,
    }
}

// =========================================================================
// 1. Roles — to_ir mapping
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
// 2. Roles — from_ir mapping
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
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => {
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
    let msgs: Vec<KimiMessage> = (0..50)
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
// 8. KimiRequest construction & serde
// =========================================================================

#[test]
fn request_serde_roundtrip_minimal() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![msg("user", Some("Hi"))],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    let json_str = serde_json::to_string(&req).unwrap();
    let parsed: KimiRequest = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.model, "moonshot-v1-8k");
    assert_eq!(parsed.messages.len(), 1);
    assert!(parsed.tools.is_none());
}

#[test]
fn request_serde_roundtrip_all_fields() {
    let req = KimiRequest {
        model: "moonshot-v1-128k".into(),
        messages: vec![msg("system", Some("Be brief")), msg("user", Some("Hello"))],
        max_tokens: Some(4096),
        temperature: Some(0.7),
        stream: Some(true),
        tools: Some(vec![KimiTool::Function {
            function: KimiFunctionDef {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
        }]),
        use_search: Some(true),
    };
    let json_str = serde_json::to_string(&req).unwrap();
    let parsed: KimiRequest = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.model, "moonshot-v1-128k");
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.tools.as_ref().unwrap().len(), 1);
    assert_eq!(parsed.temperature, Some(0.7));
    assert_eq!(parsed.max_tokens, Some(4096));
    assert_eq!(parsed.stream, Some(true));
}

#[test]
fn request_skip_serializing_none_fields() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![msg("user", Some("Hi"))],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    let json_str = serde_json::to_string(&req).unwrap();
    assert!(!json_str.contains("max_tokens"));
    assert!(!json_str.contains("temperature"));
    assert!(!json_str.contains("stream"));
    assert!(!json_str.contains("tools"));
    assert!(!json_str.contains("use_search"));
}

#[test]
fn request_with_tools_includes_tools_field() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: Some(vec![]),
        use_search: None,
    };
    let json_str = serde_json::to_string(&req).unwrap();
    assert!(json_str.contains("tools"));
}

// =========================================================================
// 9. KimiResponse parsing & serde
// =========================================================================

#[test]
fn response_serde_roundtrip() {
    let resp = KimiResponse {
        id: "cmpl-abc".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        refs: None,
    };
    let json_str = serde_json::to_string(&resp).unwrap();
    let parsed: KimiResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.id, "cmpl-abc");
    assert_eq!(parsed.choices.len(), 1);
    assert_eq!(parsed.usage.as_ref().unwrap().total_tokens, 15);
}

#[test]
fn response_without_usage() {
    let resp = simple_response("Hi");
    let json_str = serde_json::to_string(&resp).unwrap();
    let parsed: KimiResponse = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.usage.is_none());
}

#[test]
fn response_multiple_choices() {
    let resp = KimiResponse {
        id: "cmpl-multi".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![
            KimiChoice {
                index: 0,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Option A".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            },
            KimiChoice {
                index: 1,
                message: KimiResponseMessage {
                    role: "assistant".into(),
                    content: Some("Option B".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            },
        ],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
}

#[test]
fn response_with_tool_calls_produces_tool_call_events() {
    let resp = KimiResponse {
        id: "cmpl-tc".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_abc".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"query":"rust async"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "web_search");
            assert_eq!(tool_use_id.as_deref(), Some("call_abc"));
            assert_eq!(input, &json!({"query": "rust async"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn response_text_and_tool_calls_produce_multiple_events() {
    let resp = KimiResponse {
        id: "cmpl-mixed".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("I'll check.".into()),
                tool_calls: Some(vec![KimiToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "ls".into(),
                        arguments: "{}".into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn response_empty_text_not_emitted() {
    let resp = KimiResponse {
        id: "cmpl-empty".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn response_none_content_no_events() {
    let resp = KimiResponse {
        id: "cmpl-none".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn response_malformed_tool_args_in_event() {
    let resp = KimiResponse {
        id: "cmpl-bad".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "fn".into(),
                        arguments: "bad-json".into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
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
        KimiChunkDelta {
            role: Some("assistant".into()),
            content: Some("Hello".into()),
            tool_calls: None,
        },
        None,
    );
    let events = map_stream_event(&chunk);
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
        KimiChunkDelta {
            role: Some("assistant".into()),
            content: Some("".into()),
            tool_calls: None,
        },
        None,
    );
    let events = map_stream_event(&chunk);
    assert!(events.is_empty());
}

#[test]
fn chunk_none_content_no_events() {
    let chunk = make_chunk(
        "cmpl-3",
        KimiChunkDelta {
            role: Some("assistant".into()),
            content: None,
            tool_calls: None,
        },
        None,
    );
    let events = map_stream_event(&chunk);
    assert!(events.is_empty());
}

#[test]
fn chunk_with_finish_reason_stop() {
    let chunk = make_chunk(
        "cmpl-4",
        KimiChunkDelta {
            role: None,
            content: None,
            tool_calls: None,
        },
        Some("stop"),
    );
    let events = map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunCompleted { message } => {
            assert!(message.contains("stop"));
        }
        other => panic!("expected RunCompleted, got {other:?}"),
    }
}

#[test]
fn chunk_with_finish_reason_tool_calls() {
    let chunk = make_chunk("cmpl-5", KimiChunkDelta::default(), Some("tool_calls"));
    let events = map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::RunCompleted { message } => {
            assert!(message.contains("tool_calls"));
        }
        other => panic!("expected RunCompleted, got {other:?}"),
    }
}

#[test]
fn chunk_text_and_finish_reason_produces_two_events() {
    let chunk = make_chunk(
        "cmpl-6",
        KimiChunkDelta {
            role: None,
            content: Some("final word".into()),
            tool_calls: None,
        },
        Some("stop"),
    );
    let events = map_stream_event(&chunk);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn chunk_serde_roundtrip() {
    let chunk = KimiChunk {
        id: "cmpl-serde".into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: Some("assistant".into()),
                content: Some("hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let json_str = serde_json::to_string(&chunk).unwrap();
    let parsed: KimiChunk = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.id, "cmpl-serde");
    assert_eq!(parsed.choices[0].delta.content.as_deref(), Some("hi"));
}

// =========================================================================
// 11. ToolCallAccumulator
// =========================================================================

#[test]
fn accumulator_single_tool_call() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"rust"}"#.into()),
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
            assert_eq!(tool_name, "search");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            assert_eq!(input, &json!({"q": "rust"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_incremental_arguments() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_2".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("edit".into()),
            arguments: Some(r#"{"pa"#.into()),
        }),
    }]);
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(KimiChunkFunctionCall {
            name: None,
            arguments: Some(r#"th":"main"#.into()),
        }),
    }]);
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(KimiChunkFunctionCall {
            name: None,
            arguments: Some(r#".rs"}"#.into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &json!({"path": "main.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_multiple_tool_calls() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        KimiChunkToolCall {
            index: 0,
            id: Some("c1".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("read".into()),
                arguments: Some("{}".into()),
            }),
        },
        KimiChunkToolCall {
            index: 1,
            id: Some("c2".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("write".into()),
                arguments: Some("{}".into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn accumulator_empty_name_filtered_out() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: None,
    }]);
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn accumulator_malformed_arguments() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("bad".into()),
            arguments: Some("not json".into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { input, .. } => {
            assert_eq!(input, &json!("not json"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_default_is_empty() {
    let acc = ToolCallAccumulator::default();
    let events = acc.finish();
    assert!(events.is_empty());
}

// =========================================================================
// 12. Configuration (KimiConfig)
// =========================================================================

#[test]
fn default_config_has_sensible_values() {
    let cfg = KimiConfig::default();
    assert!(cfg.base_url.contains("moonshot.cn"));
    assert_eq!(cfg.model, "moonshot-v1-8k");
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.api_key.is_empty());
    assert!(cfg.temperature.is_none());
    assert!(cfg.use_k1_reasoning.is_none());
}

#[test]
fn config_serde_roundtrip() {
    let cfg = KimiConfig {
        api_key: "test-key".into(),
        base_url: "https://custom.api.com/v1".into(),
        model: "kimi-latest".into(),
        max_tokens: Some(8192),
        temperature: Some(0.5),
        use_k1_reasoning: Some(true),
    };
    let json_str = serde_json::to_string(&cfg).unwrap();
    let parsed: KimiConfig = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.api_key, "test-key");
    assert_eq!(parsed.base_url, "https://custom.api.com/v1");
    assert_eq!(parsed.model, "kimi-latest");
    assert_eq!(parsed.max_tokens, Some(8192));
    assert_eq!(parsed.temperature, Some(0.5));
    assert_eq!(parsed.use_k1_reasoning, Some(true));
}

#[test]
fn config_k1_reasoning_none_by_default() {
    let cfg = KimiConfig::default();
    let json_str = serde_json::to_string(&cfg).unwrap();
    assert!(!json_str.contains("use_k1_reasoning"));
}

// =========================================================================
// 13. map_work_order
// =========================================================================

#[test]
fn map_work_order_uses_task_as_user_message() {
    let wo = WorkOrderBuilder::new("Optimize database queries").build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(
        req.messages[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("Optimize database queries")
    );
}

#[test]
fn map_work_order_uses_default_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-8k");
}

#[test]
fn map_work_order_respects_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("moonshot-v1-128k")
        .build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "moonshot-v1-128k");
}

#[test]
fn map_work_order_applies_max_tokens() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig {
        max_tokens: Some(2048),
        ..KimiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.max_tokens, Some(2048));
}

#[test]
fn map_work_order_applies_temperature() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig {
        temperature: Some(0.3),
        ..KimiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.temperature, Some(0.3));
}

#[test]
fn map_work_order_use_search_enabled() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig {
        use_k1_reasoning: Some(true),
        ..KimiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.use_search, Some(true));
}

#[test]
fn map_work_order_use_search_disabled() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig {
        use_k1_reasoning: Some(false),
        ..KimiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert!(req.use_search.is_none());
}

#[test]
fn map_work_order_stream_is_none() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(req.stream.is_none());
}

#[test]
fn map_work_order_tools_is_none() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = KimiConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(req.tools.is_none());
}

// =========================================================================
// 14. Model name mapping
// =========================================================================

#[test]
fn canonical_model_adds_prefix() {
    assert_eq!(
        to_canonical_model("moonshot-v1-8k"),
        "moonshot/moonshot-v1-8k"
    );
}

#[test]
fn canonical_model_with_kimi_latest() {
    assert_eq!(to_canonical_model("kimi-latest"), "moonshot/kimi-latest");
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        from_canonical_model("moonshot/moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
}

#[test]
fn from_canonical_model_no_prefix_passthrough() {
    assert_eq!(from_canonical_model("gpt-4o"), "gpt-4o");
}

#[test]
fn from_canonical_roundtrip() {
    let model = "moonshot-v1-128k";
    assert_eq!(from_canonical_model(&to_canonical_model(model)), model);
}

#[test]
fn is_known_model_positive() {
    assert!(is_known_model("moonshot-v1-8k"));
    assert!(is_known_model("moonshot-v1-32k"));
    assert!(is_known_model("moonshot-v1-128k"));
    assert!(is_known_model("kimi-latest"));
    assert!(is_known_model("k1"));
}

#[test]
fn is_known_model_negative() {
    assert!(!is_known_model("gpt-4o"));
    assert!(!is_known_model("claude-3-opus"));
    assert!(!is_known_model(""));
}

// =========================================================================
// 15. Capability manifest
// =========================================================================

#[test]
fn capability_manifest_has_streaming() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(m.contains_key(&Capability::Streaming));
}

#[test]
fn capability_manifest_has_web_search() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(m.contains_key(&Capability::ToolWebSearch));
}

#[test]
fn capability_manifest_tool_edit_present() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(m.contains_key(&Capability::ToolEdit));
}

#[test]
fn capability_manifest_tool_bash_present() {
    use abp_core::Capability;
    let m = capability_manifest();
    assert!(m.contains_key(&Capability::ToolBash));
}

// =========================================================================
// 16. Tool definition conversion
// =========================================================================

#[test]
fn tool_def_to_kimi_conversion() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let kimi = tool_def_to_kimi(&canonical);
    assert_eq!(kimi.tool_type, "function");
    assert_eq!(kimi.function.name, "read_file");
    assert_eq!(kimi.function.description, "Read a file");
    assert_eq!(kimi.function.parameters, canonical.parameters_schema);
}

#[test]
fn tool_def_from_kimi_conversion() {
    let kimi = KimiToolDef {
        tool_type: "function".into(),
        function: KimiFunctionDef {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let canonical = tool_def_from_kimi(&kimi);
    assert_eq!(canonical.name, "write_file");
    assert_eq!(canonical.description, "Write a file");
    assert_eq!(canonical.parameters_schema, json!({"type": "object"}));
}

#[test]
fn tool_def_roundtrip() {
    let original = CanonicalToolDef {
        name: "exec".into(),
        description: "Execute command".into(),
        parameters_schema: json!({"type": "object", "properties": {"cmd": {"type": "string"}}, "required": ["cmd"]}),
    };
    let back = tool_def_from_kimi(&tool_def_to_kimi(&original));
    assert_eq!(back, original);
}

// =========================================================================
// 17. Built-in tools
// =========================================================================

#[test]
fn builtin_search_internet_type() {
    let tool = builtin_search_internet();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$web_search");
}

#[test]
fn builtin_browser_type() {
    let tool = builtin_browser();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$browser");
}

#[test]
fn builtin_search_serde_roundtrip() {
    let tool = builtin_search_internet();
    let json_str = serde_json::to_string(&tool).unwrap();
    let parsed: KimiBuiltinTool = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn builtin_browser_serde_roundtrip() {
    let tool = builtin_browser();
    let json_str = serde_json::to_string(&tool).unwrap();
    let parsed: KimiBuiltinTool = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, tool);
}

// =========================================================================
// 18. Citation refs
// =========================================================================

#[test]
fn ref_serde_roundtrip() {
    let r = KimiRef {
        index: 1,
        url: "https://example.com".into(),
        title: Some("Example".into()),
    };
    let json_str = serde_json::to_string(&r).unwrap();
    let parsed: KimiRef = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.index, 1);
    assert_eq!(parsed.url, "https://example.com");
    assert_eq!(parsed.title.as_deref(), Some("Example"));
}

#[test]
fn ref_without_title() {
    let r = KimiRef {
        index: 2,
        url: "https://other.com".into(),
        title: None,
    };
    let json_str = serde_json::to_string(&r).unwrap();
    assert!(!json_str.contains("title"));
    let parsed: KimiRef = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.title.is_none());
}

#[test]
fn response_with_refs_attaches_ext_metadata() {
    let resp = KimiResponse {
        id: "cmpl-ref".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Here are results.".into()),
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
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("kimi_refs"));
}

#[test]
fn chunk_with_refs_attaches_ext_metadata() {
    let chunk = KimiChunk {
        id: "cmpl-ref".into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("Results:".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: None,
        }]),
    };
    let events = map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("kimi_refs"));
}

// =========================================================================
// 19. Usage handling
// =========================================================================

#[test]
fn usage_to_ir_computes_total() {
    let usage = KimiUsage {
        prompt_tokens: 200,
        completion_tokens: 80,
        total_tokens: 280,
    };
    let ir = lowering::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 200);
    assert_eq!(ir.output_tokens, 80);
    assert_eq!(ir.total_tokens, 280);
}

#[test]
fn usage_to_ir_zero_values() {
    let usage = KimiUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    };
    let ir = lowering::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 0);
    assert_eq!(ir.output_tokens, 0);
    assert_eq!(ir.total_tokens, 0);
}

#[test]
fn extract_usage_present() {
    let resp = KimiResponse {
        id: "cmpl-usage".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: Some(KimiUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
        refs: None,
    };
    let u = extract_usage(&resp).unwrap();
    assert_eq!(u["prompt_tokens"], json!(100));
    assert_eq!(u["completion_tokens"], json!(50));
    assert_eq!(u["total_tokens"], json!(150));
}

#[test]
fn extract_usage_absent() {
    let resp = simple_response("Hi");
    assert!(extract_usage(&resp).is_none());
}

// =========================================================================
// 20. KimiRole enum
// =========================================================================

#[test]
fn kimi_role_display() {
    assert_eq!(KimiRole::System.to_string(), "system");
    assert_eq!(KimiRole::User.to_string(), "user");
    assert_eq!(KimiRole::Assistant.to_string(), "assistant");
    assert_eq!(KimiRole::Tool.to_string(), "tool");
}

#[test]
fn kimi_role_serde_roundtrip() {
    let roles = [
        KimiRole::System,
        KimiRole::User,
        KimiRole::Assistant,
        KimiRole::Tool,
    ];
    for role in &roles {
        let json_str = serde_json::to_string(role).unwrap();
        let parsed: KimiRole = serde_json::from_str(&json_str).unwrap();
        assert_eq!(&parsed, role);
    }
}

// =========================================================================
// 21. KimiTool enum serde
// =========================================================================

#[test]
fn kimi_tool_function_serde() {
    let tool = KimiTool::Function {
        function: KimiFunctionDef {
            name: "search".into(),
            description: "Search things".into(),
            parameters: json!({"type": "object"}),
        },
    };
    let json_str = serde_json::to_string(&tool).unwrap();
    assert!(json_str.contains(r#""type":"function"#));
    let parsed: KimiTool = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn kimi_tool_builtin_serde() {
    let tool = KimiTool::BuiltinFunction {
        function: KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    };
    let json_str = serde_json::to_string(&tool).unwrap();
    assert!(json_str.contains(r#""type":"builtin_function"#));
    let parsed: KimiTool = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, tool);
}

// =========================================================================
// 22. KimiMessage serde
// =========================================================================

#[test]
fn kimi_message_skip_none_fields() {
    let m = msg("user", Some("Hello"));
    let json_str = serde_json::to_string(&m).unwrap();
    assert!(!json_str.contains("tool_call_id"));
    assert!(!json_str.contains("tool_calls"));
}

#[test]
fn kimi_message_with_all_fields() {
    let m = KimiMessage {
        role: "assistant".into(),
        content: Some("text".into()),
        tool_call_id: None,
        tool_calls: Some(vec![make_tool_call("c1", "fn", "{}")]),
    };
    let json_str = serde_json::to_string(&m).unwrap();
    assert!(json_str.contains("tool_calls"));
    assert!(json_str.contains("\"content\""));
}

// =========================================================================
// 23. Thinking blocks pass through from_ir
// =========================================================================

#[test]
fn thinking_block_becomes_text_in_from_ir() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "Let me think...".into(),
        }],
    )]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Let me think..."));
}

#[test]
fn mixed_thinking_and_text_in_from_ir() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Hmm ".into(),
            },
            IrContentBlock::Text {
                text: "Answer".into(),
            },
        ],
    )]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Hmm Answer"));
}

// =========================================================================
// 24. Image blocks ignored in from_ir
// =========================================================================

#[test]
fn image_block_ignored_in_from_ir() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "See this:".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    )]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("See this:"));
    assert!(back[0].tool_calls.is_none());
}

// =========================================================================
// 25. Chunk streaming types
// =========================================================================

#[test]
fn chunk_delta_default() {
    let delta = KimiChunkDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn chunk_tool_call_fragment_serde() {
    let frag = KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"rust"}"#.into()),
        }),
    };
    let json_str = serde_json::to_string(&frag).unwrap();
    let parsed: KimiChunkToolCall = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, frag);
}

#[test]
fn chunk_function_call_partial_serde() {
    let frag = KimiChunkFunctionCall {
        name: None,
        arguments: Some(r#"partial"#.into()),
    };
    let json_str = serde_json::to_string(&frag).unwrap();
    assert!(!json_str.contains("name"));
    let parsed: KimiChunkFunctionCall = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, frag);
}

#[test]
fn chunk_with_usage() {
    let chunk = KimiChunk {
        id: "cmpl-final".into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 50,
            completion_tokens: 30,
            total_tokens: 80,
        }),
        refs: None,
    };
    let json_str = serde_json::to_string(&chunk).unwrap();
    let parsed: KimiChunk = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.usage.as_ref().unwrap().total_tokens, 80);
}

// =========================================================================
// 26. Constants
// =========================================================================

#[test]
fn dialect_version_correct() {
    assert_eq!(DIALECT_VERSION, "kimi/v0.1");
}

#[test]
fn default_model_correct() {
    assert_eq!(DEFAULT_MODEL, "moonshot-v1-8k");
}

// =========================================================================
// 27. Conversation-level IR helpers
// =========================================================================

#[test]
fn ir_conversation_system_message_extracted() {
    let msgs = [msg("system", Some("sys prompt")), msg("user", Some("hi"))];
    let conv = lowering::to_ir(&msgs);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "sys prompt");
}

#[test]
fn ir_conversation_last_assistant() {
    let msgs = [
        msg("user", Some("hi")),
        msg("assistant", Some("hello")),
        msg("user", Some("bye")),
        msg("assistant", Some("goodbye")),
    ];
    let conv = lowering::to_ir(&msgs);
    let last = conv.last_assistant().unwrap();
    assert_eq!(last.text_content(), "goodbye");
}

#[test]
fn ir_conversation_tool_calls_collected() {
    let msgs = [
        msg("user", Some("do stuff")),
        assistant_with_tool_calls(
            None,
            vec![
                make_tool_call("c1", "a", "{}"),
                make_tool_call("c2", "b", "{}"),
            ],
        ),
        tool_result_msg(Some("ok"), "c1"),
        assistant_with_tool_calls(None, vec![make_tool_call("c3", "c", "{}")]),
    ];
    let conv = lowering::to_ir(&msgs);
    let tool_calls = conv.tool_calls();
    assert_eq!(tool_calls.len(), 3);
}

// =========================================================================
// 28. KimiUsage serde
// =========================================================================

#[test]
fn usage_serde_roundtrip() {
    let usage = KimiUsage {
        prompt_tokens: 42,
        completion_tokens: 17,
        total_tokens: 59,
    };
    let json_str = serde_json::to_string(&usage).unwrap();
    let parsed: KimiUsage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed, usage);
}

// =========================================================================
// 29. Response message serde
// =========================================================================

#[test]
fn response_message_serde_roundtrip() {
    let rm = KimiResponseMessage {
        role: "assistant".into(),
        content: Some("text".into()),
        tool_calls: Some(vec![KimiToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "fn".into(),
                arguments: "{}".into(),
            },
        }]),
    };
    let json_str = serde_json::to_string(&rm).unwrap();
    let parsed: KimiResponseMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.role, "assistant");
    assert_eq!(parsed.content.as_deref(), Some("text"));
    assert_eq!(parsed.tool_calls.as_ref().unwrap().len(), 1);
}

// =========================================================================
// 30. Tool result from_ir with multiple text blocks
// =========================================================================

#[test]
fn tool_result_from_ir_joins_multiple_text_blocks() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![
                IrContentBlock::Text {
                    text: "part1".into(),
                },
                IrContentBlock::Text {
                    text: "part2".into(),
                },
            ],
            is_error: false,
        }],
    )]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("part1part2"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("c1"));
}

#[test]
fn tool_result_from_ir_empty_content() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![],
            is_error: false,
        }],
    )]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some(""));
    assert_eq!(back[0].role, "tool");
}
