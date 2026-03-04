// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep public API surface tests for `abp-shim-claude`.
//!
//! 80+ tests covering message creation, content blocks, tool definitions,
//! response handling, streaming, errors, model selection, extended thinking,
//! WorkOrder/Receipt conversions, serde roundtrips, and edge cases.

use std::collections::BTreeMap;

use abp_claude_sdk::dialect::{
    self, ClaudeContentBlock, ClaudeResponse, ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage,
    ThinkingConfig,
};
use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use abp_shim_claude::convert::{
    build_response, content_block_to_event_kind, content_to_text, extract_task, from_agent_event,
    from_receipt, map_role_from_abp, map_role_to_abp, to_work_order, tools_to_vendor_json,
    usage_from_raw,
};
use abp_shim_claude::types::{
    ClaudeContent, ClaudeMessage, ClaudeTool, ClaudeToolChoice, ContentBlock as TypesContentBlock,
    ImageSource as TypesImageSource, MessagesRequest,
};
use abp_shim_claude::{
    AnthropicClient, ApiError, ContentBlock, EventStream, ImageSource, Message,
    MessageDeltaPayload, MessageRequest, MessageResponse, Role, ShimError, StreamDelta,
    StreamEvent, Usage, content_block_from_ir, content_block_to_ir, request_to_claude,
    request_to_work_order, response_from_claude, response_from_events, stream_event_from_claude,
};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn shim_request(text: &str) -> MessageRequest {
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

fn types_request(text: &str) -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: ClaudeContent::Text(text.into()),
        }],
        max_tokens: 4096,
        system: None,
        temperature: None,
        top_p: None,
        top_k: None,
        stream: None,
        tools: None,
        tool_choice: None,
    }
}

fn text_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn tool_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Message create — basic request, all parameters, streaming (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_basic_returns_message_type() {
    let client = AnthropicClient::new();
    let resp = client.create(shim_request("ping")).await.unwrap();
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
}

#[tokio::test]
async fn create_all_parameters_accepted() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 2048,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "all-params test".into(),
            }],
        }],
        system: Some("Be terse.".into()),
        temperature: Some(0.3),
        stop_sequences: Some(vec!["HALT".into()]),
        thinking: Some(ThinkingConfig::new(1024)),
        stream: Some(false),
    };
    let resp = client.create(req).await.unwrap();
    assert!(!resp.content.is_empty());
}

#[tokio::test]
async fn create_stream_returns_ordered_events() {
    let client = AnthropicClient::new();
    let es = client
        .create_stream(shim_request("stream me"))
        .await
        .unwrap();
    let events = es.collect_all().await;
    assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(
        events.last().unwrap(),
        StreamEvent::MessageStop {}
    ));
}

#[tokio::test]
async fn create_preserves_model_in_response() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "claude-opus-4-20250514".into(),
        ..shim_request("model test")
    };
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

#[tokio::test]
async fn create_with_custom_handler_overrides_pipeline() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        Ok(MessageResponse {
            id: "msg_custom".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "custom".into(),
            }],
            model: "test".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        })
    }));
    let resp = client.create(shim_request("hi")).await.unwrap();
    assert_eq!(resp.id, "msg_custom");
}

#[tokio::test]
async fn create_stream_with_custom_handler() {
    let mut client = AnthropicClient::new();
    client.set_stream_handler(Box::new(|_| {
        Ok(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}])
    }));
    let es = client.create_stream(shim_request("hi")).await.unwrap();
    let events = es.collect_all().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn create_response_has_msg_id_prefix() {
    let client = AnthropicClient::new();
    let resp = client.create(shim_request("id check")).await.unwrap();
    assert!(resp.id.starts_with("msg_"));
}

