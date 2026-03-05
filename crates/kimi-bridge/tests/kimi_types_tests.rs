// SPDX-License-Identifier: MIT OR Apache-2.0
//! Serde round-trip and edge-case tests for `kimi_types`.

use kimi_bridge::kimi_types::*;
use serde_json::{Value, json};

// ── Helper builders ─────────────────────────────────────────────────────

fn user_message(text: &str) -> Message {
    Message {
        role: Role::User,
        content: Some(text.into()),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn assistant_message(text: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: Some(text.into()),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn tool_result_message(call_id: &str, text: &str) -> Message {
    Message {
        role: Role::Tool,
        content: Some(text.into()),
        tool_call_id: Some(call_id.into()),
        tool_calls: None,
    }
}

fn simple_request() -> KimiRequest {
    KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![user_message("Hello")],
        max_tokens: Some(1024),
        temperature: Some(0.7),
        stream: None,
        tools: None,
        use_search: None,
    }
}

fn simple_response() -> KimiResponse {
    KimiResponse {
        id: "cmpl-001".into(),
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

// ── Role serde ──────────────────────────────────────────────────────────

#[test]
fn role_serialize_roundtrip() {
    for (role, expected) in [
        (Role::System, "\"system\""),
        (Role::User, "\"user\""),
        (Role::Assistant, "\"assistant\""),
        (Role::Tool, "\"tool\""),
    ] {
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, expected);
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }
}

// ── Message serde ───────────────────────────────────────────────────────

#[test]
fn user_message_roundtrip() {
    let msg = user_message("Hello world");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, Role::User);
    assert_eq!(back.content.as_deref(), Some("Hello world"));
    assert!(back.tool_call_id.is_none());
    assert!(back.tool_calls.is_none());
}

#[test]
fn tool_result_message_roundtrip() {
    let msg = tool_result_message("call_123", "result data");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, Role::Tool);
    assert_eq!(back.content.as_deref(), Some("result data"));
    assert_eq!(back.tool_call_id.as_deref(), Some("call_123"));
}

#[test]
fn assistant_message_with_tool_calls_roundtrip() {
    let msg = Message {
        role: Role::Assistant,
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_abc".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"city":"Tokyo"}"#.into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert!(back.content.is_none());
    let tcs = back.tool_calls.unwrap();
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].id, "call_abc");
    assert_eq!(tcs[0].function.name, "get_weather");
    assert!(tcs[0].function.arguments.contains("Tokyo"));
}

#[test]
fn message_optional_fields_omitted_when_none() {
    let msg = user_message("test");
    let v: Value = serde_json::to_value(&msg).unwrap();
    assert!(!v.as_object().unwrap().contains_key("tool_call_id"));
    assert!(!v.as_object().unwrap().contains_key("tool_calls"));
}

// ── ToolDefinition serde ────────────────────────────────────────────────

#[test]
fn function_tool_definition_roundtrip() {
    let tool = ToolDefinition::Function {
        function: FunctionDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn builtin_tool_definition_roundtrip() {
    let tool = ToolDefinition::BuiltinFunction {
        function: BuiltinFunctionDef {
            name: "$web_search".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn tool_definition_tag_field_is_type() {
    let tool = ToolDefinition::Function {
        function: FunctionDefinition {
            name: "f".into(),
            description: "d".into(),
            parameters: json!({}),
        },
    };
    let v: Value = serde_json::to_value(&tool).unwrap();
    assert_eq!(v["type"], "function");
}

#[test]
fn builtin_tool_definition_tag_field() {
    let tool = ToolDefinition::BuiltinFunction {
        function: BuiltinFunctionDef {
            name: "$browser".into(),
        },
    };
    let v: Value = serde_json::to_value(&tool).unwrap();
    assert_eq!(v["type"], "builtin_function");
}

// ── ToolCall serde ──────────────────────────────────────────────────────

#[test]
fn tool_call_roundtrip() {
    let tc = ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"rust"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
}

#[test]
fn tool_call_type_field_renamed() {
    let tc = ToolCall {
        id: "c".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "f".into(),
            arguments: "{}".into(),
        },
    };
    let v: Value = serde_json::to_value(&tc).unwrap();
    assert!(v.get("type").is_some());
    assert!(v.get("call_type").is_none());
}

// ── KimiRequest serde ───────────────────────────────────────────────────

#[test]
fn request_minimal_roundtrip() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![user_message("Hi")],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "moonshot-v1-8k");
    assert_eq!(back.messages.len(), 1);
    assert!(back.max_tokens.is_none());
}

#[test]
fn request_full_roundtrip() {
    let req = simple_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.max_tokens, Some(1024));
    assert_eq!(back.temperature, Some(0.7));
}

#[test]
fn request_optional_fields_omitted() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![user_message("Hi")],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    let v: Value = serde_json::to_value(&req).unwrap();
    let obj = v.as_object().unwrap();
    assert!(!obj.contains_key("max_tokens"));
    assert!(!obj.contains_key("temperature"));
    assert!(!obj.contains_key("stream"));
    assert!(!obj.contains_key("tools"));
    assert!(!obj.contains_key("use_search"));
}

