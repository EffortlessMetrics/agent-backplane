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
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the `abp-shim-claude` crate covering the full
//! shim surface: request/response types, content blocks, streaming, IR
//! lowering roundtrips, capability mapping, error handling, and edge cases.

use abp_claude_sdk::dialect::{
    self, ClaudeApiError, ClaudeContentBlock, ClaudeImageSource, ClaudeMessage, ClaudeMessageDelta,
    ClaudeResponse, ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage, ThinkingConfig,
};
use abp_claude_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEvent, AgentEventKind};
use abp_shim_claude::{
    content_block_from_ir, content_block_to_ir, message_to_ir, request_to_claude,
    request_to_work_order, response_from_claude, response_from_events, stream_event_from_claude,
    AnthropicClient, ApiError, ContentBlock, EventStream, ImageSource, Message,
    MessageDeltaPayload, MessageRequest, MessageResponse, Role, ShimError, StreamDelta,
    StreamEvent, Usage,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use tokio_stream::{Stream, StreamExt};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn simple_request(text: &str) -> MessageRequest {
    MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_event_ext(kind: AgentEventKind, ext: BTreeMap<String, serde_json::Value>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

fn thinking_ext() -> BTreeMap<String, serde_json::Value> {
    let mut m = BTreeMap::new();
    m.insert("thinking".into(), serde_json::Value::Bool(true));
    m
}

fn thinking_ext_with_sig(sig: &str) -> BTreeMap<String, serde_json::Value> {
    let mut m = thinking_ext();
    m.insert("signature".into(), serde_json::Value::String(sig.into()));
    m
}

fn sample_claude_response(text: &str) -> ClaudeResponse {
    ClaudeResponse {
        id: "msg_sample".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text { text: text.into() }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. MessageRequest construction & validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_minimal_serde_roundtrip() {
    let req = simple_request("hi");
    let json = serde_json::to_string(&req).unwrap();
    let back: MessageRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.max_tokens, req.max_tokens);
    assert_eq!(back.messages.len(), 1);
}

#[test]
fn request_all_fields_present_in_json() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 8192,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "test".into(),
            }],
        }],
        system: Some("sys".into()),
        temperature: Some(0.3),
        stop_sequences: Some(vec!["END".into()]),
        thinking: Some(ThinkingConfig::new(2048)),
        stream: Some(true),
    };
    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "claude-sonnet-4-20250514");
    assert_eq!(v["max_tokens"], 8192);
    assert_eq!(v["system"], "sys");
    assert_eq!(v["temperature"], 0.3);
    assert!(v["stop_sequences"].is_array());
    assert!(v["thinking"].is_object());
    assert_eq!(v["stream"], true);
}

#[test]
fn request_optional_fields_absent_when_none() {
    let req = simple_request("x");
    let v = serde_json::to_value(&req).unwrap();
    assert!(v.get("system").is_none());
    assert!(v.get("temperature").is_none());
    assert!(v.get("stop_sequences").is_none());
    assert!(v.get("thinking").is_none());
    assert!(v.get("stream").is_none());
}

#[test]
fn request_zero_max_tokens_serde() {
    let mut req = simple_request("x");
    req.max_tokens = 0;
    let json = serde_json::to_string(&req).unwrap();
    let back: MessageRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_tokens, 0);
}

#[test]
fn request_multiple_stop_sequences() {
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 100,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "t".into() }],
        }],
        system: None,
        temperature: None,
        stop_sequences: Some(vec!["A".into(), "B".into(), "C".into()]),
        thinking: None,
        stream: None,
    };
    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["stop_sequences"].as_array().unwrap().len(), 3);
}

#[test]
fn request_temperature_boundary_zero() {
    let mut req = simple_request("x");
    req.temperature = Some(0.0);
    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["temperature"], 0.0);
}

#[test]
fn request_temperature_boundary_one() {
    let mut req = simple_request("x");
    req.temperature = Some(1.0);
    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["temperature"], 1.0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. MessageResponse parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_serde_roundtrip_complete() {
    let resp = MessageResponse {
        id: "msg_abc".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![
            ContentBlock::Text {
                text: "Hello".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: json!({"cmd": "ls"}),
            },
        ],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("tool_use".into()),
        stop_sequence: Some("END".into()),
        usage: Usage {
            input_tokens: 50,
            output_tokens: 150,
            cache_creation_input_tokens: Some(20),
            cache_read_input_tokens: Some(10),
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: MessageResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn response_has_type_field_as_message() {
    let resp = MessageResponse {
        id: "msg_t".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![],
        model: "test".into(),
        stop_reason: None,
        stop_sequence: None,
        usage: Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["type"], "message");
}

#[test]
fn response_from_claude_maps_all_fields() {
    let claude_resp = ClaudeResponse {
        id: "msg_full".into(),
        model: "claude-opus-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text { text: "hi".into() },
            ClaudeContentBlock::ToolUse {
                id: "tu_x".into(),
                name: "grep".into(),
                input: json!({"q": "test"}),
            },
        ],
        stop_reason: Some("tool_use".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation_input_tokens: Some(50),
            cache_read_input_tokens: Some(30),
        }),
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.id, "msg_full");
    assert_eq!(resp.model, "claude-opus-4-20250514");
    assert_eq!(resp.content.len(), 2);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(resp.usage.input_tokens, 100);
    assert_eq!(resp.usage.output_tokens, 200);
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(50));
    assert_eq!(resp.usage.cache_read_input_tokens, Some(30));
}

#[test]
fn response_from_claude_no_usage_defaults_zero() {
    let resp = response_from_claude(&ClaudeResponse {
        id: "msg_nu".into(),
        model: "t".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: None,
        usage: None,
    });
    assert_eq!(resp.usage.input_tokens, 0);
    assert_eq!(resp.usage.output_tokens, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Content block types (text, image, tool_use, tool_result, thinking)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_serde_has_type_field() {
    let block = ContentBlock::Text {
        text: "hello".into(),
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "text");
    assert_eq!(v["text"], "hello");
}

#[test]
fn content_block_tool_use_serde_has_type_field() {
    let block = ContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: json!({}),
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_use");
}

#[test]
fn content_block_tool_result_serde_has_type_field() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("output".into()),
        is_error: None,
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_result");
}

#[test]
fn content_block_thinking_serde_has_type_field() {
    let block = ContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: None,
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "thinking");
}

#[test]
fn content_block_image_serde_has_type_field() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc".into(),
        },
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "image");
}