#[test]
fn create_request_serde_preserves_all_fields() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 8192,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "test".into(),
            }],
        }],
        system: Some("system".into()),
        temperature: Some(0.9),
        stop_sequences: Some(vec!["X".into()]),
        thinking: Some(ThinkingConfig::new(512)),
        stream: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: MessageRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.max_tokens, 8192);
    assert_eq!(back.system, req.system);
    assert_eq!(back.temperature, Some(0.9));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. System prompt handling (5 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_prompt_separate_from_messages_in_claude_request() {
    let req = MessageRequest {
        system: Some("You are a compiler.".into()),
        ..shim_request("hello")
    };
    let cr = request_to_claude(&req);
    assert_eq!(cr.system.as_deref(), Some("You are a compiler."));
    assert_eq!(cr.messages.len(), 1);
}

#[test]
fn system_prompt_stored_in_work_order_vendor() {
    let mut req = types_request("hello");
    req.system = Some("system prompt".into());
    let wo = to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("system").and_then(|v| v.as_str()),
        Some("system prompt")
    );
}

#[test]
fn system_prompt_used_as_task_fallback() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![],
        max_tokens: 1024,
        system: Some("fallback system".into()),
        temperature: None,
        top_p: None,
        top_k: None,
        stream: None,
        tools: None,
        tool_choice: None,
    };
    assert_eq!(extract_task(&req), "fallback system");
}

#[test]
fn system_prompt_none_not_in_json() {
    let req = shim_request("test");
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("system").is_none());
}

#[test]
fn system_prompt_none_not_in_work_order_vendor() {
    let req = types_request("test");
    let wo = to_work_order(&req);
    assert!(!wo.config.vendor.contains_key("system"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Content blocks — text, image, tool_use, tool_result (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_json_has_type_text() {
    let b = ContentBlock::Text {
        text: "hello".into(),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "text");
}

#[test]
fn content_block_image_base64_json_has_nested_source() {
    let b = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/webp".into(),
            data: "AAAA".into(),
        },
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "image");
    assert_eq!(v["source"]["type"], "base64");
    assert_eq!(v["source"]["media_type"], "image/webp");
}

#[test]
fn content_block_image_url_json_shape() {
    let b = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://a.b/c.png".into(),
        },
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["source"]["type"], "url");
    assert_eq!(v["source"]["url"], "https://a.b/c.png");
}

#[test]
fn content_block_tool_use_json_has_all_fields() {
    let b = ContentBlock::ToolUse {
        id: "tu_99".into(),
        name: "search".into(),
        input: json!({"q": "hello"}),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "tool_use");
    assert_eq!(v["id"], "tu_99");
    assert_eq!(v["name"], "search");
    assert_eq!(v["input"]["q"], "hello");
}

#[test]
fn content_block_tool_result_json_shape() {
    let b = ContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("result text".into()),
        is_error: Some(false),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["tool_use_id"], "tu_1");
    assert_eq!(v["content"], "result text");
}

#[test]
fn content_block_tool_result_error_flag() {
    let b = ContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("error!".into()),
        is_error: Some(true),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["is_error"], true);
}

#[test]
fn content_block_thinking_json_shape() {
    let b = ContentBlock::Thinking {
        thinking: "reasoning…".into(),
        signature: Some("sig_a".into()),
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["type"], "thinking");
    assert_eq!(v["thinking"], "reasoning…");
    assert_eq!(v["signature"], "sig_a");
}

