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
//! Tests for `abp_shim_claude::types` — Claude Messages API wire types.

use abp_shim_claude::types::*;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn minimal_request() -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: ClaudeContent::Text("Hello".into()),
        }],
        max_tokens: 1024,
        system: None,
        temperature: None,
        top_p: None,
        top_k: None,
        stream: None,
        tools: None,
        tool_choice: None,
    }
}

fn sample_response() -> MessagesResponse {
    MessagesResponse {
        id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
        type_field: "message".into(),
        role: "assistant".into(),
        content: vec![ContentBlock::Text {
            text: "Hello!".into(),
        }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        usage: ClaudeUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. MessagesRequest serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_minimal_roundtrip() {
    let req = minimal_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.max_tokens, 1024);
}

#[test]
fn request_optional_fields_omitted() {
    let req = minimal_request();
    let val: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert!(val.get("system").is_none());
    assert!(val.get("temperature").is_none());
    assert!(val.get("top_p").is_none());
    assert!(val.get("top_k").is_none());
    assert!(val.get("stream").is_none());
    assert!(val.get("tools").is_none());
    assert!(val.get("tool_choice").is_none());
}

#[test]
fn request_with_all_optional_fields() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: ClaudeContent::Text("Hi".into()),
        }],
        max_tokens: 2048,
        system: Some("You are helpful.".into()),
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        stream: Some(true),
        tools: Some(vec![ClaudeTool {
            name: "get_weather".into(),
            description: Some("Get current weather".into()),
            input_schema: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        }]),
        tool_choice: Some(ClaudeToolChoice::Auto {}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.system.as_deref(), Some("You are helpful."));
    assert_eq!(back.temperature, Some(0.7));
    assert_eq!(back.top_p, Some(0.9));
    assert_eq!(back.top_k, Some(40));
    assert_eq!(back.stream, Some(true));
    assert!(back.tools.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. ClaudeContent (untagged string vs blocks)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_string_from_json() {
    let c: ClaudeContent = serde_json::from_value(json!("hello")).unwrap();
    assert_eq!(c, ClaudeContent::Text("hello".into()));
}

#[test]
fn content_blocks_from_json() {
    let c: ClaudeContent = serde_json::from_value(json!([{"type": "text", "text": "hi"}])).unwrap();
    assert!(matches!(c, ClaudeContent::Blocks(_)));
    if let ClaudeContent::Blocks(blocks) = c {
        assert_eq!(blocks.len(), 1);
    }
}

#[test]
fn content_string_serializes_as_bare_string() {
    let c = ClaudeContent::Text("hello".into());
    let val = serde_json::to_value(&c).unwrap();
    assert!(val.is_string());
    assert_eq!(val.as_str().unwrap(), "hello");
}

#[test]
fn content_blocks_serializes_as_array() {
    let c = ClaudeContent::Blocks(vec![ContentBlock::Text { text: "hi".into() }]);
    let val = serde_json::to_value(&c).unwrap();
    assert!(val.is_array());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. ContentBlock variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_roundtrip() {
    let block = ContentBlock::Text {
        text: "Hello!".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"text""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_use_roundtrip() {
    let block = ContentBlock::ToolUse {
        id: "toolu_01A".into(),
        name: "bash".into(),
        input: json!({"command": "ls"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"tool_use""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_result_roundtrip() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "toolu_01A".into(),
        content: "file contents".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"tool_result""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_image_base64_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"image""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_image_url_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. ClaudeTool & ClaudeToolChoice
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_definition_roundtrip() {
    let tool = ClaudeTool {
        name: "get_weather".into(),
        description: Some("Gets the weather for a city".into()),
        input_schema: json!({
            "type": "object",
            "properties": {
                "city": {"type": "string"}
            },
            "required": ["city"]
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ClaudeTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn tool_choice_auto_roundtrip() {
    let choice = ClaudeToolChoice::Auto {};
    let json = serde_json::to_string(&choice).unwrap();
    assert!(json.contains(r#""type":"auto""#));
    let back: ClaudeToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(choice, back);
}

#[test]
fn tool_choice_any_roundtrip() {
    let choice = ClaudeToolChoice::Any {};
    let json = serde_json::to_string(&choice).unwrap();
    assert!(json.contains(r#""type":"any""#));
    let back: ClaudeToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(choice, back);
}

#[test]
fn tool_choice_specific_tool_roundtrip() {
    let choice = ClaudeToolChoice::Tool {
        name: "bash".into(),
    };
    let json = serde_json::to_string(&choice).unwrap();
    assert!(json.contains(r#""type":"tool""#));
    assert!(json.contains(r#""name":"bash""#));
    let back: ClaudeToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(choice, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. MessagesResponse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_roundtrip() {
    let resp = sample_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: MessagesResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn response_type_field_serializes_as_type() {
    let resp = sample_response();
    let val: serde_json::Value = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["type"], "message");
    // `type_field` key should NOT appear in JSON
    assert!(val.get("type_field").is_none());
}

#[test]
fn response_from_real_api_json() {
    let json = json!({
        "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
        "type": "message",
        "role": "assistant",
        "content": [
            {"type": "text", "text": "Hi! How can I help?"}
        ],
        "model": "claude-sonnet-4-20250514",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 12,
            "output_tokens": 8
        }
    });
    let resp: MessagesResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.type_field, "message");
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.usage.input_tokens, 12);
    assert_eq!(resp.content.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. ClaudeUsage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_minimal_roundtrip() {
    let usage = ClaudeUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.get("cache_creation_input_tokens").is_none());
}

#[test]
fn usage_with_cache_tokens() {
    let usage = ClaudeUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: Some(20),
        cache_read_input_tokens: Some(80),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.cache_creation_input_tokens, Some(20));
    assert_eq!(back.cache_read_input_tokens, Some(80));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. StreamEvent
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_message_start_roundtrip() {
    let ev = StreamEvent::MessageStart {
        message: sample_response(),
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"message_start""#));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_content_block_start_roundtrip() {
    let ev = StreamEvent::ContentBlockStart {
        index: 0,
        content_block: ContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"content_block_start""#));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_content_block_delta_text() {
    let ev = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::TextDelta {
            text: "Hello".into(),
        },
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"content_block_delta""#));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_content_block_stop_roundtrip() {
    let ev = StreamEvent::ContentBlockStop { index: 0 };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"content_block_stop""#));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_message_delta_roundtrip() {
    let ev = StreamEvent::MessageDelta {
        delta: MessageDeltaBody {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 25,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"message_delta""#));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_message_stop_roundtrip() {
    let ev = StreamEvent::MessageStop {};
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"message_stop""#));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_ping_roundtrip() {
    let ev = StreamEvent::Ping {};
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains(r#""type":"ping""#));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_delta_input_json_roundtrip() {
    let delta = StreamDelta::InputJsonDelta {
        partial_json: r#"{"ci"#.into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    assert!(json.contains(r#""type":"input_json_delta""#));
    let back: StreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(delta, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Full request JSON fidelity (real API shape)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_json_matches_api_shape() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: ClaudeContent::Blocks(vec![ContentBlock::Text {
                text: "Describe this image.".into(),
            }]),
        }],
        max_tokens: 1024,
        system: Some("Be concise.".into()),
        temperature: Some(0.5),
        top_p: None,
        top_k: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    let val: serde_json::Value = serde_json::to_value(&req).unwrap();
    assert_eq!(val["model"], "claude-sonnet-4-20250514");
    assert_eq!(val["max_tokens"], 1024);
    assert_eq!(val["system"], "Be concise.");
    assert!(val["messages"][0]["content"].is_array());
}

#[test]
fn message_with_string_content_json() {
    let msg = ClaudeMessage {
        role: "user".into(),
        content: ClaudeContent::Text("Hello".into()),
    };
    let val: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(val["role"], "user");
    assert_eq!(val["content"], "Hello");
}

#[test]
fn message_with_blocks_content_json() {
    let msg = ClaudeMessage {
        role: "user".into(),
        content: ClaudeContent::Blocks(vec![
            ContentBlock::Text {
                text: "Look at this:".into(),
            },
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "abc123".into(),
                },
            },
        ]),
    };
    let val: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert!(val["content"].is_array());
    assert_eq!(val["content"].as_array().unwrap().len(), 2);
}