#[test]
fn content_block_text_roundtrip_ir() {
    let block = ContentBlock::Text {
        text: "foobar".into(),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_tool_use_roundtrip_ir() {
    let block = ContentBlock::ToolUse {
        id: "tu_42".into(),
        name: "read".into(),
        input: json!({"path": "/tmp/a"}),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_tool_result_success_roundtrip_ir() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_ok".into(),
        content: Some("success output".into()),
        is_error: Some(false),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_tool_result_error_roundtrip_ir() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_fail".into(),
        content: Some("ENOENT".into()),
        is_error: Some(true),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_tool_result_none_content_roundtrip_ir() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_nil".into(),
        content: None,
        is_error: None,
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_thinking_with_signature_roundtrip_ir() {
    let block = ContentBlock::Thinking {
        thinking: "deep thought".into(),
        signature: Some("sig_xyz".into()),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_thinking_without_signature_roundtrip_ir() {
    let block = ContentBlock::Thinking {
        thinking: "simple thought".into(),
        signature: None,
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_image_base64_roundtrip_ir() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBORw0KGgoAAAAN".into(),
        },
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_image_url_roundtrip_ir() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/photo.jpg".into(),
        },
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn content_block_tool_use_complex_nested_input() {
    let input = json!({
        "files": [
            {"path": "a.rs", "line": 10},
            {"path": "b.rs", "line": 20}
        ],
        "options": {"recursive": true, "max_depth": null}
    });
    let block = ContentBlock::ToolUse {
        id: "tu_nested".into(),
        name: "multi_read".into(),
        input: input.clone(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_use_array_input() {
    let block = ContentBlock::ToolUse {
        id: "tu_arr".into(),
        name: "multi".into(),
        input: json!([1, 2, 3]),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming (StreamEvent, StreamDelta)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_ping_serde_roundtrip() {
    let ev = StreamEvent::Ping {};
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[test]
fn stream_event_message_stop_serde_roundtrip() {
    let ev = StreamEvent::MessageStop {};
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[test]
fn stream_event_content_block_stop_serde_roundtrip() {
    let ev = StreamEvent::ContentBlockStop { index: 3 };
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[test]
fn stream_delta_text_serde_roundtrip() {
    let d = StreamDelta::TextDelta {
        text: "hello".into(),
    };
    let j = serde_json::to_string(&d).unwrap();
    assert_eq!(serde_json::from_str::<StreamDelta>(&j).unwrap(), d);
}

#[test]
fn stream_delta_input_json_serde_roundtrip() {
    let d = StreamDelta::InputJsonDelta {
        partial_json: "{\"path\":".into(),
    };
    let j = serde_json::to_string(&d).unwrap();
    assert_eq!(serde_json::from_str::<StreamDelta>(&j).unwrap(), d);
}

#[test]
fn stream_delta_thinking_serde_roundtrip() {
    let d = StreamDelta::ThinkingDelta {
        thinking: "hmm".into(),
    };
    let j = serde_json::to_string(&d).unwrap();
    assert_eq!(serde_json::from_str::<StreamDelta>(&j).unwrap(), d);
}

#[test]
fn stream_delta_signature_serde_roundtrip() {
    let d = StreamDelta::SignatureDelta {
        signature: "sig_part".into(),
    };
    let j = serde_json::to_string(&d).unwrap();
    assert_eq!(serde_json::from_str::<StreamDelta>(&j).unwrap(), d);
}

#[test]
fn stream_event_message_delta_serde_roundtrip() {
    let ev = StreamEvent::MessageDelta {
        delta: MessageDeltaPayload {
            stop_reason: Some("end_turn".into()),
            stop_sequence: Some("STOP".into()),
        },
        usage: Some(Usage {
            input_tokens: 5,
            output_tokens: 10,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[test]
fn stream_event_error_serde_roundtrip() {
    let ev = StreamEvent::Error {
        error: ApiError {
            error_type: "overloaded_error".into(),
            message: "Server busy".into(),
        },
    };
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[test]
fn stream_event_content_block_delta_text_serde() {
    let ev = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::TextDelta {
            text: "world".into(),
        },
    };
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[test]
fn stream_event_content_block_start_text_serde() {
    let ev = StreamEvent::ContentBlockStart {
        index: 0,
        content_block: ContentBlock::Text {
            text: String::new(),
        },
    };
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[test]
fn stream_event_content_block_start_tool_use_serde() {
    let ev = StreamEvent::ContentBlockStart {
        index: 1,
        content_block: ContentBlock::ToolUse {
            id: "tu_s".into(),
            name: "search".into(),
            input: json!({}),
        },
    };
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<StreamEvent>(&j).unwrap(), ev);
}

#[tokio::test]
async fn event_stream_from_vec_and_collect() {
    let events = vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}];
    let stream = EventStream::from_vec(events.clone());
    let collected = stream.collect_all().await;
    assert_eq!(collected, events);
}

#[tokio::test]
async fn event_stream_size_hint_tracks() {
    let stream = EventStream::from_vec(vec![StreamEvent::Ping {}, StreamEvent::Ping {}]);
    assert_eq!(stream.size_hint(), (2, Some(2)));
}

#[tokio::test]
async fn event_stream_empty_returns_none() {
    let mut stream = EventStream::from_vec(vec![]);
    assert!(StreamExt::next(&mut stream).await.is_none());
}

#[tokio::test]
async fn event_stream_incremental_next() {
    let mut stream = EventStream::from_vec(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}]);
    assert!(matches!(
        StreamExt::next(&mut stream).await.unwrap(),
        StreamEvent::Ping {}
    ));
    assert!(matches!(
        StreamExt::next(&mut stream).await.unwrap(),
        StreamEvent::MessageStop {}
    ));
    assert!(StreamExt::next(&mut stream).await.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. request_to_work_order conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_task_from_last_text_message() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "First".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "OK".into() }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Second question".into(),
                }],
            },
        ],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Second question");
}

#[test]
fn work_order_fallback_when_no_text_in_last_message() {
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 100,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_r".into(),
                content: Some("output".into()),
                is_error: None,
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Claude shim request");
}

#[test]
fn work_order_model_set_from_request() {
    let req = simple_request("x");
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn work_order_vendor_fields_when_temperature_set() {
    let mut req = simple_request("x");
    req.temperature = Some(0.8);
    req.max_tokens = 2048;
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.8))
    );
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(2048))
    );
}

#[test]
fn work_order_stop_sequences_in_vendor() {
    let mut req = simple_request("x");
    req.temperature = Some(0.5);
    req.stop_sequences = Some(vec!["STOP".into(), "END".into()]);
    let wo = request_to_work_order(&req);
    let stops = wo.config.vendor.get("stop_sequences").unwrap();
    assert_eq!(stops.as_array().unwrap().len(), 2);
}

#[test]
fn work_order_no_vendor_without_temperature() {
    let req = simple_request("x");
    let wo = request_to_work_order(&req);
    assert!(!wo.config.vendor.contains_key("temperature"));
}

#[test]
fn work_order_from_empty_messages_still_works() {
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 100,
        messages: vec![],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Claude shim request");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. response_from_events conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_from_events_empty_no_content_no_stop() {
    let resp = response_from_events(&[], "model-x", None);
    assert!(resp.content.is_empty());
    assert!(resp.stop_reason.is_none());
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.response_type, "message");
}

#[test]
fn response_from_events_text_event_yields_text_block() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "Hello world".into(),
    })];
    let resp = response_from_events(&events, "test", None);
    assert_eq!(resp.content.len(), 1);
    assert!(matches!(&resp.content[0], ContentBlock::Text { text } if text == "Hello world"));
}

#[test]
fn response_from_events_tool_call_yields_tool_use_block() {
    let events = vec![make_event(AgentEventKind::ToolCall {
        tool_name: "bash".into(),
        tool_use_id: Some("tu_1".into()),
        parent_tool_use_id: None,
        input: json!({"cmd": "ls"}),
    })];
    let resp = response_from_events(&events, "test", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "bash");
            assert_eq!(input, &json!({"cmd": "ls"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn response_from_events_thinking_event_yields_thinking_block() {
    let events = vec![make_event_ext(
        AgentEventKind::AssistantMessage {
            text: "Reasoning...".into(),
        },
        thinking_ext_with_sig("sig_abc"),
    )];
    let resp = response_from_events(&events, "test", None);
    match &resp.content[0] {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "Reasoning...");
            assert_eq!(signature.as_deref(), Some("sig_abc"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn response_from_events_thinking_no_sig() {
    let events = vec![make_event_ext(
        AgentEventKind::AssistantMessage {
            text: "Just thinking".into(),
        },
        thinking_ext(),
    )];
    let resp = response_from_events(&events, "test", None);
    match &resp.content[0] {
        ContentBlock::Thinking { signature, .. } => assert!(signature.is_none()),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn response_from_events_run_completed_sets_end_turn() {
    let events = vec![
        make_event(AgentEventKind::AssistantMessage {
            text: "done".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "ok".into(),
        }),
    ];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn response_from_events_tool_call_overrides_end_turn() {
    let events = vec![
        make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn response_from_events_ignores_irrelevant_event_kinds() {
    let events = vec![
        make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_event(AgentEventKind::Warning {
            message: "warn".into(),
        }),
    ];
    let resp = response_from_events(&events, "m", None);
    assert!(resp.content.is_empty());
}

#[test]
fn response_from_events_with_usage() {
    let usage = ClaudeUsage {
        input_tokens: 100,
        output_tokens: 250,
        cache_creation_input_tokens: Some(10),
        cache_read_input_tokens: Some(5),
    };
    let resp = response_from_events(&[], "m", Some(&usage));
    assert_eq!(resp.usage.input_tokens, 100);
    assert_eq!(resp.usage.output_tokens, 250);
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(10));
    assert_eq!(resp.usage.cache_read_input_tokens, Some(5));
}

#[test]
fn response_from_events_id_starts_with_msg() {
    let resp = response_from_events(&[], "m", None);
    assert!(resp.id.starts_with("msg_"));
}

#[test]
fn response_from_events_content_only_defaults_end_turn() {
    let events = vec![make_event(AgentEventKind::AssistantMessage {
        text: "hi".into(),
    })];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn response_from_events_multiple_text_events() {
    let events = vec![
        make_event(AgentEventKind::AssistantMessage {
            text: "Part 1".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "Part 2".into(),
        }),
    ];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.content.len(), 2);
}

#[test]
fn response_from_events_mixed_thinking_and_text() {
    let events = vec![
        make_event_ext(
            AgentEventKind::AssistantMessage {
                text: "Thinking...".into(),
            },
            thinking_ext(),
        ),
        make_event(AgentEventKind::AssistantMessage {
            text: "Answer".into(),
        }),
    ];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.content.len(), 2);
    assert!(matches!(&resp.content[0], ContentBlock::Thinking { .. }));
    assert!(matches!(&resp.content[1], ContentBlock::Text { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. IR↔Claude message conversion bidirectional
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_to_ir_simple_user_text() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "hello".into(),
        }],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "user");
    assert_eq!(claude_msg.content, "hello");
}

#[test]
fn message_to_ir_simple_assistant_text() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "Sure!".into(),
        }],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "assistant");
    assert_eq!(claude_msg.content, "Sure!");
}

#[test]
fn message_to_ir_empty_content_list() {
    let msg = Message {
        role: Role::User,
        content: vec![],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "user");
    assert!(claude_msg.content.is_empty());
}

#[test]
fn message_to_ir_structured_content_serialized_as_json() {
    let msg = Message {
        role: Role::User,
        content: vec![
            ContentBlock::Text {
                text: "Look".into(),
            },
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "abc".into(),
                },
            },
        ],
    };
    let claude_msg = message_to_ir(&msg);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msg.content).unwrap();
    assert_eq!(blocks.len(), 2);
}

#[test]
fn message_to_ir_tool_result_is_structured() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("result".into()),
            is_error: None,
        }],
    };
    let claude_msg = message_to_ir(&msg);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msg.content).unwrap();
    assert_eq!(blocks.len(), 1);
}

