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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the Claude shim crate — testing ABP as a Claude SDK drop-in replacement.

use std::collections::BTreeMap;

use abp_claude_sdk::dialect::{
    self, ClaudeApiError, ClaudeContentBlock, ClaudeMessageDelta, ClaudeResponse, ClaudeStopReason,
    ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage, ThinkingConfig,
};
use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use abp_shim_claude::{
    AnthropicClient, ApiError, ContentBlock, EventStream, ImageSource, Message,
    MessageDeltaPayload, MessageRequest, MessageResponse, Role, ShimError, StreamDelta,
    StreamEvent, Usage, content_block_from_ir, content_block_to_ir, message_to_ir,
    request_to_claude, request_to_work_order, response_from_claude, response_from_events,
    stream_event_from_claude,
};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn simple_request(text: &str) -> MessageRequest {
    MessageRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 4096,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    }
}

fn make_claude_response(content: Vec<ClaudeContentBlock>) -> ClaudeResponse {
    ClaudeResponse {
        id: "msg_test".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content,
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 50,
            output_tokens: 25,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Claude types fidelity (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_text_content_block_serde_roundtrip() {
    let block = ContentBlock::Text {
        text: "Hello, world!".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
    assert!(json.contains(r#""type":"text""#));
}

#[test]
fn fidelity_tool_use_content_block_serde_roundtrip() {
    let block = ContentBlock::ToolUse {
        id: "toolu_01A".into(),
        name: "bash".into(),
        input: json!({"command": "ls -la"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
    assert!(json.contains(r#""type":"tool_use""#));
}

#[test]
fn fidelity_tool_result_content_block_serde_roundtrip() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "toolu_01A".into(),
        content: Some("drwxr-xr-x 2 user group".into()),
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
    assert!(json.contains(r#""type":"tool_result""#));
}

#[test]
fn fidelity_tool_result_error_serde_roundtrip() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "toolu_err".into(),
        content: Some("permission denied".into()),
        is_error: Some(true),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn fidelity_thinking_content_block_serde_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "Let me analyze the problem step by step...".into(),
        signature: Some("sig_abc123".into()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
    assert!(json.contains(r#""type":"thinking""#));
}

#[test]
fn fidelity_thinking_no_signature_serde() {
    let block = ContentBlock::Thinking {
        thinking: "reasoning...".into(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("signature"));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn fidelity_image_base64_serde_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBORw0KGgoAAAANSUhEUg==".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn fidelity_image_url_serde_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/photo.jpg".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn fidelity_role_user_serde() {
    let json = serde_json::to_string(&Role::User).unwrap();
    assert_eq!(json, r#""user""#);
    let back: Role = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Role::User);
}

#[test]
fn fidelity_role_assistant_serde() {
    let json = serde_json::to_string(&Role::Assistant).unwrap();
    assert_eq!(json, r#""assistant""#);
    let back: Role = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Role::Assistant);
}

#[test]
fn fidelity_model_identifiers_known() {
    assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
    assert!(dialect::is_known_model("claude-opus-4-20250514"));
    assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
    assert!(dialect::is_known_model("claude-sonnet-3-5-20241022"));
    assert!(dialect::is_known_model("claude-3-5-haiku-latest"));
    assert!(dialect::is_known_model("claude-sonnet-4-latest"));
    assert!(dialect::is_known_model("claude-opus-4-latest"));
}

#[test]
fn fidelity_model_identifiers_unknown() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model("gemini-pro"));
    assert!(!dialect::is_known_model("unknown-model"));
}

#[test]
fn fidelity_canonical_model_mapping() {
    assert_eq!(
        dialect::to_canonical_model("claude-sonnet-4-20250514"),
        "anthropic/claude-sonnet-4-20250514"
    );
    assert_eq!(
        dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
    // round trip
    let model = "claude-opus-4-20250514";
    assert_eq!(
        dialect::from_canonical_model(&dialect::to_canonical_model(model)),
        model
    );
}

#[test]
fn fidelity_message_response_type_field_serializes_as_type() {
    let resp = MessageResponse {
        id: "msg_test".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: None,
        stop_sequence: None,
        usage: Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    let val = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["type"], "message");
}

#[test]
fn fidelity_usage_omits_none_cache_fields() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(!json.contains("cache_creation"));
    assert!(!json.contains("cache_read"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request translation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_basic_message_to_claude_request() {
    let req = simple_request("Fix the bug");
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.model, "claude-sonnet-4-20250514");
    assert_eq!(claude_req.max_tokens, 4096);
    assert_eq!(claude_req.messages.len(), 1);
    assert_eq!(claude_req.messages[0].role, "user");
    assert_eq!(claude_req.messages[0].content, "Fix the bug");
}

#[test]
fn request_system_prompt_maps_to_claude_request() {
    let req = MessageRequest {
        system: Some("You are a code reviewer.".into()),
        ..simple_request("Review this")
    };
    let claude_req = request_to_claude(&req);
    assert_eq!(
        claude_req.system.as_deref(),
        Some("You are a code reviewer.")
    );
    assert_eq!(claude_req.messages.len(), 1);
}

#[test]
fn request_to_work_order_extracts_task() {
    let req = simple_request("Implement login flow");
    let wo = request_to_work_order(&req);
    assert!(wo.task.contains("Implement login flow"));
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn request_to_work_order_model_preserved() {
    let req = MessageRequest {
        model: "claude-opus-4-20250514".into(),
        ..simple_request("task")
    };
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("claude-opus-4-20250514"));
}

#[test]
fn request_to_work_order_with_temperature_stores_in_vendor() {
    let req = MessageRequest {
        temperature: Some(0.3),
        ..simple_request("task")
    };
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.3))
    );
}

#[test]
fn request_to_work_order_with_stop_sequences_stores_in_vendor() {
    let req = MessageRequest {
        temperature: Some(0.5),
        stop_sequences: Some(vec!["###".into(), "END".into()]),
        ..simple_request("task")
    };
    let wo = request_to_work_order(&req);
    let stops = wo.config.vendor.get("stop_sequences").unwrap();
    let arr = stops.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0], "###");
}

#[test]
fn request_multi_turn_messages_convert() {
    let req = MessageRequest {
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
                content: vec![ContentBlock::Text {
                    text: "Help me code".into(),
                }],
            },
        ],
        ..simple_request("ignored")
    };
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages.len(), 3);
    assert_eq!(claude_req.messages[0].role, "user");
    assert_eq!(claude_req.messages[1].role, "assistant");
    assert_eq!(claude_req.messages[2].role, "user");
}

#[test]
fn request_thinking_config_preserved() {
    let req = MessageRequest {
        thinking: Some(ThinkingConfig::new(2048)),
        ..simple_request("think about this")
    };
    let claude_req = request_to_claude(&req);
    let tc = claude_req.thinking.unwrap();
    assert_eq!(tc.budget_tokens, 2048);
    assert_eq!(tc.thinking_type, "enabled");
}

#[test]
fn request_with_tool_use_blocks_structured_content() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Text {
                text: "Let me check.".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            },
        ],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "assistant");
    // Multiple blocks with tool_use → serialized as JSON
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msg.content).unwrap();
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], ClaudeContentBlock::Text { .. }));
    assert!(matches!(&blocks[1], ClaudeContentBlock::ToolUse { .. }));
}

#[test]
fn request_tool_result_message_converts() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("file contents".into()),
            is_error: None,
        }],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "user");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msg.content).unwrap();
    assert!(matches!(&blocks[0], ClaudeContentBlock::ToolResult { .. }));
}

