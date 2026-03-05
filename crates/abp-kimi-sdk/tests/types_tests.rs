#![allow(clippy::all)]
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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the Kimi API types.

use abp_kimi_sdk::types::*;
use serde_json::{Value, json};

// ── Helper builders ─────────────────────────────────────────────────────

fn minimal_request() -> KimiChatRequest {
    KimiChatRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![ChatMessage::User {
            content: "Hello".into(),
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        use_search: None,
        search_options: None,
    }
}

fn minimal_response() -> KimiChatResponse {
    KimiChatResponse {
        id: "cmpl-abc123".into(),
        object: "chat.completion".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![Choice {
            index: 0,
            message: ChoiceMessage {
                role: "assistant".into(),
                content: Some("Hi!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            search_tokens: None,
        }),
    }
}

fn minimal_stream_chunk() -> KimiStreamChunk {
    KimiStreamChunk {
        id: "cmpl-abc123".into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    }
}

// ── Request serialization ───────────────────────────────────────────────

#[test]
fn serialize_minimal_request() {
    let req = minimal_request();
    let v: Value = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "moonshot-v1-8k");
    assert!(v.get("use_search").is_none());
    assert!(v.get("search_options").is_none());
}

#[test]
fn serialize_request_with_search() {
    let mut req = minimal_request();
    req.use_search = Some(true);
    req.search_options = Some(SearchOptions {
        mode: SearchMode::Always,
        result_count: Some(5),
    });
    let v: Value = serde_json::to_value(&req).unwrap();
    assert_eq!(v["use_search"], true);
    assert_eq!(v["search_options"]["mode"], "always");
    assert_eq!(v["search_options"]["result_count"], 5);
}

#[test]
fn deserialize_request_roundtrip() {
    let req = minimal_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiChatRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn deserialize_request_with_search_from_json() {
    let raw = json!({
        "model": "moonshot-v1-128k",
        "messages": [{"role": "user", "content": "hi"}],
        "use_search": true,
        "search_options": {"mode": "auto", "result_count": 3}
    });
    let req: KimiChatRequest = serde_json::from_value(raw).unwrap();
    assert_eq!(req.use_search, Some(true));
    let opts = req.search_options.unwrap();
    assert_eq!(opts.mode, SearchMode::Auto);
    assert_eq!(opts.result_count, Some(3));
}

#[test]
fn request_optional_fields_omitted_when_none() {
    let req = minimal_request();
    let v: Value = serde_json::to_value(&req).unwrap();
    let obj = v.as_object().unwrap();
    for key in &[
        "temperature",
        "top_p",
        "max_tokens",
        "stream",
        "tools",
        "tool_choice",
        "use_search",
        "search_options",
    ] {
        assert!(!obj.contains_key(*key), "expected {key} to be absent");
    }
}

// ── Search types ────────────────────────────────────────────────────────

#[test]
fn search_mode_serialization() {
    assert_eq!(serde_json::to_value(SearchMode::Auto).unwrap(), "auto");
    assert_eq!(serde_json::to_value(SearchMode::Always).unwrap(), "always");
    assert_eq!(serde_json::to_value(SearchMode::Never).unwrap(), "never");
}

#[test]
fn search_mode_deserialization() {
    let auto: SearchMode = serde_json::from_value(json!("auto")).unwrap();
    assert_eq!(auto, SearchMode::Auto);
    let always: SearchMode = serde_json::from_value(json!("always")).unwrap();
    assert_eq!(always, SearchMode::Always);
    let never: SearchMode = serde_json::from_value(json!("never")).unwrap();
    assert_eq!(never, SearchMode::Never);
}

#[test]
fn search_options_without_result_count() {
    let opts = SearchOptions {
        mode: SearchMode::Never,
        result_count: None,
    };
    let v: Value = serde_json::to_value(&opts).unwrap();
    assert_eq!(v["mode"], "never");
    assert!(v.get("result_count").is_none());
}

// ── Chat message ────────────────────────────────────────────────────────

#[test]
fn system_message_roundtrip() {
    let msg = ChatMessage::System {
        content: "You are helpful.".into(),
    };
    let v: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "system");
    assert_eq!(v["content"], "You are helpful.");
    let back: ChatMessage = serde_json::from_value(v).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn assistant_message_with_tool_calls() {
    let msg = ChatMessage::Assistant {
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"city":"London"}"#.into(),
            },
        }]),
    };
    let v: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "assistant");
    assert!(v.get("content").is_none());
    assert_eq!(v["tool_calls"][0]["function"]["name"], "get_weather");
}