#[test]
fn lowering_to_ir_user_text_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello".into(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");

    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello");
}

#[test]
fn lowering_to_ir_assistant_text_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: "OK".into(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content, "OK");
}

#[test]
fn lowering_system_prompt_becomes_system_message() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "hi".into(),
    }];
    let conv = lowering::to_ir(&msgs, Some("Be concise"));
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "Be concise");
}

#[test]
fn lowering_empty_system_prompt_skipped() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "hi".into(),
    }];
    let conv = lowering::to_ir(&msgs, Some(""));
    assert_eq!(conv.len(), 1);
}

#[test]
fn lowering_extract_system_prompt() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    assert_eq!(
        lowering::extract_system_prompt(&conv).as_deref(),
        Some("instructions")
    );
}

#[test]
fn lowering_system_messages_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn lowering_tool_use_to_ir_and_back() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read".into(),
        input: json!({"path": "x.rs"}),
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "read");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }

    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    assert!(matches!(&parsed[0], ClaudeContentBlock::ToolUse { .. }));
}

#[test]
fn lowering_tool_result_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("data".into()),
        is_error: Some(true),
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(*is_error, Some(true));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn lowering_thinking_block_roundtrip() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: None,
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Thinking { text } if text == "hmm"
    ));
}

#[test]
fn lowering_image_base64_to_ir() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Image { media_type, .. } if media_type == "image/png"
    ));
}