#[test]
fn request_with_tools_roundtrip() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![user_message("search something")],
        max_tokens: None,
        temperature: None,
        stream: Some(true),
        tools: Some(vec![
            ToolDefinition::Function {
                function: FunctionDefinition {
                    name: "get_weather".into(),
                    description: "Get weather".into(),
                    parameters: json!({"type": "object"}),
                },
            },
            ToolDefinition::BuiltinFunction {
                function: BuiltinFunctionDef {
                    name: "$web_search".into(),
                },
            },
        ]),
        use_search: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.stream, Some(true));
    assert_eq!(back.use_search, Some(true));
    let tools = back.tools.unwrap();
    assert_eq!(tools.len(), 2);
}

// ── KimiResponse serde ──────────────────────────────────────────────────

#[test]
fn response_roundtrip() {
    let resp = simple_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "cmpl-001");
    assert_eq!(back.model, "moonshot-v1-8k");
    assert_eq!(back.choices.len(), 1);
    assert_eq!(
        back.choices[0].message.content.as_deref(),
        Some("Hi there!")
    );
    assert_eq!(back.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn response_with_tool_calls_roundtrip() {
    let resp = KimiResponse {
        id: "cmpl-tc".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![Choice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_xyz".into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: "search".into(),
                        arguments: r#"{"q":"hello"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiResponse = serde_json::from_str(&json).unwrap();
    let tcs = back.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_xyz");
}

#[test]
fn response_with_refs_roundtrip() {
    let resp = KimiResponse {
        id: "cmpl-ref".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![Choice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".into(),
                content: Some("See [1]".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: Some(vec![
            KimiRef {
                index: 1,
                url: "https://example.com".into(),
                title: Some("Example".into()),
            },
            KimiRef {
                index: 2,
                url: "https://other.com".into(),
                title: None,
            },
        ]),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiResponse = serde_json::from_str(&json).unwrap();
    let refs = back.refs.unwrap();
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].index, 1);
    assert_eq!(refs[0].url, "https://example.com");
    assert_eq!(refs[0].title.as_deref(), Some("Example"));
    assert!(refs[1].title.is_none());
}

// ── Usage serde ─────────────────────────────────────────────────────────

#[test]
fn usage_roundtrip() {
    let u = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn usage_default_is_zero() {
    let u = Usage::default();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

// ── KimiRef serde ───────────────────────────────────────────────────────

#[test]
fn kimi_ref_roundtrip() {
    let r = KimiRef {
        index: 1,
        url: "https://example.com".into(),
        title: Some("Title".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r);
}

#[test]
fn kimi_ref_title_omitted_when_none() {
    let r = KimiRef {
        index: 1,
        url: "https://example.com".into(),
        title: None,
    };
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(!v.as_object().unwrap().contains_key("title"));
}

// ── StreamChunk serde ───────────────────────────────────────────────────

#[test]
fn stream_chunk_text_delta_roundtrip() {
    let chunk = StreamChunk {
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
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back, chunk);
}

#[test]
fn stream_chunk_role_delta_roundtrip() {
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
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back.choices[0].delta.role.as_deref(), Some("assistant"));
}

#[test]
fn stream_chunk_tool_call_delta_roundtrip() {
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
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    let tc = &back.choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id.as_deref(), Some("call_1"));
    assert_eq!(tc.call_type.as_deref(), Some("function"));
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("search")
    );
}

#[test]
fn stream_chunk_final_with_usage_roundtrip() {
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
        usage: Some(Usage {
            prompt_tokens: 50,
            completion_tokens: 25,
            total_tokens: 75,
        }),
        refs: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(back.choices[0].finish_reason.as_deref(), Some("stop"));
    let u = back.usage.unwrap();
    assert_eq!(u.prompt_tokens, 50);
    assert_eq!(u.total_tokens, 75);
}

#[test]
fn stream_delta_default_all_none() {
    let d = StreamDelta::default();
    assert!(d.role.is_none());
    assert!(d.content.is_none());
    assert!(d.tool_calls.is_none());
}

#[test]
fn stream_tool_call_type_field_renamed() {
    let tc = StreamToolCall {
        index: 0,
        id: None,
        call_type: Some("function".into()),
        function: None,
    };
    let v: Value = serde_json::to_value(&tc).unwrap();
    assert!(v.get("type").is_some());
    assert!(v.get("call_type").is_none());
}

// ── Builtin constants ───────────────────────────────────────────────────

#[test]
fn builtin_constants_values() {
    assert_eq!(builtin::WEB_SEARCH, "$web_search");
    assert_eq!(builtin::FILE_TOOL, "$file_tool");
    assert_eq!(builtin::CODE_TOOL, "$code_tool");
    assert_eq!(builtin::BROWSER, "$browser");
}

#[test]
fn builtin_is_builtin_recognizes_all() {
    assert!(builtin::is_builtin("$web_search"));
    assert!(builtin::is_builtin("$file_tool"));
    assert!(builtin::is_builtin("$code_tool"));
    assert!(builtin::is_builtin("$browser"));
}

#[test]
fn builtin_is_builtin_rejects_non_builtins() {
    assert!(!builtin::is_builtin("web_search"));
    assert!(!builtin::is_builtin("$unknown"));
    assert!(!builtin::is_builtin(""));
    assert!(!builtin::is_builtin("get_weather"));
}

// ── JSON wire format ────────────────────────────────────────────────────

#[test]
fn request_deserializes_from_wire_format() {
    let wire = json!({
        "model": "moonshot-v1-128k",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hello"}
        ],
        "max_tokens": 2048,
        "temperature": 0.5,
        "stream": true,
        "use_search": true
    });
    let req: KimiRequest = serde_json::from_value(wire).unwrap();
    assert_eq!(req.model, "moonshot-v1-128k");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, Role::System);
    assert_eq!(req.max_tokens, Some(2048));
    assert_eq!(req.stream, Some(true));
    assert_eq!(req.use_search, Some(true));
}

#[test]
fn response_deserializes_from_wire_format() {
    let wire = json!({
        "id": "cmpl-wire",
        "model": "moonshot-v1-8k",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 5,
            "completion_tokens": 3,
            "total_tokens": 8
        }
    });
    let resp: KimiResponse = serde_json::from_value(wire).unwrap();
    assert_eq!(resp.id, "cmpl-wire");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.usage.unwrap().total_tokens, 8);
}

