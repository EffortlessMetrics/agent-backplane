// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for Kimi SDK types, serde roundtrips, mapping, streaming,
//! built-in tools, citations, and token usage.

use abp_core::{AgentEventKind, WorkOrderBuilder};
use abp_kimi_sdk::dialect::*;

// ---------------------------------------------------------------------------
// 1. Serde roundtrip: KimiRequest
// ---------------------------------------------------------------------------

#[test]
fn serde_roundtrip_kimi_request_minimal() {
    let req = KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        }],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "moonshot-v1-8k");
    assert_eq!(parsed.messages.len(), 1);
    // Verify optional fields are omitted from JSON
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("tools"));
}

#[test]
fn serde_roundtrip_kimi_request_with_tools() {
    let req = KimiRequest {
        model: "moonshot-v1-32k".into(),
        messages: vec![KimiMessage {
            role: "user".into(),
            content: Some("Search for Rust tutorials".into()),
            tool_call_id: None,
            tool_calls: None,
        }],
        max_tokens: Some(2048),
        temperature: Some(0.7),
        stream: Some(true),
        tools: Some(vec![
            KimiTool::Function {
                function: KimiFunctionDef {
                    name: "calculator".into(),
                    description: "Do math".into(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            },
            KimiTool::BuiltinFunction {
                function: KimiBuiltinFunction {
                    name: "$web_search".into(),
                },
            },
        ]),
        use_search: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.tools.as_ref().unwrap().len(), 2);
    assert_eq!(parsed.stream, Some(true));
    assert_eq!(parsed.use_search, Some(true));
}

// ---------------------------------------------------------------------------
// 2. Serde roundtrip: KimiResponse
// ---------------------------------------------------------------------------

#[test]
fn serde_roundtrip_kimi_response_with_refs() {
    let resp = KimiResponse {
        id: "cmpl_refs".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("According to [1], Rust is fast.".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 20,
            completion_tokens: 15,
            total_tokens: 35,
        }),
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://rust-lang.org".into(),
            title: Some("Rust Programming Language".into()),
        }]),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.refs.as_ref().unwrap().len(), 1);
    assert_eq!(
        parsed.refs.as_ref().unwrap()[0].url,
        "https://rust-lang.org"
    );
    assert_eq!(
        parsed.refs.as_ref().unwrap()[0].title.as_deref(),
        Some("Rust Programming Language")
    );
}