#[test]
fn lowering_image_url_becomes_text() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Text { text } => assert!(text.contains("example.com")),
        other => panic!("expected Text placeholder, got {other:?}"),
    }
}

#[test]
fn lowering_empty_messages_roundtrip() {
    let conv = lowering::to_ir(&[], None);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Claude-specific capability mapping
// ═══════════════════════════════════════════════════════════════════════════

fn support_level_json(
    m: &std::collections::BTreeMap<abp_core::Capability, abp_core::SupportLevel>,
    cap: abp_core::Capability,
) -> serde_json::Value {
    serde_json::to_value(m.get(&cap).expect("capability missing")).unwrap()
}

#[test]
fn capability_manifest_streaming_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::Streaming),
        json!("native")
    );
}

#[test]
fn capability_manifest_tool_read_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::ToolRead),
        json!("native")
    );
}

#[test]
fn capability_manifest_tool_write_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::ToolWrite),
        json!("native")
    );
}

#[test]
fn capability_manifest_tool_edit_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::ToolEdit),
        json!("native")
    );
}

#[test]
fn capability_manifest_tool_bash_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::ToolBash),
        json!("native")
    );
}

#[test]
fn capability_manifest_mcp_client_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::McpClient),
        json!("native")
    );
}

#[test]
fn capability_manifest_mcp_server_unsupported() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::McpServer),
        json!("unsupported")
    );
}

#[test]
fn capability_manifest_checkpointing_emulated() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::Checkpointing),
        json!("emulated")
    );
}

#[test]
fn capability_manifest_structured_output_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::StructuredOutputJsonSchema),
        json!("native")
    );
}

#[test]
fn capability_manifest_hooks_native() {
    let m = dialect::capability_manifest();
    assert_eq!(
        support_level_json(&m, abp_core::Capability::HooksPreToolUse),
        json!("native")
    );
    assert_eq!(
        support_level_json(&m, abp_core::Capability::HooksPostToolUse),
        json!("native")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Error handling (API errors, rate limits, auth)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn shim_error_invalid_request_display() {
    let err = ShimError::InvalidRequest("missing model".into());
    assert_eq!(err.to_string(), "invalid request: missing model");
}

#[test]
fn shim_error_api_error_display() {
    let err = ShimError::ApiError {
        error_type: "rate_limit_error".into(),
        message: "Too many requests".into(),
    };
    let s = err.to_string();
    assert!(s.contains("rate_limit_error"));
    assert!(s.contains("Too many requests"));
}

#[test]
fn shim_error_internal_display() {
    let err = ShimError::Internal("oops".into());
    assert_eq!(err.to_string(), "internal: oops");
}

#[tokio::test]
async fn create_empty_messages_returns_invalid_request() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "t".into(),
        max_tokens: 100,
        messages: vec![],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::InvalidRequest(_)));
}

#[tokio::test]
async fn create_stream_empty_messages_returns_invalid_request() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "t".into(),
        max_tokens: 100,
        messages: vec![],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let err = client.create_stream(req).await.unwrap_err();
    assert!(matches!(err, ShimError::InvalidRequest(_)));
}

#[tokio::test]
async fn custom_handler_rate_limit_error() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        Err(ShimError::ApiError {
            error_type: "rate_limit_error".into(),
            message: "Rate limited".into(),
        })
    }));
    let err = client.create(simple_request("test")).await.unwrap_err();
    match err {
        ShimError::ApiError {
            error_type,
            message,
        } => {
            assert_eq!(error_type, "rate_limit_error");
            assert_eq!(message, "Rate limited");
        }
        other => panic!("expected ApiError, got {other:?}"),
    }
}

#[tokio::test]
async fn custom_handler_auth_error() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        Err(ShimError::ApiError {
            error_type: "authentication_error".into(),
            message: "Invalid API key".into(),
        })
    }));
    let err = client.create(simple_request("test")).await.unwrap_err();
    match err {
        ShimError::ApiError { error_type, .. } => {
            assert_eq!(error_type, "authentication_error");
        }
        other => panic!("expected ApiError, got {other:?}"),
    }
}

#[tokio::test]
async fn custom_handler_internal_error() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        Err(ShimError::Internal("pipeline crash".into()))
    }));
    let err = client.create(simple_request("test")).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn custom_stream_handler_error_propagates() {
    let mut client = AnthropicClient::new();
    client.set_stream_handler(Box::new(|_| Err(ShimError::InvalidRequest("nope".into()))));
    let err = client.create_stream(simple_request("x")).await.unwrap_err();
    assert!(matches!(err, ShimError::InvalidRequest(_)));
}

#[test]
fn api_error_serde_roundtrip() {
    let err = ApiError {
        error_type: "overloaded_error".into(),
        message: "Server busy".into(),
    };
    let j = serde_json::to_string(&err).unwrap();
    let back: ApiError = serde_json::from_str(&j).unwrap();
    assert_eq!(err, back);
}

#[test]
fn api_error_has_type_field_in_json() {
    let err = ApiError {
        error_type: "invalid_request_error".into(),
        message: "bad".into(),
    };
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["type"], "invalid_request_error");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Model configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_canonical_model_adds_prefix() {
    assert_eq!(
        dialect::to_canonical_model("claude-sonnet-4-20250514"),
        "anthropic/claude-sonnet-4-20250514"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        dialect::from_canonical_model("anthropic/claude-opus-4-20250514"),
        "claude-opus-4-20250514"
    );
}