#[test]
fn content_block_thinking_no_sig_omitted() {
    let b = ContentBlock::Thinking {
        thinking: "x".into(),
        signature: None,
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(!json.contains("signature"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Tool definitions (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_name_description_schema_roundtrip() {
    let tool = ClaudeTool {
        name: "get_weather".into(),
        description: Some("Get the weather".into()),
        input_schema: json!({"type":"object","properties":{"city":{"type":"string"}}}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: ClaudeTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn tool_def_no_description() {
    let tool = ClaudeTool {
        name: "noop".into(),
        description: None,
        input_schema: json!({"type":"object"}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(!json.contains("description"));
    let back: ClaudeTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back.description, None);
}

#[test]
fn tools_to_vendor_json_array() {
    let tools = vec![
        ClaudeTool {
            name: "a".into(),
            description: None,
            input_schema: json!({}),
        },
        ClaudeTool {
            name: "b".into(),
            description: Some("b desc".into()),
            input_schema: json!({"type":"object"}),
        },
    ];
    let v = tools_to_vendor_json(&tools);
    assert!(v.is_array());
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn tool_choice_auto_json() {
    let tc = ClaudeToolChoice::Auto {};
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "auto");
}

#[test]
fn tool_choice_any_json() {
    let tc = ClaudeToolChoice::Any {};
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "any");
}

#[test]
fn tool_choice_specific_json() {
    let tc = ClaudeToolChoice::Tool {
        name: "bash".into(),
    };
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "tool");
    assert_eq!(v["name"], "bash");
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Response handling — content blocks, stop_reason, usage (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_from_claude_text_content() {
    let cr = ClaudeResponse {
        id: "msg_r1".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text {
            text: "reply".into(),
        }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 5,
            output_tokens: 3,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let resp = response_from_claude(&cr);
    assert_eq!(resp.content.len(), 1);
    assert!(matches!(&resp.content[0], ContentBlock::Text { text } if text == "reply"));
    assert_eq!(resp.usage.input_tokens, 5);
}

#[test]
fn response_stop_reason_end_turn_from_events() {
    let events = vec![text_event("done")];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn response_stop_reason_tool_use_from_events() {
    let events = vec![tool_event("bash", "tu_1", json!({}))];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn response_stop_reason_none_when_empty() {
    let resp = response_from_events(&[], "m", None);
    assert!(resp.stop_reason.is_none());
}

#[test]
fn response_usage_passed_through() {
    let u = ClaudeUsage {
        input_tokens: 42,
        output_tokens: 18,
        cache_creation_input_tokens: Some(5),
        cache_read_input_tokens: Some(10),
    };
    let resp = response_from_events(&[text_event("x")], "m", Some(&u));
    assert_eq!(resp.usage.input_tokens, 42);
    assert_eq!(resp.usage.output_tokens, 18);
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(5));
    assert_eq!(resp.usage.cache_read_input_tokens, Some(10));
}

#[test]
fn response_usage_defaults_zero() {
    let resp = response_from_events(&[text_event("x")], "m", None);
    assert_eq!(resp.usage.input_tokens, 0);
    assert_eq!(resp.usage.output_tokens, 0);
}

#[test]
fn response_type_field_is_message() {
    let resp = MessageResponse {
        id: "msg_x".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![],
        model: "m".into(),
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
    assert!(v.get("response_type").is_none());
}

#[test]
fn response_from_claude_multiple_blocks() {
    let cr = ClaudeResponse {
        id: "msg_multi".into(),
        model: "m".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "I'll help.".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_m".into(),
                name: "edit".into(),
                input: json!({}),
            },
        ],
        stop_reason: Some("tool_use".into()),
        usage: None,
    };
    let resp = response_from_claude(&cr);
    assert_eq!(resp.content.len(), 2);
    assert!(matches!(&resp.content[0], ContentBlock::Text { .. }));
    assert!(matches!(&resp.content[1], ContentBlock::ToolUse { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Streaming event types (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_message_start_serde() {
    let ev = StreamEvent::MessageStart {
        message: MessageResponse {
            id: "msg_s".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "m".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage {
                input_tokens: 1,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        },
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("message_start"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_content_block_start_serde() {
    let ev = StreamEvent::ContentBlockStart {
        index: 0,
        content_block: ContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("content_block_start"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_content_block_delta_text_serde() {
    let ev = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::TextDelta { text: "tok".into() },
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("content_block_delta"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_content_block_delta_input_json_serde() {
    let ev = StreamEvent::ContentBlockDelta {
        index: 1,
        delta: StreamDelta::InputJsonDelta {
            partial_json: r#"{"pa"#.into(),
        },
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_content_block_stop_serde() {
    let ev = StreamEvent::ContentBlockStop { index: 2 };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("content_block_stop"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_message_delta_serde() {
    let ev = StreamEvent::MessageDelta {
        delta: MessageDeltaPayload {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&ev).unwrap();
    assert!(json.contains("message_delta"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_message_stop_serde() {
    let ev = StreamEvent::MessageStop {};
    let json = serde_json::to_string(&ev).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_ping_serde() {
    let ev = StreamEvent::Ping {};
    let json = serde_json::to_string(&ev).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_event_error_serde() {
    let ev = StreamEvent::Error {
        error: ApiError {
            error_type: "overloaded_error".into(),
            message: "server busy".into(),
        },
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, back);
}

#[test]
fn stream_claude_sdk_events_map_to_shim() {
    let sdk_events = vec![
        ClaudeStreamEvent::Ping {},
        ClaudeStreamEvent::MessageStop {},
        ClaudeStreamEvent::ContentBlockStop { index: 0 },
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta { text: "hi".into() },
        },
    ];
    for ce in &sdk_events {
        let se = stream_event_from_claude(ce);
        let json = serde_json::to_string(&se).unwrap();
        assert!(!json.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Error responses (7 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_rate_limit_serde() {
    let err = ApiError {
        error_type: "rate_limit_error".into(),
        message: "Too many requests, please slow down.".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
    assert!(json.contains("rate_limit_error"));
}

#[test]
fn error_authentication_serde() {
    let err = ApiError {
        error_type: "authentication_error".into(),
        message: "Invalid API key provided.".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn error_overloaded_serde() {
    let err = ApiError {
        error_type: "overloaded_error".into(),
        message: "Anthropic's API is temporarily overloaded.".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn error_context_too_long_serde() {
    let err = ApiError {
        error_type: "invalid_request_error".into(),
        message: "prompt is too long: 200001 tokens > 200000 maximum".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[tokio::test]
async fn error_empty_messages_create() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        messages: vec![],
        ..shim_request("ignored")
    };
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::InvalidRequest(_)));
}

#[tokio::test]
async fn error_empty_messages_stream() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        messages: vec![],
        ..shim_request("ignored")
    };
    let err = client.create_stream(req).await.unwrap_err();
    assert!(matches!(err, ShimError::InvalidRequest(_)));
}

#[test]
fn error_shim_error_display() {
    let e1 = ShimError::InvalidRequest("bad".into());
    assert!(e1.to_string().contains("bad"));
    let e2 = ShimError::ApiError {
        error_type: "rate_limit_error".into(),
        message: "slow down".into(),
    };
    assert!(e2.to_string().contains("rate_limit_error"));
    let e3 = ShimError::Internal("oops".into());
    assert!(e3.to_string().contains("oops"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Max tokens (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn max_tokens_required_in_request() {
    let req = shim_request("test");
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("max_tokens").is_some());
    assert_eq!(json["max_tokens"], 4096);
}

#[test]
fn max_tokens_forwarded_to_claude_request() {
    let req = MessageRequest {
        max_tokens: 8192,
        ..shim_request("task")
    };
    let cr = request_to_claude(&req);
    assert_eq!(cr.max_tokens, 8192);
}

#[test]
fn max_tokens_stored_in_work_order_vendor() {
    let req = types_request("test");
    let wo = to_work_order(&req);
    assert_eq!(wo.config.vendor.get("max_tokens"), Some(&json!(4096)));
}

#[test]
fn max_tokens_one_is_valid() {
    let req = MessageRequest {
        max_tokens: 1,
        ..shim_request("task")
    };
    let cr = request_to_claude(&req);
    assert_eq!(cr.max_tokens, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Model selection (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn model_sonnet_4_known() {
    assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
}

#[test]
fn model_opus_4_known() {
    assert!(dialect::is_known_model("claude-opus-4-20250514"));
}

#[test]
fn model_haiku_3_5_known() {
    assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
}

#[test]
fn model_sonnet_3_5_known() {
    assert!(dialect::is_known_model("claude-sonnet-3-5-20241022"));
}

#[test]
fn model_canonical_roundtrip() {
    for m in [
        "claude-sonnet-4-20250514",
        "claude-opus-4-20250514",
        "claude-haiku-3-5-20241022",
    ] {
        let canonical = dialect::to_canonical_model(m);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, m, "canonical roundtrip failed for {m}");
    }
}

#[test]
fn model_unknown_not_known() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model("gemini-2.0-flash"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Extended thinking (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_config_serde() {
    let tc = ThinkingConfig::new(2048);
    let json = serde_json::to_string(&tc).unwrap();
    let back: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "enabled");
    assert_eq!(v["budget_tokens"], 2048);
}

#[test]
fn thinking_blocks_response_from_events() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), serde_json::Value::Bool(true));

    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "reasoning".into(),
            },
            ext: Some(ext),
        },
        text_event("answer"),
    ];
    let resp = response_from_events(&events, "m", None);
    assert_eq!(resp.content.len(), 2);
    assert!(matches!(&resp.content[0], ContentBlock::Thinking { .. }));
    assert!(matches!(&resp.content[1], ContentBlock::Text { .. }));
}

#[test]
fn thinking_block_ir_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "step-by-step".into(),
        signature: Some("sig_irt".into()),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn thinking_stream_delta_type() {
    let d = StreamDelta::ThinkingDelta {
        thinking: "hmm".into(),
    };
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["type"], "thinking_delta");
}

#[test]
fn thinking_signature_delta_type() {
    let d = StreamDelta::SignatureDelta {
        signature: "sig_part".into(),
    };
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["type"], "signature_delta");
}

#[test]
fn thinking_config_passes_through_request_pipeline() {
    let req = MessageRequest {
        thinking: Some(ThinkingConfig::new(4096)),
        ..shim_request("think")
    };
    let cr = request_to_claude(&req);
    assert_eq!(cr.thinking.unwrap().budget_tokens, 4096);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Conversion to ABP WorkOrder (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_extracts_model() {
    let req = types_request("hi");
    let wo = to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn work_order_stores_dialect_claude() {
    let req = types_request("hi");
    let wo = to_work_order(&req);
    assert_eq!(wo.config.vendor.get("dialect"), Some(&json!("claude")));
}

#[test]
fn work_order_stores_temperature() {
    let mut req = types_request("hi");
    req.temperature = Some(0.5);
    let wo = to_work_order(&req);
    assert_eq!(wo.config.vendor.get("temperature"), Some(&json!(0.5)));
}

#[test]
fn work_order_stores_tools() {
    let mut req = types_request("hi");
    req.tools = Some(vec![ClaudeTool {
        name: "x".into(),
        description: None,
        input_schema: json!({}),
    }]);
    let wo = to_work_order(&req);
    let tools = wo.config.vendor.get("tools").unwrap();
    assert!(tools.is_array());
}

#[test]
fn work_order_preserves_messages() {
    let mut req = types_request("hello world");
    req.messages.push(ClaudeMessage {
        role: "assistant".into(),
        content: ClaudeContent::Text("hi".into()),
    });
    let wo = to_work_order(&req);
    let msgs = wo.config.vendor.get("messages").unwrap();
    assert!(msgs.is_array());
    assert_eq!(msgs.as_array().unwrap().len(), 2);
}

#[test]
fn work_order_from_shim_request_extracts_task() {
    let req = shim_request("Fix the login bug");
    let wo = request_to_work_order(&req);
    assert!(wo.task.contains("Fix the login bug"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Conversion from ABP Receipt (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_text_to_response() {
    let req = types_request("hi");
    let wo = to_work_order(&req);
    let receipt = ReceiptBuilder::new("claude")
        .outcome(Outcome::Complete)
        .add_trace_event(text_event("Hello!"))
        .usage_raw(json!({"input_tokens": 10, "output_tokens": 5}))
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.type_field, "message");
    assert_eq!(resp.content.len(), 1);
    assert_eq!(resp.usage.input_tokens, 10);
    assert_eq!(resp.usage.output_tokens, 5);
}

#[test]
fn receipt_tool_call_stop_reason() {
    let req = types_request("hi");
    let wo = to_work_order(&req);
    let receipt = ReceiptBuilder::new("claude")
        .outcome(Outcome::Complete)
        .add_trace_event(tool_event("bash", "tu_1", json!({})))
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn receipt_empty_trace_empty_content() {
    let req = types_request("hi");
    let wo = to_work_order(&req);
    let receipt = ReceiptBuilder::new("claude")
        .outcome(Outcome::Complete)
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert!(resp.content.is_empty());
}

#[test]
fn receipt_model_from_work_order() {
    let mut req = types_request("hi");
    req.model = "claude-opus-4-20250514".into();
    let wo = to_work_order(&req);
    let receipt = ReceiptBuilder::new("claude")
        .outcome(Outcome::Complete)
        .add_trace_event(text_event("ok"))
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

#[test]
fn receipt_mixed_text_and_tool() {
    let req = types_request("hi");
    let wo = to_work_order(&req);
    let receipt = ReceiptBuilder::new("claude")
        .outcome(Outcome::Complete)
        .add_trace_event(text_event("I'll check"))
        .add_trace_event(tool_event("read", "tu_2", json!({"path":"a"})))
        .build();
    let resp = from_receipt(&receipt, &wo);
    assert_eq!(resp.content.len(), 2);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn receipt_to_sdk_messages_response() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(30),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        })
        .add_trace_event(text_event("answer"))
        .build();
    use abp_claude_sdk::messages::MessagesResponse as SdkResp;
    let api_resp: SdkResp = receipt.into();
    assert_eq!(api_resp.response_type, "message");
    assert!(api_resp.id.starts_with("msg_"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Serde roundtrip for all types (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_role_user() {
    let r = Role::User;
    let json = serde_json::to_string(&r).unwrap();
    let back: Role = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn serde_roundtrip_role_assistant() {
    let r = Role::Assistant;
    let json = serde_json::to_string(&r).unwrap();
    let back: Role = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn serde_roundtrip_usage() {
    let u = Usage {
        input_tokens: 999,
        output_tokens: 888,
        cache_creation_input_tokens: Some(77),
        cache_read_input_tokens: Some(66),
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

#[test]
fn serde_roundtrip_message_response() {
    let resp = MessageResponse {
        id: "msg_serde".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![
            ContentBlock::Text {
                text: "hello".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_s".into(),
                name: "tool".into(),
                input: json!({"k":"v"}),
            },
        ],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("tool_use".into()),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: MessageResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn serde_roundtrip_stream_delta_text() {
    let d = StreamDelta::TextDelta {
        text: "token".into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: StreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(d, back);
}

#[test]
fn serde_roundtrip_stream_delta_input_json() {
    let d = StreamDelta::InputJsonDelta {
        partial_json: r#"{"k"#.into(),
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: StreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(d, back);
}

#[test]
fn serde_roundtrip_api_error() {
    let e = ApiError {
        error_type: "not_found_error".into(),
        message: "Resource not found".into(),
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(e, back);
}

#[test]
fn serde_roundtrip_message_delta_payload() {
    let p = MessageDeltaPayload {
        stop_reason: Some("end_turn".into()),
        stop_sequence: Some("###".into()),
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: MessageDeltaPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Edge cases (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_text_content_block() {
    let b = ContentBlock::Text {
        text: String::new(),
    };
    let json = serde_json::to_string(&b).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn edge_tool_use_empty_input() {
    let b = ContentBlock::ToolUse {
        id: "tu_e".into(),
        name: "noop".into(),
        input: json!({}),
    };
    let json = serde_json::to_string(&b).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(b, back);
}

#[test]
fn edge_tool_result_none_content_and_error() {
    let b = ContentBlock::ToolResult {
        tool_use_id: "tu_n".into(),
        content: None,
        is_error: None,
    };
    let json = serde_json::to_string(&b).unwrap();
    assert!(!json.contains(r#""content""#));
    assert!(!json.contains(r#""is_error""#));
}

#[test]
fn edge_multiple_content_blocks_mixed() {
    let blocks = vec![
        ContentBlock::Text { text: "A".into() },
        ContentBlock::ToolUse {
            id: "tu_mix".into(),
            name: "grep".into(),
            input: json!({}),
        },
        ContentBlock::Thinking {
            thinking: "hmm".into(),
            signature: None,
        },
    ];
    for b in &blocks {
        let json = serde_json::to_string(b).unwrap();
        let back: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(*b, back);
    }
}

#[test]
fn edge_content_to_text_empty_string_is_none() {
    let c = ClaudeContent::Text(String::new());
    assert!(content_to_text(&c).is_none());
}

#[test]
fn edge_content_to_text_blocks_concatenates() {
    let c = ClaudeContent::Blocks(vec![
        TypesContentBlock::Text { text: "a".into() },
        TypesContentBlock::Text { text: "b".into() },
    ]);
    assert_eq!(content_to_text(&c), Some("ab".into()));
}

#[test]
fn edge_content_to_text_blocks_only_tool_result_is_none() {
    let c = ClaudeContent::Blocks(vec![TypesContentBlock::ToolResult {
        tool_use_id: "tu_x".into(),
        content: "data".into(),
    }]);
    assert!(content_to_text(&c).is_none());
}

#[test]
fn edge_map_role_to_abp_unknown_defaults_user() {
    assert_eq!(map_role_to_abp("unknown"), "user");
}

#[test]
fn edge_map_role_from_abp_system_becomes_user() {
    assert_eq!(map_role_from_abp("system"), "user");
    assert_eq!(map_role_from_abp("tool"), "user");
}

#[test]
fn edge_usage_from_raw_missing_fields_defaults_zero() {
    let raw = json!({});
    let u = usage_from_raw(&raw);
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert!(u.cache_creation_input_tokens.is_none());
}

#[test]
fn edge_content_block_to_event_kind_text() {
    let b = TypesContentBlock::Text {
        text: "hello".into(),
    };
    let ek = content_block_to_event_kind(&b).unwrap();
    assert!(matches!(ek, AgentEventKind::AssistantMessage { text } if text == "hello"));
}

#[test]
fn edge_content_block_to_event_kind_tool_use() {
    let b = TypesContentBlock::ToolUse {
        id: "tu_ek".into(),
        name: "bash".into(),
        input: json!({"cmd":"ls"}),
    };
    let ek = content_block_to_event_kind(&b).unwrap();
    assert!(matches!(ek, AgentEventKind::ToolCall { tool_name, .. } if tool_name == "bash"));
}

#[test]
fn edge_content_block_to_event_kind_image_is_none() {
    let b = TypesContentBlock::Image {
        source: TypesImageSource::Url {
            url: "https://x.com/y.png".into(),
        },
    };
    assert!(content_block_to_event_kind(&b).is_none());
}

#[test]
fn edge_build_response_helper() {
    let usage = abp_shim_claude::types::ClaudeUsage {
        input_tokens: 1,
        output_tokens: 2,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let resp = build_response(
        "msg_build",
        "claude-sonnet-4-20250514",
        vec![TypesContentBlock::Text {
            text: "built".into(),
        }],
        Some("end_turn".into()),
        usage,
    );
    assert_eq!(resp.id, "msg_build");
    assert_eq!(resp.type_field, "message");
    assert_eq!(resp.content.len(), 1);
}

#[test]
fn edge_from_agent_event_warning_is_none() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "oops".into(),
        },
        ext: None,
    };
    assert!(from_agent_event(&ev).is_none());
}

#[test]
fn edge_from_agent_event_run_started_is_none() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    assert!(from_agent_event(&ev).is_none());
}

#[test]
fn edge_event_stream_empty() {
    let es = EventStream::from_vec(vec![]);
    use tokio_stream::Stream;
    assert_eq!(Stream::size_hint(&es), (0, Some(0)));
}