#[test]
fn request_single_text_block_stays_plain_string() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "Hello world".into(),
        }],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.content, "Hello world");
}

#[test]
fn request_empty_content_message() {
    let msg = Message {
        role: Role::User,
        content: vec![],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "user");
    assert!(claude_msg.content.is_empty());
}

#[test]
fn request_max_tokens_forwarded() {
    let req = MessageRequest {
        max_tokens: 8192,
        ..simple_request("task")
    };
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.max_tokens, 8192);
}

#[test]
fn request_serde_roundtrip_full() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
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
        ],
        system: Some("Be helpful".into()),
        temperature: Some(0.7),
        stop_sequences: Some(vec!["END".into()]),
        thinking: Some(ThinkingConfig::new(1024)),
        stream: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: MessageRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.max_tokens, req.max_tokens);
    assert_eq!(back.system, req.system);
    assert_eq!(back.temperature, req.temperature);
    assert_eq!(back.stop_sequences, req.stop_sequences);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response translation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_text_claude_to_shim() {
    let claude_resp = make_claude_response(vec![ClaudeContentBlock::Text {
        text: "Hello!".into(),
    }]);
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.content.len(), 1);
    assert!(matches!(
        &resp.content[0],
        ContentBlock::Text { text } if text == "Hello!"
    ));
}