#[test]
fn from_canonical_model_no_prefix_passthrough() {
    assert_eq!(
        dialect::from_canonical_model("claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn canonical_model_roundtrip() {
    let orig = "claude-opus-4-20250514";
    let canonical = dialect::to_canonical_model(orig);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, orig);
}

#[test]
fn is_known_model_sonnet_4() {
    assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
}

#[test]
fn is_known_model_opus_4() {
    assert!(dialect::is_known_model("claude-opus-4-20250514"));
}

#[test]
fn is_known_model_haiku_35() {
    assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
}

#[test]
fn is_known_model_latest_variants() {
    assert!(dialect::is_known_model("claude-sonnet-4-latest"));
    assert!(dialect::is_known_model("claude-opus-4-latest"));
    assert!(dialect::is_known_model("claude-3-5-haiku-latest"));
    assert!(dialect::is_known_model("claude-4-latest"));
}

#[test]
fn is_known_model_claude_4() {
    assert!(dialect::is_known_model("claude-4-20250714"));
}

#[test]
fn is_known_model_unknown_returns_false() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model(""));
    assert!(!dialect::is_known_model("claude-999"));
}

#[test]
fn default_model_is_known() {
    assert!(dialect::is_known_model(dialect::DEFAULT_MODEL));
}

#[test]
fn dialect_version_is_set() {
    assert_eq!(dialect::DIALECT_VERSION, "claude/v0.1");
}

#[test]
fn default_config_sensible() {
    let cfg = dialect::ClaudeConfig::default();
    assert!(cfg.base_url.contains("anthropic.com"));
    assert!(cfg.model.contains("claude"));
    assert!(cfg.max_tokens > 0);
    assert!(cfg.api_key.is_empty());
    assert!(cfg.system_prompt.is_none());
    assert!(cfg.thinking.is_none());
}

#[test]
fn client_default_model() {
    let client = AnthropicClient::new();
    let dbg = format!("{client:?}");
    assert!(dbg.contains("claude-sonnet-4-20250514"));
}

#[test]
fn client_with_model() {
    let client = AnthropicClient::with_model("claude-opus-4-20250514");
    let dbg = format!("{client:?}");
    assert!(dbg.contains("claude-opus-4-20250514"));
}

#[test]
fn client_default_and_new_equivalent() {
    let d1 = format!("{:?}", AnthropicClient::new());
    let d2 = format!("{:?}", AnthropicClient::default());
    assert_eq!(d1, d2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. System message handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_prompt_in_claude_request() {
    let mut req = simple_request("hi");
    req.system = Some("Be helpful".into());
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.system.as_deref(), Some("Be helpful"));
}

#[test]
fn system_prompt_none_in_claude_request() {
    let req = simple_request("hi");
    let claude_req = request_to_claude(&req);
    assert!(claude_req.system.is_none());
}

#[test]
fn system_prompt_not_in_messages_of_claude_request() {
    let mut req = simple_request("hi");
    req.system = Some("You are helpful".into());
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages.len(), 1);
    assert_eq!(claude_req.messages[0].role, "user");
}

#[test]
fn system_block_text_serde() {
    let block = dialect::ClaudeSystemBlock::Text {
        text: "System prompt".into(),
        cache_control: None,
    };
    let j = serde_json::to_string(&block).unwrap();
    let back: dialect::ClaudeSystemBlock = serde_json::from_str(&j).unwrap();
    assert_eq!(block, back);
}

#[test]
fn system_block_with_cache_control() {
    let block = dialect::ClaudeSystemBlock::Text {
        text: "System prompt".into(),
        cache_control: Some(dialect::ClaudeCacheControl::ephemeral()),
    };
    let j = serde_json::to_string(&block).unwrap();
    let back: dialect::ClaudeSystemBlock = serde_json::from_str(&j).unwrap();
    assert_eq!(block, back);
}

#[test]
fn cache_control_ephemeral() {
    let cc = dialect::ClaudeCacheControl::ephemeral();
    assert_eq!(cc.cache_type, "ephemeral");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Tool use/result cycles
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_canonical_to_claude_and_back() {
    let canonical = dialect::CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        }),
    };
    let claude_def = dialect::tool_def_to_claude(&canonical);
    assert_eq!(claude_def.name, "read_file");
    assert_eq!(claude_def.input_schema, canonical.parameters_schema);
    let back = dialect::tool_def_from_claude(&claude_def);
    assert_eq!(canonical, back);
}

#[test]
fn map_tool_result_success() {
    let msg = dialect::map_tool_result("tu_1", "output data", false);
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
fn map_tool_result_error() {
    let msg = dialect::map_tool_result("tu_err", "ENOENT", true);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(*is_error, Some(true));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[tokio::test]
async fn tool_use_response_via_handler() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|req| {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu_w".into()),
            parent_tool_use_id: None,
            input: json!({"path": "x.txt", "content": "data"}),
        })];
        Ok(response_from_events(&events, &req.model, None))
    }));
    let resp = client.create(simple_request("write")).await.unwrap();
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    assert!(matches!(&resp.content[0], ContentBlock::ToolUse { name, .. } if name == "write_file"));
}

#[tokio::test]
async fn multiple_tool_uses_in_single_response() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|req| {
        let events = vec![
            make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_a".into()),
                parent_tool_use_id: None,
                input: json!({"path": "a.rs"}),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_b".into()),
                parent_tool_use_id: None,
                input: json!({"path": "b.rs"}),
            }),
        ];
        Ok(response_from_events(&events, &req.model, None))
    }));
    let resp = client.create(simple_request("read both")).await.unwrap();
    assert_eq!(resp.content.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Multi-turn conversations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_request_to_claude() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "Hi".into() }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Hello!".into(),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: "Bye".into() }],
            },
        ],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages.len(), 3);
    assert_eq!(claude_req.messages[0].role, "user");
    assert_eq!(claude_req.messages[1].role, "assistant");
    assert_eq!(claude_req.messages[2].role, "user");
}