#[test]
fn stream_chunk_deserializes_from_wire_format() {
    let wire = json!({
        "id": "chatcmpl-xxx",
        "object": "chat.completion.chunk",
        "created": 1700000000,
        "model": "moonshot-v1-8k",
        "choices": [{
            "index": 0,
            "delta": {"content": "Hi"},
            "finish_reason": null
        }]
    });
    let chunk: StreamChunk = serde_json::from_value(wire).unwrap();
    assert_eq!(chunk.id, "chatcmpl-xxx");
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hi"));
    assert!(chunk.choices[0].finish_reason.is_none());
}

// ── Multi-choice response ───────────────────────────────────────────────

#[test]
fn response_multiple_choices() {
    let resp = KimiResponse {
        id: "multi".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![
            Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: Some("Answer A".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            },
            Choice {
                index: 1,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: Some("Answer B".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            },
        ],
        usage: None,
        refs: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.choices.len(), 2);
    assert_eq!(back.choices[1].index, 1);
    assert_eq!(back.choices[1].message.content.as_deref(), Some("Answer B"));
}

// ── Edge cases ──────────────────────────────────────────────────────────

#[test]
fn message_empty_content() {
    let msg = Message {
        role: Role::User,
        content: Some("".into()),
        tool_call_id: None,
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content.as_deref(), Some(""));
}

#[test]
fn message_null_content_deserializes_as_none() {
    let wire = json!({"role": "user", "content": null});
    let msg: Message = serde_json::from_value(wire).unwrap();
    assert!(msg.content.is_none());
}

#[test]
fn function_call_empty_arguments() {
    let fc = FunctionCall {
        name: "noop".into(),
        arguments: "".into(),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let back: FunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back.arguments, "");
}

#[test]
fn multi_turn_conversation_serialization() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![
            user_message("What is 2+2?"),
            assistant_message("4"),
            user_message("And 3+3?"),
        ],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.messages.len(), 3);
    assert_eq!(back.messages[0].role, Role::User);
    assert_eq!(back.messages[1].role, Role::Assistant);
    assert_eq!(back.messages[2].role, Role::User);
}

#[test]
fn stream_chunk_with_refs() {
    let chunk = StreamChunk {
        id: "chunk-ref".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://example.com".into(),
            title: Some("Ref Title".into()),
        }]),
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: StreamChunk = serde_json::from_str(&json).unwrap();
    let refs = back.refs.unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].url, "https://example.com");
}