#[test]
fn serde_roundtrip_kimi_response_no_refs() {
    let resp = KimiResponse {
        id: "cmpl_norefs".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Plain answer.".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    // refs should be omitted when None (skip_serializing_if)
    assert!(
        !json.contains("\"refs\""),
        "expected no 'refs' key in JSON but got: {json}"
    );
    let parsed: KimiResponse = serde_json::from_str(&json).unwrap();
    assert!(parsed.refs.is_none());
}

// ---------------------------------------------------------------------------
// 3. Serde roundtrip: KimiChunk (streaming)
// ---------------------------------------------------------------------------

#[test]
fn serde_roundtrip_kimi_chunk_text_delta() {
    let chunk = KimiChunk {
        id: "chatcmpl_stream1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
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
    let parsed: KimiChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.choices[0].delta.content.as_deref(), Some("Hello"));
    assert!(parsed.choices[0].finish_reason.is_none());
}

#[test]
fn serde_roundtrip_kimi_chunk_with_tool_call() {
    let chunk = KimiChunk {
        id: "chatcmpl_stream2".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000001,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: Some("assistant".into()),
                content: None,
                tool_calls: Some(vec![KimiChunkToolCall {
                    index: 0,
                    id: Some("call_abc".into()),
                    call_type: Some("function".into()),
                    function: Some(KimiChunkFunctionCall {
                        name: Some("web_search".into()),
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
    let parsed: KimiChunk = serde_json::from_str(&json).unwrap();
    let tc = &parsed.choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id.as_deref(), Some("call_abc"));
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("web_search")
    );
}

#[test]
fn serde_roundtrip_kimi_chunk_final_with_usage() {
    let chunk = KimiChunk {
        id: "chatcmpl_stream3".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000002,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        refs: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let parsed: KimiChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.usage.as_ref().unwrap().total_tokens, 15);
    assert_eq!(parsed.choices[0].finish_reason.as_deref(), Some("stop"));
}

// ---------------------------------------------------------------------------
// 4. Message role mapping
// ---------------------------------------------------------------------------

#[test]
fn kimi_role_serde_roundtrip() {
    let roles = [
        KimiRole::System,
        KimiRole::User,
        KimiRole::Assistant,
        KimiRole::Tool,
    ];
    for role in &roles {
        let json = serde_json::to_string(role).unwrap();
        let parsed: KimiRole = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, role);
    }
}

#[test]
fn kimi_role_display() {
    assert_eq!(KimiRole::System.to_string(), "system");
    assert_eq!(KimiRole::User.to_string(), "user");
    assert_eq!(KimiRole::Assistant.to_string(), "assistant");
    assert_eq!(KimiRole::Tool.to_string(), "tool");
}

#[test]
fn kimi_message_with_tool_role() {
    let msg = KimiMessage {
        role: "tool".into(),
        content: Some(r#"{"result": 42}"#.into()),
        tool_call_id: Some("call_1".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("tool_call_id"));
    let parsed: KimiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, "tool");
    assert_eq!(parsed.tool_call_id.as_deref(), Some("call_1"));
}

// ---------------------------------------------------------------------------
// 5. Tool mapping (including built-in search_internet)
// ---------------------------------------------------------------------------

#[test]
fn builtin_search_internet_serde() {
    let tool = builtin_search_internet();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$web_search");
    let json = serde_json::to_string(&tool).unwrap();
    let parsed: KimiBuiltinTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn builtin_browser_serde() {
    let tool = builtin_browser();
    assert_eq!(tool.tool_type, "builtin_function");
    assert_eq!(tool.function.name, "$browser");
}

#[test]
fn kimi_tool_enum_function_serde() {
    let tool = KimiTool::Function {
        function: KimiFunctionDef {
            name: "get_weather".into(),
            description: "Get current weather".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "city": { "type": "string" } },
                "required": ["city"]
            }),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("\"type\":\"function\""));
    let parsed: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn kimi_tool_enum_builtin_serde() {
    let tool = KimiTool::BuiltinFunction {
        function: KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains("\"type\":\"builtin_function\""));
    let parsed: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

// ---------------------------------------------------------------------------
// 6. Stream delta handling (map_stream_event)
// ---------------------------------------------------------------------------

#[test]
fn map_stream_event_text_delta() {
    let chunk = KimiChunk {
        id: "ch1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("token1".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = map_stream_event(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "token1"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn map_stream_event_empty_content_produces_no_events() {
    let chunk = KimiChunk {
        id: "ch_empty".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: Some("assistant".into()),
                content: Some(String::new()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let events = map_stream_event(&chunk);
    assert!(events.is_empty());
}

#[test]
fn map_stream_event_finish_reason_emits_run_completed() {
    let chunk = KimiChunk {
        id: "ch_fin".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
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
fn map_stream_event_with_refs_attaches_ext() {
    let chunk = KimiChunk {
        id: "ch_refs".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("See [1]".into()),
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
    let ext = events[0].ext.as_ref().expect("ext should be present");
    assert!(ext.contains_key("kimi_refs"));
}

// ---------------------------------------------------------------------------
// 7. ToolCallAccumulator for streaming tool calls
// ---------------------------------------------------------------------------

#[test]
fn tool_call_accumulator_collects_fragments() {
    let mut acc = ToolCallAccumulator::new();

    // First fragment: id + name + partial args
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(KimiChunkFunctionCall {
            name: Some("search".into()),
            arguments: Some(r#"{"q":"#.into()),
        }),
    }]);

    // Second fragment: more args
    acc.feed(&[KimiChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(KimiChunkFunctionCall {
            name: None,
            arguments: Some(r#""hello"}"#.into()),
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
            assert_eq!(input["q"], "hello");
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn tool_call_accumulator_multiple_tools() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        KimiChunkToolCall {
            index: 0,
            id: Some("call_a".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("search".into()),
                arguments: Some(r#"{"q":"a"}"#.into()),
            }),
        },
        KimiChunkToolCall {
            index: 1,
            id: Some("call_b".into()),
            call_type: Some("function".into()),
            function: Some(KimiChunkFunctionCall {
                name: Some("calc".into()),
                arguments: Some(r#"{"x":1}"#.into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

// ---------------------------------------------------------------------------
// 8. Usage / token counting
// ---------------------------------------------------------------------------

#[test]
fn extract_usage_returns_btreemap() {
    let resp = KimiResponse {
        id: "u1".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: Some(KimiUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
        refs: None,
    };
    let usage = extract_usage(&resp).unwrap();
    assert_eq!(usage["prompt_tokens"], serde_json::json!(100));
    assert_eq!(usage["completion_tokens"], serde_json::json!(50));
    assert_eq!(usage["total_tokens"], serde_json::json!(150));
}

#[test]
fn extract_usage_returns_none_when_absent() {
    let resp = KimiResponse {
        id: "u2".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    };
    assert!(extract_usage(&resp).is_none());
}

#[test]
fn kimi_usage_serde_roundtrip() {
    let usage = KimiUsage {
        prompt_tokens: 42,
        completion_tokens: 18,
        total_tokens: 60,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: KimiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

// ---------------------------------------------------------------------------
// 9. Citation / refs handling
// ---------------------------------------------------------------------------

#[test]
fn kimi_ref_serde_roundtrip() {
    let r = KimiRef {
        index: 1,
        url: "https://example.com/page".into(),
        title: Some("Example Page".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    let parsed: KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, r);
}

#[test]
fn kimi_ref_without_title() {
    let r = KimiRef {
        index: 2,
        url: "https://example.com".into(),
        title: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("title"));
    let parsed: KimiRef = serde_json::from_str(&json).unwrap();
    assert!(parsed.title.is_none());
}

#[test]
fn map_response_with_refs_attaches_ext() {
    let resp = KimiResponse {
        id: "cmpl_refs".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("According to [1], Rust is fast.".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://rust-lang.org".into(),
            title: Some("Rust".into()),
        }]),
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().expect("ext should be set for refs");
    assert!(ext.contains_key("kimi_refs"));
    let refs_val = &ext["kimi_refs"];
    assert!(refs_val.is_array());
    assert_eq!(refs_val.as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// 10. k1 reasoning model
// ---------------------------------------------------------------------------

#[test]
fn k1_model_is_recognized() {
    assert!(is_known_model("k1"));
    assert!(is_known_model("kimi-latest"));
}

#[test]
fn map_work_order_with_k1_reasoning() {
    let wo = WorkOrderBuilder::new("Solve this math problem").build();
    let cfg = KimiConfig {
        model: "k1".into(),
        use_k1_reasoning: Some(true),
        ..KimiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.model, "k1");
    assert_eq!(req.use_search, Some(true));
}

// ---------------------------------------------------------------------------
// 11. Config serde with new field
// ---------------------------------------------------------------------------

#[test]
fn kimi_config_serde_with_k1_reasoning() {
    let cfg = KimiConfig {
        use_k1_reasoning: Some(true),
        ..KimiConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("use_k1_reasoning"));
    let parsed: KimiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.use_k1_reasoning, Some(true));
}

#[test]
fn kimi_config_serde_omits_none_k1_reasoning() {
    let cfg = KimiConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("use_k1_reasoning"));
}