#[test]
fn response_tool_use_claude_to_shim() {
    let claude_resp = make_claude_response(vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read_file".into(),
        input: json!({"path": "lib.rs"}),
    }]);
    let resp = response_from_claude(&claude_resp);
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "read_file");
            assert_eq!(input, &json!({"path": "lib.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn response_thinking_claude_to_shim() {
    let claude_resp = make_claude_response(vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: Some("sig_xyz".into()),
        },
        ClaudeContentBlock::Text {
            text: "Answer".into(),
        },
    ]);
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.content.len(), 2);
    match &resp.content[0] {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "Let me reason...");
            assert_eq!(signature.as_deref(), Some("sig_xyz"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn response_usage_mapped() {
    let claude_resp = ClaudeResponse {
        id: "msg_u".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text { text: "hi".into() }],
        stop_reason: None,
        usage: Some(ClaudeUsage {
            input_tokens: 200,
            output_tokens: 100,
            cache_creation_input_tokens: Some(30),
            cache_read_input_tokens: Some(50),
        }),
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.usage.input_tokens, 200);
    assert_eq!(resp.usage.output_tokens, 100);
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(30));
    assert_eq!(resp.usage.cache_read_input_tokens, Some(50));
}

#[test]
fn response_usage_defaults_when_none() {
    let claude_resp = ClaudeResponse {
        usage: None,
        ..make_claude_response(vec![])
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.usage.input_tokens, 0);
    assert_eq!(resp.usage.output_tokens, 0);
}

#[test]
fn response_stop_reason_preserved() {
    let claude_resp = ClaudeResponse {
        stop_reason: Some("tool_use".into()),
        ..make_claude_response(vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "bash".into(),
            input: json!({}),
        }])
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn response_model_preserved() {
    let claude_resp = ClaudeResponse {
        model: "claude-opus-4-20250514".into(),
        ..make_claude_response(vec![])
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

#[test]
fn response_id_preserved() {
    let claude_resp = ClaudeResponse {
        id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
        ..make_claude_response(vec![])
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.id, "msg_01XFDUDYJgAACzvnptvVoYEL");
}

#[test]
fn response_from_receipt_text_only() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done!".into(),
            },
            ext: None,
        })
        .build();

    use abp_claude_sdk::messages::MessagesResponse;
    let api_resp: MessagesResponse = receipt.into();
    assert_eq!(api_resp.response_type, "message");
    assert_eq!(api_resp.role, "assistant");
    assert!(api_resp.id.starts_with("msg_"));
    assert_eq!(api_resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn response_from_receipt_with_tool_use_stop_reason() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_01".into()),
                parent_tool_use_id: None,
                input: json!({"path": "a.rs"}),
            },
            ext: None,
        })
        .build();

    use abp_claude_sdk::messages::MessagesResponse;
    let api_resp: MessagesResponse = receipt.into();
    assert_eq!(api_resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn response_from_receipt_partial_outcome_max_tokens() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Partial)
        .build();

    use abp_claude_sdk::messages::MessagesResponse;
    let api_resp: MessagesResponse = receipt.into();
    assert_eq!(api_resp.stop_reason.as_deref(), Some("max_tokens"));
}

#[test]
fn response_from_receipt_failed_outcome_no_stop_reason() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Failed)
        .build();

    use abp_claude_sdk::messages::MessagesResponse;
    let api_resp: MessagesResponse = receipt.into();
    assert!(api_resp.stop_reason.is_none());
}

