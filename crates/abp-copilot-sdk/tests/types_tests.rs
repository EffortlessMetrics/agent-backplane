// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the `types` module of abp-copilot-sdk.

use abp_copilot_sdk::types::*;
use std::collections::BTreeMap;

// ── Helpers ─────────────────────────────────────────────────────────────

fn sample_reference(rt: ReferenceType, id: &str) -> Reference {
    Reference {
        ref_type: rt,
        id: id.to_string(),
        uri: Some(format!("file:///{id}")),
        content: Some("hello".into()),
        metadata: None,
    }
}

fn minimal_chat_request() -> CopilotChatRequest {
    CopilotChatRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotChatMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: None,
        tools: None,
        tool_choice: None,
        intent: None,
        references: None,
    }
}

fn full_chat_response() -> CopilotChatResponse {
    CopilotChatResponse {
        id: "resp-1".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![CopilotChatChoice {
            index: 0,
            message: CopilotChatChoiceMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(CopilotUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            copilot_tokens: Some(3),
        }),
    }
}

// ── ReferenceType ───────────────────────────────────────────────────────

#[test]
fn reference_type_file_roundtrip() {
    let json = serde_json::to_string(&ReferenceType::File).unwrap();
    assert_eq!(json, r#""file""#);
    let rt: ReferenceType = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ReferenceType::File);
}

#[test]
fn reference_type_selection_roundtrip() {
    let json = serde_json::to_string(&ReferenceType::Selection).unwrap();
    assert_eq!(json, r#""selection""#);
    let rt: ReferenceType = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ReferenceType::Selection);
}

#[test]
fn reference_type_terminal_roundtrip() {
    let json = serde_json::to_string(&ReferenceType::Terminal).unwrap();
    assert_eq!(json, r#""terminal""#);
}

#[test]
fn reference_type_web_page_roundtrip() {
    let json = serde_json::to_string(&ReferenceType::WebPage).unwrap();
    assert_eq!(json, r#""web_page""#);
}

#[test]
fn reference_type_git_diff_roundtrip() {
    let json = serde_json::to_string(&ReferenceType::GitDiff).unwrap();
    assert_eq!(json, r#""git_diff""#);
    let rt: ReferenceType = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ReferenceType::GitDiff);
}

// ── Reference ───────────────────────────────────────────────────────────

#[test]
fn reference_serialize_minimal() {
    let r = Reference {
        ref_type: ReferenceType::File,
        id: "f1".into(),
        uri: None,
        content: None,
        metadata: None,
    };
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["type"], "file");
    assert_eq!(json["id"], "f1");
    assert!(json.get("uri").is_none());
    assert!(json.get("content").is_none());
}

#[test]
fn reference_roundtrip_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("lang".into(), serde_json::json!("rust"));
    let r = Reference {
        ref_type: ReferenceType::Selection,
        id: "sel-1".into(),
        uri: Some("file:///main.rs".into()),
        content: Some("fn main() {}".into()),
        metadata: Some(meta),
    };
    let json = serde_json::to_string(&r).unwrap();
    let r2: Reference = serde_json::from_str(&json).unwrap();
    assert_eq!(r, r2);
}

#[test]
fn reference_deserialize_unknown_fields_ignored() {
    let json = r#"{
        "type": "terminal",
        "id": "t1",
        "uri": null,
        "extra_field": true
    }"#;
    let r: Reference = serde_json::from_str(json).unwrap();
    assert_eq!(r.ref_type, ReferenceType::Terminal);
    assert_eq!(r.id, "t1");
}

// ── CopilotChatRequest ─────────────────────────────────────────────────

#[test]
fn chat_request_minimal_roundtrip() {
    let req = minimal_chat_request();
    let json = serde_json::to_string(&req).unwrap();
    let req2: CopilotChatRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, req2);
}

#[test]
fn chat_request_with_copilot_extensions() {
    let mut req = minimal_chat_request();
    req.intent = Some("code-review".into());
    req.references = Some(vec![sample_reference(ReferenceType::File, "src/main.rs")]);
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["intent"], "code-review");
    assert!(json["references"].is_array());
    assert_eq!(json["references"][0]["type"], "file");
}

#[test]
fn chat_request_optional_fields_omitted_when_none() {
    let req = minimal_chat_request();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("temperature").is_none());
    assert!(json.get("top_p").is_none());
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("stream").is_none());
    assert!(json.get("tools").is_none());
    assert!(json.get("tool_choice").is_none());
    assert!(json.get("intent").is_none());
    assert!(json.get("references").is_none());
}