#[test]
fn tool_message_roundtrip() {
    let msg = ChatMessage::Tool {
        content: "sunny, 22°C".into(),
        tool_call_id: "call_1".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

// ── Response ────────────────────────────────────────────────────────────

#[test]
fn serialize_minimal_response() {
    let resp = minimal_response();
    let v: Value = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["choices"][0]["message"]["content"], "Hi!");
    assert_eq!(v["usage"]["total_tokens"], 15);
}

#[test]
fn response_roundtrip() {
    let resp = minimal_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiChatResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn response_with_search_tokens() {
    let mut resp = minimal_response();
    resp.usage = Some(KimiUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 180,
        search_tokens: Some(30),
    });
    let v: Value = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["usage"]["search_tokens"], 30);
}

#[test]
fn response_without_usage() {
    let mut resp = minimal_response();
    resp.usage = None;
    let v: Value = serde_json::to_value(&resp).unwrap();
    assert!(v.get("usage").is_none());
}

// ── Usage ───────────────────────────────────────────────────────────────

#[test]
fn usage_without_search_tokens_omits_field() {
    let usage = KimiUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
        search_tokens: None,
    };
    let v: Value = serde_json::to_value(&usage).unwrap();
    assert!(v.get("search_tokens").is_none());
}

#[test]
fn usage_roundtrip_with_search_tokens() {
    let usage = KimiUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 170,
        search_tokens: Some(20),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: KimiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

// ── Streaming ───────────────────────────────────────────────────────────

#[test]
fn stream_chunk_roundtrip() {
    let chunk = minimal_stream_chunk();
    let json = serde_json::to_string(&chunk).unwrap();
    let back: KimiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn stream_chunk_with_tool_call_delta() {
    let chunk = KimiStreamChunk {
        id: "cmpl-xyz".into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
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
    };
    let v: Value = serde_json::to_value(&chunk).unwrap();
    assert_eq!(
        v["choices"][0]["delta"]["tool_calls"][0]["function"]["name"],
        "search"
    );
}

#[test]
fn stream_chunk_final_with_usage() {
    let chunk = KimiStreamChunk {
        id: "cmpl-abc".into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "moonshot-v1-8k".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: StreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            search_tokens: None,
        }),
    };
    let v: Value = serde_json::to_value(&chunk).unwrap();
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["usage"]["total_tokens"], 15);
}

#[test]
fn stream_delta_default_is_empty() {
    let delta = StreamDelta::default();
    let v: Value = serde_json::to_value(&delta).unwrap();
    assert_eq!(v, json!({}));
}

// ── Tool types ──────────────────────────────────────────────────────────

#[test]
fn tool_choice_mode_auto_roundtrip() {
    let choice = ToolChoice::Mode(ToolChoiceMode::Auto);
    let json = serde_json::to_string(&choice).unwrap();
    assert_eq!(json, r#""auto""#);
    let back: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(choice, back);
}

#[test]
fn tool_choice_function_roundtrip() {
    let choice = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "get_weather".into(),
        },
    };
    let v: Value = serde_json::to_value(&choice).unwrap();
    assert_eq!(v["type"], "function");
    assert_eq!(v["function"]["name"], "get_weather");
    let back: ToolChoice = serde_json::from_value(v).unwrap();
    assert_eq!(choice, back);
}

#[test]
fn tool_definition_roundtrip() {
    let tool = Tool {
        tool_type: "function".into(),
        function: FunctionDef {
            name: "get_weather".into(),
            description: "Get the current weather".into(),
            parameters: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: Tool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

// ── Clone / Debug / PartialEq ───────────────────────────────────────────

#[test]
fn types_implement_clone() {
    let req = minimal_request();
    let cloned = req.clone();
    assert_eq!(req, cloned);

    let resp = minimal_response();
    let cloned = resp.clone();
    assert_eq!(resp, cloned);

    let chunk = minimal_stream_chunk();
    let cloned = chunk.clone();
    assert_eq!(chunk, cloned);
}

#[test]
fn types_implement_debug() {
    let req = minimal_request();
    let dbg = format!("{req:?}");
    assert!(dbg.contains("moonshot-v1-8k"));
}