#[test]
fn multi_turn_with_tool_cycle() {
    let msgs = vec![
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Do something".into(),
            }],
        },
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: json!({"cmd": "ls"}),
            }],
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: Some("file1.txt\nfile2.txt".into()),
                is_error: Some(false),
            }],
        },
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "I found 2 files.".into(),
            }],
        },
    ];
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 4096,
        messages: msgs,
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages.len(), 4);
}

#[test]
fn multi_turn_lowering_roundtrip() {
    let msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Q1".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "A1".into(),
        },
        ClaudeMessage {
            role: "user".into(),
            content: "Q2".into(),
        },
    ];
    let conv = lowering::to_ir(&msgs, Some("system prompt"));
    assert_eq!(conv.len(), 4); // system + 3 messages
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 3); // system skipped
    assert_eq!(back[0].content, "Q1");
    assert_eq!(back[1].content, "A1");
    assert_eq!(back[2].content, "Q2");
}

#[tokio::test]
async fn multi_turn_client_roundtrip() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "2+2?".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "4".into() }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "3+3?".into(),
                }],
            },
        ],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.role, "assistant");
    assert!(!resp.content.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Content block IR roundtrip fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_fidelity_text_unicode() {
    let block = ContentBlock::Text {
        text: "Ω ∑ π 日本語 🦀".into(),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn ir_fidelity_text_multiline() {
    let block = ContentBlock::Text {
        text: "line1\nline2\n\nline4".into(),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn ir_fidelity_text_empty() {
    let block = ContentBlock::Text {
        text: String::new(),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn ir_fidelity_text_special_chars() {
    let block = ContentBlock::Text {
        text: "Hello \"world\" & <tag> \t\r\n".into(),
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn ir_fidelity_tool_use_null_input() {
    let block = ContentBlock::ToolUse {
        id: "tu_n".into(),
        name: "noop".into(),
        input: serde_json::Value::Null,
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn ir_fidelity_tool_use_deeply_nested() {
    let input = json!({"a": {"b": {"c": {"d": {"e": "deep"}}}}});
    let block = ContentBlock::ToolUse {
        id: "tu_deep".into(),
        name: "deep".into(),
        input,
    };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn ir_fidelity_large_text() {
    let big = "x".repeat(50_000);
    let block = ContentBlock::Text { text: big };
    assert_eq!(content_block_from_ir(&content_block_to_ir(&block)), block);
}

#[test]
fn ir_fidelity_image_various_mime_types() {
    for mime in &[
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/svg+xml",
    ] {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: (*mime).into(),
                data: "data_here".into(),
            },
        };
        assert_eq!(
            content_block_from_ir(&content_block_to_ir(&block)),
            block,
            "failed for mime type {mime}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Metadata and usage extraction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_serde_roundtrip() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 200,
        cache_creation_input_tokens: Some(50),
        cache_read_input_tokens: Some(30),
    };
    let j = serde_json::to_string(&usage).unwrap();
    let back: Usage = serde_json::from_str(&j).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn usage_cache_fields_skipped_when_none() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 20,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let j = serde_json::to_string(&usage).unwrap();
    assert!(!j.contains("cache_creation"));
    assert!(!j.contains("cache_read"));
}

#[test]
fn usage_zero_values() {
    let usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_input_tokens: Some(0),
        cache_read_input_tokens: Some(0),
    };
    let j = serde_json::to_string(&usage).unwrap();
    let back: Usage = serde_json::from_str(&j).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn claude_usage_to_shim_usage_via_response() {
    let claude_resp = ClaudeResponse {
        id: "msg_u".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: None,
        usage: Some(ClaudeUsage {
            input_tokens: 42,
            output_tokens: 84,
            cache_creation_input_tokens: Some(7),
            cache_read_input_tokens: Some(3),
        }),
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.usage.input_tokens, 42);
    assert_eq!(resp.usage.output_tokens, 84);
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(7));
    assert_eq!(resp.usage.cache_read_input_tokens, Some(3));
}

#[tokio::test]
async fn streaming_usage_in_message_delta() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("test")).await.unwrap();
    let events = stream.collect_all().await;
    let usage = events.iter().find_map(|e| match e {
        StreamEvent::MessageDelta { usage, .. } => usage.clone(),
        _ => None,
    });
    assert!(usage.is_some());
    let u = usage.unwrap();
    assert!(u.input_tokens > 0 || u.output_tokens > 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream event conversion from Claude SDK types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_from_claude_message_start() {
    let resp = sample_claude_response("hi");
    let ev = stream_event_from_claude(&ClaudeStreamEvent::MessageStart { message: resp });
    assert!(matches!(ev, StreamEvent::MessageStart { .. }));
}

#[test]
fn stream_event_from_claude_content_block_start_text() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    });
    match ev {
        StreamEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            assert_eq!(index, 0);
            assert!(matches!(content_block, ContentBlock::Text { .. }));
        }
        other => panic!("expected ContentBlockStart, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_content_block_start_tool_use() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::ContentBlockStart {
        index: 1,
        content_block: ClaudeContentBlock::ToolUse {
            id: "tu_s".into(),
            name: "grep".into(),
            input: json!({}),
        },
    });
    match ev {
        StreamEvent::ContentBlockStart {
            content_block: ContentBlock::ToolUse { name, .. },
            ..
        } => assert_eq!(name, "grep"),
        other => panic!("expected ContentBlockStart/ToolUse, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_text_delta() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "chunk".into(),
        },
    });
    match ev {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::TextDelta { text },
            ..
        } => assert_eq!(text, "chunk"),
        other => panic!("expected TextDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_input_json_delta() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: "{\"key\":".into(),
        },
    });
    match ev {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::InputJsonDelta { partial_json },
            ..
        } => assert_eq!(partial_json, "{\"key\":"),
        other => panic!("expected InputJsonDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_thinking_delta() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "hmm".into(),
        },
    });
    match ev {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::ThinkingDelta { thinking },
            ..
        } => assert_eq!(thinking, "hmm"),
        other => panic!("expected ThinkingDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_signature_delta() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::SignatureDelta {
            signature: "sig_p".into(),
        },
    });
    match ev {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::SignatureDelta { signature },
            ..
        } => assert_eq!(signature, "sig_p"),
        other => panic!("expected SignatureDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_content_block_stop() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::ContentBlockStop { index: 5 });
    assert!(matches!(ev, StreamEvent::ContentBlockStop { index: 5 }));
}

#[test]
fn stream_event_from_claude_message_delta_with_usage() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 25,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    });
    match ev {
        StreamEvent::MessageDelta { delta, usage } => {
            assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
            assert!(usage.is_some());
            assert_eq!(usage.unwrap().output_tokens, 25);
        }
        other => panic!("expected MessageDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_message_delta_no_usage() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("max_tokens".into()),
            stop_sequence: None,
        },
        usage: None,
    });
    match ev {
        StreamEvent::MessageDelta { usage, .. } => assert!(usage.is_none()),
        other => panic!("expected MessageDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_message_stop() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::MessageStop {});
    assert!(matches!(ev, StreamEvent::MessageStop {}));
}

#[test]
fn stream_event_from_claude_ping() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::Ping {});
    assert!(matches!(ev, StreamEvent::Ping {}));
}