#[test]
fn chat_request_with_tools() {
    let mut req = minimal_chat_request();
    req.tools = Some(vec![CopilotTool {
        tool_type: "function".into(),
        function: CopilotToolFunction {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        },
    }]);
    let json = serde_json::to_string(&req).unwrap();
    let req2: CopilotChatRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.tools, req2.tools);
}

// ── CopilotChatResponse ────────────────────────────────────────────────

#[test]
fn chat_response_roundtrip() {
    let resp = full_chat_response();
    let json = serde_json::to_string(&resp).unwrap();
    let resp2: CopilotChatResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, resp2);
}

#[test]
fn chat_response_object_field() {
    let resp = full_chat_response();
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["object"], "chat.completion");
}

#[test]
fn chat_response_without_usage() {
    let mut resp = full_chat_response();
    resp.usage = None;
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json.get("usage").is_none());
}

// ── CopilotUsage ────────────────────────────────────────────────────────

#[test]
fn usage_standard_fields() {
    let u = CopilotUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        copilot_tokens: None,
    };
    let json = serde_json::to_value(&u).unwrap();
    assert_eq!(json["prompt_tokens"], 100);
    assert_eq!(json["completion_tokens"], 50);
    assert_eq!(json["total_tokens"], 150);
    assert!(json.get("copilot_tokens").is_none());
}

#[test]
fn usage_with_copilot_tokens() {
    let u = CopilotUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        copilot_tokens: Some(42),
    };
    let json = serde_json::to_value(&u).unwrap();
    assert_eq!(json["copilot_tokens"], 42);
}

#[test]
fn usage_roundtrip() {
    let u = CopilotUsage {
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
        copilot_tokens: Some(5),
    };
    let json = serde_json::to_string(&u).unwrap();
    let u2: CopilotUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, u2);
}

// ── CopilotStreamChunk ─────────────────────────────────────────────────

#[test]
fn stream_chunk_roundtrip() {
    let chunk = CopilotStreamChunk {
        id: "chunk-1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![CopilotStreamChoice {
            index: 0,
            delta: CopilotStreamDelta {
                role: Some("assistant".into()),
                content: Some("He".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let chunk2: CopilotStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, chunk2);
}

#[test]
fn stream_chunk_final_with_finish_reason() {
    let chunk = CopilotStreamChunk {
        id: "chunk-end".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![CopilotStreamChoice {
            index: 0,
            delta: CopilotStreamDelta::default(),
            finish_reason: Some("stop".into()),
        }],
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["choices"][0]["finish_reason"], "stop");
}

#[test]
fn stream_delta_default_is_empty() {
    let d = CopilotStreamDelta::default();
    assert_eq!(d.role, None);
    assert_eq!(d.content, None);
    assert_eq!(d.tool_calls, None);
}

#[test]
fn stream_tool_call_fragment_roundtrip() {
    let tc = CopilotStreamToolCall {
        index: 0,
        id: Some("call-1".into()),
        call_type: Some("function".into()),
        function: Some(CopilotStreamFunctionCall {
            name: Some("read_file".into()),
            arguments: Some(r#"{"pa"#.into()),
        }),
    };
    let json = serde_json::to_string(&tc).unwrap();
    let tc2: CopilotStreamToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, tc2);
    // Verify "type" rename
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "function");
}

// ── CopilotChatMessage ─────────────────────────────────────────────────

#[test]
fn chat_message_tool_role() {
    let msg = CopilotChatMessage {
        role: "tool".into(),
        content: Some("result".into()),
        name: None,
        tool_calls: None,
        tool_call_id: Some("call-1".into()),
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "tool");
    assert_eq!(json["tool_call_id"], "call-1");
}

#[test]
fn chat_message_assistant_with_tool_calls() {
    let msg = CopilotChatMessage {
        role: "assistant".into(),
        content: None,
        name: None,
        tool_calls: Some(vec![CopilotToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: CopilotFunctionCall {
                name: "edit".into(),
                arguments: r#"{"file":"a.rs"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let msg2: CopilotChatMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, msg2);
}

// ── CopilotTool ─────────────────────────────────────────────────────────

#[test]
fn tool_type_field_renamed() {
    let tool = CopilotTool {
        tool_type: "function".into(),
        function: CopilotToolFunction {
            name: "search".into(),
            description: "Search code".into(),
            parameters: serde_json::json!({}),
        },
    };
    let v = serde_json::to_value(&tool).unwrap();
    assert!(v.get("type").is_some());
    assert!(v.get("tool_type").is_none());
}