#[test]
fn response_from_events_tool_call_sets_tool_use_stop() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_42".into()),
            parent_tool_use_id: None,
            input: json!({"command": "echo hi"}),
        },
        ext: None,
    }];
    let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    match &resp.content[0] {
        ContentBlock::ToolUse { name, .. } => assert_eq!(name, "bash"),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn response_from_events_empty_produces_no_stop_reason() {
    let resp = response_from_events(&[], "test-model", None);
    assert!(resp.content.is_empty());
    assert!(resp.stop_reason.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_text_delta_serde_roundtrip() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::TextDelta {
            text: "Hello".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_input_json_delta_serde_roundtrip() {
    let event = StreamEvent::ContentBlockDelta {
        index: 1,
        delta: StreamDelta::InputJsonDelta {
            partial_json: r#"{"path":"#.into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_thinking_delta_serde_roundtrip() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::ThinkingDelta {
            thinking: "Let me think...".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_signature_delta_serde_roundtrip() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::SignatureDelta {
            signature: "sig_partial".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_message_start_serde_roundtrip() {
    let event = StreamEvent::MessageStart {
        message: MessageResponse {
            id: "msg_stream".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("message_start"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_message_delta_with_stop_reason() {
    let event = StreamEvent::MessageDelta {
        delta: MessageDeltaPayload {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(Usage {
            input_tokens: 0,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_message_stop_serde() {
    let event = StreamEvent::MessageStop {};
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_ping_serde() {
    let event = StreamEvent::Ping {};
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_error_event_serde() {
    let event = StreamEvent::Error {
        error: ApiError {
            error_type: "overloaded_error".into(),
            message: "Server is busy".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_claude_event_to_shim_text_delta() {
    let claude_event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "token".into(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::ContentBlockDelta {
            index,
            delta: StreamDelta::TextDelta { text },
        } => {
            assert_eq!(index, 0);
            assert_eq!(text, "token");
        }
        other => panic!("expected ContentBlockDelta/TextDelta, got {other:?}"),
    }
}

#[test]
fn stream_claude_event_to_shim_thinking_delta() {
    let claude_event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "reasoning".into(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::ThinkingDelta { thinking },
            ..
        } => assert_eq!(thinking, "reasoning"),
        other => panic!("expected ThinkingDelta, got {other:?}"),
    }
}

#[test]
fn stream_claude_event_to_shim_message_delta() {
    let claude_event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 0,
            output_tokens: 42,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::MessageDelta { delta, usage } => {
            assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
            assert_eq!(usage.unwrap().output_tokens, 42);
        }
        other => panic!("expected MessageDelta, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Extended thinking (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_config_new() {
    let tc = ThinkingConfig::new(4096);
    assert_eq!(tc.thinking_type, "enabled");
    assert_eq!(tc.budget_tokens, 4096);
}

#[test]
fn thinking_config_serde_roundtrip() {
    let tc = ThinkingConfig::new(2048);
    let json = serde_json::to_string(&tc).unwrap();
    let back: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn thinking_config_json_has_type_field() {
    let tc = ThinkingConfig::new(1024);
    let val = serde_json::to_value(&tc).unwrap();
    assert_eq!(val["type"], "enabled");
    assert_eq!(val["budget_tokens"], 1024);
}

#[test]
fn thinking_block_in_response_via_events() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), serde_json::Value::Bool(true));
    ext.insert(
        "signature".into(),
        serde_json::Value::String("sig_think".into()),
    );

    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Deep reasoning here...".into(),
            },
            ext: Some(ext),
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Final answer".into(),
            },
            ext: None,
        },
    ];

    let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
    assert_eq!(resp.content.len(), 2);
    match &resp.content[0] {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "Deep reasoning here...");
            assert_eq!(signature.as_deref(), Some("sig_think"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
    assert!(matches!(&resp.content[1], ContentBlock::Text { text } if text == "Final answer"));
}

#[test]
fn thinking_block_without_signature_in_response() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), serde_json::Value::Bool(true));

    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hmm...".into(),
        },
        ext: Some(ext),
    }];

    let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
    match &resp.content[0] {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "hmm...");
            assert!(signature.is_none());
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn thinking_stream_delta_maps_through_claude_sdk() {
    let claude_event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "step 1: analyze...".into(),
        },
    };

    // Maps to ABP events
    let abp_events = dialect::map_stream_event(&claude_event);
    assert_eq!(abp_events.len(), 1);
    match &abp_events[0].kind {
        AgentEventKind::AssistantDelta { text } => {
            assert_eq!(text, "step 1: analyze...");
        }
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
    // Check thinking ext marker
    let ext = abp_events[0].ext.as_ref().unwrap();
    assert_eq!(ext.get("thinking"), Some(&serde_json::Value::Bool(true)));
}

#[test]
fn thinking_content_block_ir_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "internal reasoning".into(),
        signature: Some("sig_round".into()),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn thinking_content_block_no_sig_ir_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "reasoning only".into(),
        signature: None,
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn thinking_request_passes_through_pipeline() {
    let req = MessageRequest {
        thinking: Some(ThinkingConfig::new(4096)),
        ..simple_request("think carefully")
    };
    let claude_req = request_to_claude(&req);
    assert!(claude_req.thinking.is_some());
    assert_eq!(claude_req.thinking.unwrap().budget_tokens, 4096);
}

#[test]
fn thinking_in_receipt_to_api_response() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), serde_json::Value::Bool(true));
    ext.insert(
        "signature".into(),
        serde_json::Value::String("sig_receipt".into()),
    );

    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Let me think...".into(),
            },
            ext: Some(ext),
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Here's my answer.".into(),
            },
            ext: None,
        })
        .build();

    use abp_claude_sdk::messages::{ContentBlock as SdkContentBlock, MessagesResponse};
    let api_resp: MessagesResponse = receipt.into();
    assert_eq!(api_resp.content.len(), 2);
    assert!(matches!(
        &api_resp.content[0],
        SdkContentBlock::Thinking { thinking, signature }
        if thinking == "Let me think..." && signature.as_deref() == Some("sig_receipt")
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Edge cases & API surface (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_content_blocks_in_message() {
    let msg = Message {
        role: Role::User,
        content: vec![],
    };
    let ir = message_to_ir(&msg);
    assert_eq!(ir.role, "user");
    // Empty content → empty string
    assert!(ir.content.is_empty());
}

#[test]
fn edge_multiple_text_blocks_serialized_as_json() {
    let msg = Message {
        role: Role::User,
        content: vec![
            ContentBlock::Text {
                text: "First".into(),
            },
            ContentBlock::Text {
                text: "Second".into(),
            },
        ],
    };
    let ir = message_to_ir(&msg);
    // >1 block → serialized as JSON array
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&ir.content).unwrap();
    assert_eq!(blocks.len(), 2);
}

#[test]
fn edge_tool_result_none_content() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_empty".into(),
        content: None,
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("content"));
    assert!(!json.contains("is_error"));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn edge_content_block_to_ir_and_back_text() {
    let block = ContentBlock::Text {
        text: "roundtrip test".into(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn edge_content_block_to_ir_and_back_tool_use() {
    let block = ContentBlock::ToolUse {
        id: "tu_rt".into(),
        name: "grep".into(),
        input: json!({"pattern": "fn main", "path": "src/"}),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn edge_image_base64_ir_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "base64data".into(),
        },
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn edge_image_url_ir_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn edge_stop_reason_mapping_all_variants() {
    assert_eq!(
        dialect::parse_stop_reason("end_turn"),
        Some(ClaudeStopReason::EndTurn)
    );
    assert_eq!(
        dialect::parse_stop_reason("tool_use"),
        Some(ClaudeStopReason::ToolUse)
    );
    assert_eq!(
        dialect::parse_stop_reason("max_tokens"),
        Some(ClaudeStopReason::MaxTokens)
    );
    assert_eq!(
        dialect::parse_stop_reason("stop_sequence"),
        Some(ClaudeStopReason::StopSequence)
    );
    assert_eq!(dialect::parse_stop_reason("unknown"), None);

    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::EndTurn),
        "end_turn"
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::ToolUse),
        "tool_use"
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::MaxTokens),
        "max_tokens"
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::StopSequence),
        "stop_sequence"
    );
}

#[test]
fn edge_api_error_serde_roundtrip() {
    let err = ApiError {
        error_type: "invalid_request_error".into(),
        message: "messages: must not be empty".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains(r#""type":"invalid_request_error""#));
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn edge_message_request_omits_none_optional_fields() {
    let req = simple_request("test");
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("system"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("stop_sequences"));
    assert!(!json.contains("thinking"));
    assert!(!json.contains("stream"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Async tests (client integration)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn client_create_simple_roundtrip() {
    let client = AnthropicClient::new();
    let resp = client.create(simple_request("Hello")).await.unwrap();
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert!(!resp.content.is_empty());
}

#[tokio::test]
async fn client_create_stream_produces_canonical_sequence() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("Hi")).await.unwrap();
    let events = stream.collect_all().await;

    // Must start with MessageStart and end with MessageStop
    assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(
        events.last().unwrap(),
        StreamEvent::MessageStop {}
    ));

    // Must contain at least one ContentBlockDelta
    let has_delta = events
        .iter()
        .any(|e| matches!(e, StreamEvent::ContentBlockDelta { .. }));
    assert!(has_delta);
}

#[tokio::test]
async fn client_empty_messages_error() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        messages: vec![],
        ..simple_request("ignored")
    };
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::InvalidRequest(_)));
}

#[tokio::test]
async fn client_with_custom_handler() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|req| {
        let events = vec![AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: format!("Echo: {}", req.model),
            },
            ext: None,
        }];
        Ok(response_from_events(&events, &req.model, None))
    }));

    let resp = client.create(simple_request("test")).await.unwrap();
    match &resp.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("claude-sonnet-4-20250514")),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[tokio::test]