#[test]
fn stream_event_from_claude_error() {
    let ev = stream_event_from_claude(&ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "authentication_error".into(),
            message: "Invalid key".into(),
        },
    });
    match ev {
        StreamEvent::Error { error } => {
            assert_eq!(error.error_type, "authentication_error");
            assert_eq!(error.message, "Invalid key");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Stop reason mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stop_reason_parse_all_known() {
    assert_eq!(
        dialect::parse_stop_reason("end_turn"),
        Some(dialect::ClaudeStopReason::EndTurn)
    );
    assert_eq!(
        dialect::parse_stop_reason("tool_use"),
        Some(dialect::ClaudeStopReason::ToolUse)
    );
    assert_eq!(
        dialect::parse_stop_reason("max_tokens"),
        Some(dialect::ClaudeStopReason::MaxTokens)
    );
    assert_eq!(
        dialect::parse_stop_reason("stop_sequence"),
        Some(dialect::ClaudeStopReason::StopSequence)
    );
}

#[test]
fn stop_reason_parse_unknown() {
    assert!(dialect::parse_stop_reason("unknown").is_none());
    assert!(dialect::parse_stop_reason("").is_none());
}

#[test]
fn stop_reason_map_roundtrip_all() {
    for reason in [
        dialect::ClaudeStopReason::EndTurn,
        dialect::ClaudeStopReason::ToolUse,
        dialect::ClaudeStopReason::MaxTokens,
        dialect::ClaudeStopReason::StopSequence,
    ] {
        let s = dialect::map_stop_reason(reason);
        let back = dialect::parse_stop_reason(s);
        assert_eq!(back, Some(reason));
    }
}

#[test]
fn stop_reason_serde_roundtrip() {
    let r = dialect::ClaudeStopReason::EndTurn;
    let j = serde_json::to_string(&r).unwrap();
    let back: dialect::ClaudeStopReason = serde_json::from_str(&j).unwrap();
    assert_eq!(r, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_text_delta_roundtrip() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "hello".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(
        ext.get("dialect"),
        Some(&serde_json::Value::String("claude".into()))
    );
    let back = dialect::from_passthrough_event(&wrapped);
    assert_eq!(back.as_ref(), Some(&event));
}

#[test]
fn passthrough_message_start_roundtrip() {
    let event = ClaudeStreamEvent::MessageStart {
        message: sample_claude_response("hi"),
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let back = dialect::from_passthrough_event(&wrapped);
    assert_eq!(back.as_ref(), Some(&event));
}

#[test]
fn passthrough_ping_roundtrip() {
    let event = ClaudeStreamEvent::Ping {};
    let wrapped = dialect::to_passthrough_event(&event);
    let back = dialect::from_passthrough_event(&wrapped);
    assert_eq!(back.as_ref(), Some(&event));
}

#[test]
fn passthrough_error_roundtrip() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "overloaded_error".into(),
            message: "Busy".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let back = dialect::from_passthrough_event(&wrapped);
    assert_eq!(back.as_ref(), Some(&event));
}

#[test]
fn verify_passthrough_fidelity_multiple_events() {
    let events = vec![
        ClaudeStreamEvent::MessageStart {
            message: sample_claude_response("hi"),
        },
        ClaudeStreamEvent::Ping {},
        ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Text {
                text: String::new(),
            },
        },
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "world".into(),
            },
        },
        ClaudeStreamEvent::ContentBlockStop { index: 0 },
        ClaudeStreamEvent::MessageDelta {
            delta: ClaudeMessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: None,
        },
        ClaudeStreamEvent::MessageStop {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn from_passthrough_event_no_ext_returns_none() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: None,
    };
    assert!(dialect::from_passthrough_event(&event).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// ThinkingConfig
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_config_new_sets_type_and_budget() {
    let tc = ThinkingConfig::new(4096);
    assert_eq!(tc.thinking_type, "enabled");
    assert_eq!(tc.budget_tokens, 4096);
}

#[test]
fn thinking_config_serde_roundtrip() {
    let tc = ThinkingConfig::new(8192);
    let j = serde_json::to_string(&tc).unwrap();
    let back: ThinkingConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn thinking_config_in_request_propagates_to_claude_req() {
    let mut req = simple_request("think");
    req.thinking = Some(ThinkingConfig::new(2048));
    let claude_req = request_to_claude(&req);
    let tc = claude_req.thinking.unwrap();
    assert_eq!(tc.budget_tokens, 2048);
}

// ═══════════════════════════════════════════════════════════════════════════
// End-to-end client roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn e2e_simple_create() {
    let client = AnthropicClient::new();
    let resp = client.create(simple_request("hello")).await.unwrap();
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert!(!resp.content.is_empty());
    assert!(resp.stop_reason.is_some());
    assert!(resp.id.starts_with("msg_"));
}

#[tokio::test]
async fn e2e_simple_stream() {
    let client = AnthropicClient::new();
    let stream = client
        .create_stream(simple_request("stream me"))
        .await
        .unwrap();
    let events = stream.collect_all().await;
    assert!(!events.is_empty());
    assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(
        events.last().unwrap(),
        StreamEvent::MessageStop {}
    ));
}

#[tokio::test]
async fn e2e_model_preserved_in_response() {
    let client = AnthropicClient::new();
    let mut req = simple_request("t");
    req.model = "claude-opus-4-20250514".into();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

#[tokio::test]
async fn e2e_custom_handler_success() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Custom reply".into(),
        })];
        Ok(response_from_events(&events, "custom-model", None))
    }));
    let resp = client.create(simple_request("x")).await.unwrap();
    assert_eq!(resp.model, "custom-model");
    match &resp.content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Custom reply"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[tokio::test]