async fn client_with_custom_stream_handler() {
    let mut client = AnthropicClient::new();
    client.set_stream_handler(Box::new(|_| {
        Ok(vec![
            StreamEvent::MessageStart {
                message: MessageResponse {
                    id: "msg_custom".into(),
                    response_type: "message".into(),
                    role: "assistant".into(),
                    content: vec![],
                    model: "claude-sonnet-4-20250514".into(),
                    stop_reason: None,
                    stop_sequence: None,
                    usage: Usage {
                        input_tokens: 5,
                        output_tokens: 0,
                        cache_creation_input_tokens: None,
                        cache_read_input_tokens: None,
                    },
                },
            },
            StreamEvent::MessageStop {},
        ])
    }));

    let stream = client.create_stream(simple_request("test")).await.unwrap();
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(&events[1], StreamEvent::MessageStop {}));
}

#[tokio::test]
async fn client_model_preserved_in_response() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "claude-opus-4-20250514".into(),
        ..simple_request("test")
    };
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

use tokio_stream::Stream;

#[test]
fn event_stream_size_hint_accurate() {
    let stream = EventStream::from_vec(vec![
        StreamEvent::Ping {},
        StreamEvent::Ping {},
        StreamEvent::MessageStop {},
    ]);
    assert_eq!(Stream::size_hint(&stream), (3, Some(3)));
}

#[test]
fn client_default_model() {
    let client = AnthropicClient::new();
    let dbg = format!("{client:?}");
    assert!(dbg.contains("claude-sonnet-4-20250514"));
}

#[test]
fn client_with_model_override() {
    let client = AnthropicClient::with_model("claude-opus-4-20250514");
    let dbg = format!("{client:?}");
    assert!(dbg.contains("claude-opus-4-20250514"));
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
    let extracted = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(event, extracted);
}

#[test]
fn passthrough_thinking_delta_roundtrip() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "hmm".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let extracted = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(event, extracted);
}

#[test]
fn passthrough_fidelity_verification() {
    let events = vec![
        ClaudeStreamEvent::Ping {},
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta { text: "tok".into() },
        },
        ClaudeStreamEvent::MessageStop {},
        ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "test".into(),
                message: "test".into(),
            },
        },
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn passthrough_event_has_dialect_marker() {
    let event = ClaudeStreamEvent::Ping {};
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(
        ext.get("dialect"),
        Some(&serde_json::Value::String("claude".into()))
    );
    assert!(ext.contains_key("raw_message"));
}