async fn e2e_custom_stream_handler_success() {
    let mut client = AnthropicClient::new();
    client.set_stream_handler(Box::new(|_| {
        Ok(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}])
    }));
    let stream = client.create_stream(simple_request("x")).await.unwrap();
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Role serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn role_user_serde_roundtrip() {
    let j = serde_json::to_string(&Role::User).unwrap();
    assert_eq!(j, "\"user\"");
    assert_eq!(serde_json::from_str::<Role>(&j).unwrap(), Role::User);
}

#[test]
fn role_assistant_serde_roundtrip() {
    let j = serde_json::to_string(&Role::Assistant).unwrap();
    assert_eq!(j, "\"assistant\"");
    assert_eq!(serde_json::from_str::<Role>(&j).unwrap(), Role::Assistant);
}

// ═══════════════════════════════════════════════════════════════════════════
// MessageDeltaPayload edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_delta_payload_all_none() {
    let p = MessageDeltaPayload {
        stop_reason: None,
        stop_sequence: None,
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: MessageDeltaPayload = serde_json::from_str(&j).unwrap();
    assert_eq!(p, back);
}

#[test]
fn message_delta_payload_with_stop_sequence() {
    let p = MessageDeltaPayload {
        stop_reason: Some("stop_sequence".into()),
        stop_sequence: Some("END".into()),
    };
    let j = serde_json::to_string(&p).unwrap();
    let back: MessageDeltaPayload = serde_json::from_str(&j).unwrap();
    assert_eq!(p, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Dialect map_work_order
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_map_work_order_basic() {
    let wo = abp_core::WorkOrderBuilder::new("Refactor auth").build();
    let cfg = dialect::ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(req.messages[0].content.contains("Refactor auth"));
}

#[test]
fn dialect_map_work_order_model_override() {
    let wo = abp_core::WorkOrderBuilder::new("task")
        .model("claude-opus-4-20250514")
        .build();
    let cfg = dialect::ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "claude-opus-4-20250514");
}

#[test]
fn dialect_map_work_order_uses_config_system_prompt() {
    let wo = abp_core::WorkOrderBuilder::new("task").build();
    let cfg = dialect::ClaudeConfig {
        system_prompt: Some("Be terse".into()),
        ..Default::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.system.as_deref(), Some("Be terse"));
}

#[test]
fn dialect_map_work_order_thinking_from_config() {
    let wo = abp_core::WorkOrderBuilder::new("task").build();
    let cfg = dialect::ClaudeConfig {
        thinking: Some(ThinkingConfig::new(4096)),
        ..Default::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.thinking.is_some());
    assert_eq!(req.thinking.unwrap().budget_tokens, 4096);
}

// ═══════════════════════════════════════════════════════════════════════════
// Dialect map_response
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_map_response_text() {
    let resp = sample_claude_response("Hello!");
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn dialect_map_response_tool_use() {
    let resp = ClaudeResponse {
        id: "msg_tu".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read".into(),
            input: json!({"path": "x"}),
        }],
        stop_reason: Some("tool_use".into()),
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn dialect_map_response_thinking_has_ext() {
    let resp = ClaudeResponse {
        id: "msg_th".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Thinking {
            thinking: "reasoning".into(),
            signature: Some("sig".into()),
        }],
        stop_reason: None,
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert_eq!(ext.get("thinking"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        ext.get("signature"),
        Some(&serde_json::Value::String("sig".into()))
    );
}

#[test]
fn dialect_map_response_image_block_ignored() {
    let resp = ClaudeResponse {
        id: "msg_img".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
        }],
        stop_reason: None,
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn dialect_map_response_tool_result() {
    let resp = ClaudeResponse {
        id: "msg_tr".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("data".into()),
            is_error: Some(false),
        }],
        stop_reason: None,
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Dialect map_stream_event
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_map_stream_event_text_delta() {
    let ev = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta { text: "hi".into() },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "hi"
    ));
}

#[test]
fn dialect_map_stream_event_message_start() {
    let ev = ClaudeStreamEvent::MessageStart {
        message: sample_claude_response("x"),
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn dialect_map_stream_event_message_stop() {
    let events = dialect::map_stream_event(&ClaudeStreamEvent::MessageStop {});
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn dialect_map_stream_event_error() {
    let ev = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "overloaded_error".into(),
            message: "Busy".into(),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert!(message.contains("overloaded_error"));
            assert!(message.contains("Busy"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_thinking_delta() {
    let ev = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "think".into(),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert_eq!(ext.get("thinking"), Some(&serde_json::Value::Bool(true)));
}

#[test]
fn dialect_map_stream_event_tool_use_block_start() {
    let ev = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "bash".into(),
            input: json!({"cmd": "ls"}),
        },
    };
    let events = dialect::map_stream_event(&ev);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn dialect_map_stream_event_ping_empty() {
    let events = dialect::map_stream_event(&ClaudeStreamEvent::Ping {});
    assert!(events.is_empty());
}

#[test]
fn dialect_map_stream_event_content_block_stop_empty() {
    let events = dialect::map_stream_event(&ClaudeStreamEvent::ContentBlockStop { index: 0 });
    assert!(events.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// ImageSource serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn image_source_base64_serde() {
    let src = ImageSource::Base64 {
        media_type: "image/png".into(),
        data: "AAAA".into(),
    };
    let j = serde_json::to_string(&src).unwrap();
    let back: ImageSource = serde_json::from_str(&j).unwrap();
    assert_eq!(src, back);
}

#[test]
fn image_source_url_serde() {
    let src = ImageSource::Url {
        url: "https://example.com/img.jpg".into(),
    };
    let j = serde_json::to_string(&src).unwrap();
    let back: ImageSource = serde_json::from_str(&j).unwrap();
    assert_eq!(src, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Many messages / stress
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn many_messages_conversion() {
    let messages: Vec<Message> = (0..50)
        .map(|i| Message {
            role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            content: vec![ContentBlock::Text {
                text: format!("msg {i}"),
            }],
        })
        .collect();
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 4096,
        messages,
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages.len(), 50);
}
