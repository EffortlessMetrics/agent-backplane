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
//! Deep surface-area tests for the Claude shim — covering the full Anthropic
//! Messages API replacement surface exposed by `abp-claude-sdk`.

use std::collections::BTreeMap;

use abp_claude_sdk::dialect::{
    self, CanonicalToolDef, ClaudeApiError, ClaudeConfig, ClaudeContentBlock, ClaudeMessage,
    ClaudeMessageDelta, ClaudeResponse, ClaudeStopReason, ClaudeStreamDelta, ClaudeStreamEvent,
    ClaudeUsage, ThinkingConfig, DEFAULT_MODEL, DIALECT_VERSION,
};
use abp_claude_sdk::lowering;
use abp_claude_sdk::messages::{
    CacheControl, ContentBlock, ImageSource, Message, MessageContent, MessagesRequest,
    MessagesResponse, Metadata, Role, StreamDelta, StreamEvent, SystemBlock, SystemMessage, Tool,
    Usage,
};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, Outcome, Receipt, ReceiptBuilder, UsageNormalized,
    WorkOrder, WorkOrderBuilder,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn minimal_request() -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text("Hello".into()),
        }],
        max_tokens: 1024,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        tool_choice: None,
        thinking: None,
    }
}

fn full_request() -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![
            Message {
                role: Role::User,
                content: MessageContent::Text("Refactor the auth module".into()),
            },
            Message {
                role: Role::Assistant,
                content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                    id: "toolu_01".into(),
                    name: "read_file".into(),
                    input: json!({"path": "src/auth.rs"}),
                }]),
            },
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_01".into(),
                    content: Some("fn login() {}".into()),
                    is_error: None,
                }]),
            },
        ],
        max_tokens: 4096,
        system: Some(SystemMessage::Text(
            "You are an expert Rust developer.".into(),
        )),
        tools: Some(vec![Tool {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        }]),
        metadata: Some(Metadata {
            user_id: Some("user_42".into()),
        }),
        stream: Some(false),
        stop_sequences: Some(vec!["###".into()]),
        temperature: Some(0.7),
        top_p: Some(0.95),
        top_k: Some(40),
        tool_choice: None,
        thinking: None,
    }
}

fn sample_response() -> MessagesResponse {
    MessagesResponse {
        id: "msg_01XFDUDYJgAACzvnptvVoYEL".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![ContentBlock::Text {
            text: "Hello!".into(),
        }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 25,
            output_tokens: 10,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    }
}

fn make_receipt_with_text(text: &str) -> Receipt {
    ReceiptBuilder::new("claude-sonnet-4-20250514")
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
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
            ext: None,
        })
        .build()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Messages request (Anthropic format)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_minimal_serde_roundtrip() {
    let req = minimal_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "claude-sonnet-4-20250514");
    assert_eq!(back.max_tokens, 1024);
}

#[test]
fn request_full_serde_roundtrip() {
    let req = full_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back, req);
}

#[test]
fn request_optional_fields_omitted_when_none() {
    let req = minimal_request();
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("system"));
    assert!(!json.contains("tools"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("top_p"));
    assert!(!json.contains("top_k"));
    assert!(!json.contains("stream"));
    assert!(!json.contains("stop_sequences"));
    assert!(!json.contains("metadata"));
}

#[test]
fn request_with_string_content() {
    let req = minimal_request();
    let val = serde_json::to_value(&req).unwrap();
    // String content serializes as bare string (untagged)
    let content = &val["messages"][0]["content"];
    assert!(content.is_string());
    assert_eq!(content.as_str().unwrap(), "Hello");
}

#[test]
fn request_with_block_content() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Look:".into(),
                },
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type: "image/png".into(),
                        data: "iVBOR==".into(),
                    },
                },
            ]),
        }],
        max_tokens: 1024,
        system: None,
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        tool_choice: None,
        thinking: None,
    };
    let val = serde_json::to_value(&req).unwrap();
    let content = &val["messages"][0]["content"];
    assert!(content.is_array());
    assert_eq!(content.as_array().unwrap().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Messages response (Anthropic format)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_serde_roundtrip() {
    let resp = sample_response();
    let json = serde_json::to_string(&resp).unwrap();
    let back: MessagesResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back, resp);
}

#[test]
fn response_type_field_serializes_as_type() {
    let resp = sample_response();
    let val = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["type"], "message");
    // Should not have "response_type" key
    assert!(val.get("response_type").is_none());
}

#[test]
fn response_role_always_assistant() {
    let resp = sample_response();
    assert_eq!(resp.role, "assistant");
}

#[test]
fn response_stop_reason_end_turn() {
    let resp = sample_response();
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn response_with_tool_use_content() {
    let resp = MessagesResponse {
        id: "msg_tools".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![
            ContentBlock::Text {
                text: "Let me check.".into(),
            },
            ContentBlock::ToolUse {
                id: "toolu_abc".into(),
                name: "bash".into(),
                input: json!({"command": "ls -la"}),
            },
        ],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("tool_use".into()),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 50,
            output_tokens: 30,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: MessagesResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content.len(), 2);
    assert_eq!(back.stop_reason.as_deref(), Some("tool_use"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Streaming response (SSE events)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_message_start_serde() {
    let event = StreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_stream".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: None,
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("message_start"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_content_block_delta_text() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::TextDelta {
            text: "Hello, ".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("content_block_delta"));
    assert!(json.contains("text_delta"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_content_block_start_serde() {
    let event = StreamEvent::ContentBlockStart {
        index: 0,
        content_block: ContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("content_block_start"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_content_block_stop_serde() {
    let event = StreamEvent::ContentBlockStop { index: 0 };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("content_block_stop"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_message_delta_with_stop_reason() {
    let event = StreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 100,
            output_tokens: 42,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("message_delta"));
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
    assert!(json.contains("ping"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_error_event_serde() {
    let event = StreamEvent::Error {
        error: ClaudeApiError {
            error_type: "overloaded_error".into(),
            message: "Server is overloaded".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("overloaded_error"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_input_json_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 1,
        delta: StreamDelta::InputJsonDelta {
            partial_json: r#"{"path":"#.into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("input_json_delta"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_thinking_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::ThinkingDelta {
            thinking: "Let me consider...".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("thinking_delta"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_signature_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::SignatureDelta {
            signature: "sig_partial".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("signature_delta"));
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Tool use
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_definition_serde_roundtrip() {
    let tool = Tool {
        name: "bash".into(),
        description: "Execute a shell command".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "command": { "type": "string" } },
            "required": ["command"]
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: Tool = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tool);
}

#[test]
fn tool_use_content_block_serde() {
    let block = ContentBlock::ToolUse {
        id: "toolu_01A".into(),
        name: "grep".into(),
        input: json!({"pattern": "fn main", "path": "."}),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"tool_use""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn tool_result_content_block_serde() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "toolu_01A".into(),
        content: Some("src/main.rs:1:fn main() {}".into()),
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"tool_result""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn tool_result_error_flag() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "toolu_err".into(),
        content: Some("permission denied".into()),
        is_error: Some(true),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("is_error"));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    match &back {
        ContentBlock::ToolResult { is_error, .. } => assert_eq!(*is_error, Some(true)),
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn tool_def_canonical_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let claude = dialect::tool_def_to_claude(&canonical);
    assert_eq!(claude.name, "read_file");
    assert_eq!(claude.input_schema, json!({"type": "object"}));

    let back = dialect::tool_def_from_claude(&claude);
    assert_eq!(back, canonical);
}

#[test]
fn tool_use_then_result_message_sequence() {
    let messages = vec![
        Message {
            role: Role::User,
            content: MessageContent::Text("Read main.rs".into()),
        },
        Message {
            role: Role::Assistant,
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: "toolu_01".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }]),
        },
        Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_01".into(),
                content: Some("fn main() { println!(\"hello\"); }".into()),
                is_error: None,
            }]),
        },
    ];
    let json = serde_json::to_string(&messages).unwrap();
    let back: Vec<Message> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Content blocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_serde() {
    let block = ContentBlock::Text {
        text: "Hello, world!".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"text""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_image_base64_serde() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "base64data==".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"image""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_image_url_serde() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/image.png".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_thinking_serde() {
    let block = ContentBlock::Thinking {
        thinking: "Let me reason step by step...".into(),
        signature: Some("sig_abc123".into()),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains(r#""type":"thinking""#));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_thinking_no_signature() {
    let block = ContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("signature"));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_mixed_array() {
    let blocks = vec![
        ContentBlock::Text {
            text: "I'll help.".into(),
        },
        ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "bash".into(),
            input: json!({"command": "echo hello"}),
        },
    ];
    let json = serde_json::to_string(&blocks).unwrap();
    let back: Vec<ContentBlock> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 2);
    assert!(matches!(&back[0], ContentBlock::Text { .. }));
    assert!(matches!(&back[1], ContentBlock::ToolUse { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. System message
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_message_text_serde() {
    let sys = SystemMessage::Text("You are a helpful assistant.".into());
    let val = serde_json::to_value(&sys).unwrap();
    assert!(val.is_string());
    let back: SystemMessage = serde_json::from_value(val).unwrap();
    assert_eq!(back, sys);
}

#[test]
fn system_message_blocks_serde() {
    let sys = SystemMessage::Blocks(vec![SystemBlock::Text {
        text: "Be helpful.".into(),
        cache_control: Some(CacheControl::ephemeral()),
    }]);
    let json = serde_json::to_string(&sys).unwrap();
    assert!(json.contains("ephemeral"));
    let back: SystemMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sys);
}

#[test]
fn system_message_not_in_messages_array() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text("Hi".into()),
        }],
        max_tokens: 1024,
        system: Some(SystemMessage::Text("system prompt".into())),
        tools: None,
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        tool_choice: None,
        thinking: None,
    };
    let val = serde_json::to_value(&req).unwrap();
    // system is a top-level field, not inside messages
    assert!(val.get("system").is_some());
    let messages = val["messages"].as_array().unwrap();
    for msg in messages {
        assert_ne!(msg["role"], "system");
    }
}

#[test]
fn system_message_blocks_without_cache_control() {
    let sys = SystemMessage::Blocks(vec![SystemBlock::Text {
        text: "Instructions here".into(),
        cache_control: None,
    }]);
    let json = serde_json::to_string(&sys).unwrap();
    assert!(!json.contains("cache_control"));
    let back: SystemMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sys);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Model names
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn model_name_sonnet_4() {
    assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
}

#[test]
fn model_name_opus_4() {
    assert!(dialect::is_known_model("claude-opus-4-20250514"));
}

#[test]
fn model_name_haiku_3_5() {
    assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
}

#[test]
fn model_name_sonnet_3_5() {
    assert!(dialect::is_known_model("claude-sonnet-3-5-20241022"));
}

#[test]
fn model_name_claude_4() {
    assert!(dialect::is_known_model("claude-4-20250714"));
}

#[test]
fn model_name_latest_aliases() {
    assert!(dialect::is_known_model("claude-3-5-haiku-latest"));
    assert!(dialect::is_known_model("claude-sonnet-4-latest"));
    assert!(dialect::is_known_model("claude-opus-4-latest"));
    assert!(dialect::is_known_model("claude-4-latest"));
}

#[test]
fn model_name_unknown_returns_false() {
    assert!(!dialect::is_known_model("gpt-4o"));
    assert!(!dialect::is_known_model("gemini-pro"));
}

#[test]
fn model_canonical_roundtrip() {
    let vendor = "claude-sonnet-4-20250514";
    let canonical = dialect::to_canonical_model(vendor);
    assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
    assert_eq!(dialect::from_canonical_model(&canonical), vendor);
}

#[test]
fn default_model_constant() {
    assert_eq!(DEFAULT_MODEL, "claude-sonnet-4-20250514");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Parameters: max_tokens, temperature, top_p, etc.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn max_tokens_is_required_field() {
    // Missing max_tokens should fail deserialization
    let json = r#"{"model":"claude-sonnet-4-20250514","messages":[]}"#;
    let result = serde_json::from_str::<MessagesRequest>(json);
    assert!(result.is_err());
}

#[test]
fn temperature_range() {
    let req = MessagesRequest {
        temperature: Some(0.0),
        ..minimal_request()
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.temperature, Some(0.0));

    let req2 = MessagesRequest {
        temperature: Some(1.0),
        ..minimal_request()
    };
    let json2 = serde_json::to_string(&req2).unwrap();
    let back2: MessagesRequest = serde_json::from_str(&json2).unwrap();
    assert_eq!(back2.temperature, Some(1.0));
}

#[test]
fn top_p_parameter() {
    let req = MessagesRequest {
        top_p: Some(0.9),
        ..minimal_request()
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["top_p"], 0.9);
}

#[test]
fn top_k_parameter() {
    let req = MessagesRequest {
        top_k: Some(50),
        ..minimal_request()
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["top_k"], 50);
}

#[test]
fn stop_sequences_parameter() {
    let req = MessagesRequest {
        stop_sequences: Some(vec!["END".into(), "STOP".into()]),
        ..minimal_request()
    };
    let val = serde_json::to_value(&req).unwrap();
    let seqs = val["stop_sequences"].as_array().unwrap();
    assert_eq!(seqs.len(), 2);
}

#[test]
fn stream_parameter_true() {
    let req = MessagesRequest {
        stream: Some(true),
        ..minimal_request()
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["stream"], true);
}

#[test]
fn metadata_user_id() {
    let req = MessagesRequest {
        metadata: Some(Metadata {
            user_id: Some("user_abc".into()),
        }),
        ..minimal_request()
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["metadata"]["user_id"], "user_abc");
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Anthropic headers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_default_base_url() {
    let config = ClaudeConfig::default();
    assert_eq!(config.base_url, "https://api.anthropic.com/v1");
}

#[test]
fn config_default_model() {
    let config = ClaudeConfig::default();
    assert_eq!(config.model, "claude-sonnet-4-20250514");
}

#[test]
fn config_default_max_tokens() {
    let config = ClaudeConfig::default();
    assert_eq!(config.max_tokens, 4096);
}

#[test]
fn config_custom_api_key() {
    let config = ClaudeConfig {
        api_key: "sk-ant-test123".into(),
        ..ClaudeConfig::default()
    };
    assert_eq!(config.api_key, "sk-ant-test123");
}

#[test]
fn dialect_version_constant() {
    assert_eq!(DIALECT_VERSION, "claude/v0.1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Error responses (Anthropic format)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn api_error_serde_roundtrip() {
    let error = ClaudeApiError {
        error_type: "invalid_request_error".into(),
        message: "max_tokens must be greater than 0".into(),
    };
    let json = serde_json::to_string(&error).unwrap();
    assert!(json.contains(r#""type":"invalid_request_error""#));
    let back: ClaudeApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, error);
}

#[test]
fn api_error_authentication() {
    let error = ClaudeApiError {
        error_type: "authentication_error".into(),
        message: "Invalid API key".into(),
    };
    let json = serde_json::to_string(&error).unwrap();
    assert!(json.contains("authentication_error"));
}

#[test]
fn api_error_rate_limit() {
    let error = ClaudeApiError {
        error_type: "rate_limit_error".into(),
        message: "Rate limit exceeded".into(),
    };
    let json = serde_json::to_string(&error).unwrap();
    assert!(json.contains("rate_limit_error"));
}

#[test]
fn api_error_overloaded() {
    let error = ClaudeApiError {
        error_type: "overloaded_error".into(),
        message: "Overloaded".into(),
    };
    let json = serde_json::to_string(&error).unwrap();
    let back: ClaudeApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(back.error_type, "overloaded_error");
}

#[test]
fn stream_error_event_carries_details() {
    let event = StreamEvent::Error {
        error: ClaudeApiError {
            error_type: "api_error".into(),
            message: "Internal server error".into(),
        },
    };
    let val = serde_json::to_value(&event).unwrap();
    assert_eq!(val["error"]["type"], "api_error");
    assert_eq!(val["error"]["message"], "Internal server error");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Client configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_serde_roundtrip() {
    let config = ClaudeConfig {
        api_key: "sk-ant-key".into(),
        base_url: "https://api.anthropic.com/v1".into(),
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 8192,
        system_prompt: Some("Be concise".into()),
        thinking: Some(ThinkingConfig::new(10000)),
    };
    let json = serde_json::to_string(&config).unwrap();
    let back: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.api_key, "sk-ant-key");
    assert_eq!(back.max_tokens, 8192);
    assert_eq!(back.system_prompt.as_deref(), Some("Be concise"));
}

#[test]
fn config_thinking_config() {
    let tc = ThinkingConfig::new(5000);
    assert_eq!(tc.thinking_type, "enabled");
    assert_eq!(tc.budget_tokens, 5000);
}

#[test]
fn config_thinking_serde() {
    let tc = ThinkingConfig::new(8000);
    let json = serde_json::to_string(&tc).unwrap();
    assert!(json.contains(r#""type":"enabled""#));
    let back: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.budget_tokens, 8000);
}

#[test]
fn config_custom_base_url() {
    let config = ClaudeConfig {
        base_url: "https://proxy.example.com/v1".into(),
        ..ClaudeConfig::default()
    };
    assert_eq!(config.base_url, "https://proxy.example.com/v1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Request → WorkOrder conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_to_work_order_basic() {
    let req = minimal_request();
    let wo: WorkOrder = req.into();
    assert_eq!(wo.task, "Hello");
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn request_to_work_order_preserves_system_prompt() {
    let req = MessagesRequest {
        system: Some(SystemMessage::Text("You are an expert.".into())),
        ..minimal_request()
    };
    let wo: WorkOrder = req.into();
    assert_eq!(
        wo.config.vendor.get("system"),
        Some(&json!("You are an expert."))
    );
}

#[test]
fn request_to_work_order_preserves_tools() {
    let req = MessagesRequest {
        tools: Some(vec![Tool {
            name: "bash".into(),
            description: "Run command".into(),
            input_schema: json!({"type": "object"}),
        }]),
        ..minimal_request()
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("tools"));
}

#[test]
fn request_to_work_order_preserves_metadata() {
    let req = MessagesRequest {
        metadata: Some(Metadata {
            user_id: Some("u123".into()),
        }),
        ..minimal_request()
    };
    let wo: WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("metadata"));
}

#[test]
fn request_to_work_order_multi_turn_extracts_all_user_text() {
    let req = MessagesRequest {
        messages: vec![
            Message {
                role: Role::User,
                content: MessageContent::Text("Part one".into()),
            },
            Message {
                role: Role::Assistant,
                content: MessageContent::Text("Response".into()),
            },
            Message {
                role: Role::User,
                content: MessageContent::Text("Part two".into()),
            },
        ],
        ..minimal_request()
    };
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains("Part one"));
    assert!(wo.task.contains("Part two"));
    assert!(!wo.task.contains("Response"));
}

#[test]
fn request_to_work_order_extracts_text_from_blocks() {
    let req = MessagesRequest {
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "First".into(),
                },
                ContentBlock::Image {
                    source: ImageSource::Url {
                        url: "https://example.com/img.png".into(),
                    },
                },
                ContentBlock::Text {
                    text: "Second".into(),
                },
            ]),
        }],
        ..minimal_request()
    };
    let wo: WorkOrder = req.into();
    assert!(wo.task.contains("First"));
    assert!(wo.task.contains("Second"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Receipt → Response conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_to_response_text_only() {
    let receipt = make_receipt_with_text("Hello, world!");
    let resp: MessagesResponse = receipt.into();
    assert!(resp.id.starts_with("msg_"));
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.content.len(), 1);
    match &resp.content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Hello, world!"),
        _ => panic!("expected Text block"),
    }
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(resp.usage.input_tokens, 100);
    assert_eq!(resp.usage.output_tokens, 50);
}

#[test]
fn receipt_to_response_with_tool_call() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("toolu_01".into()),
                parent_tool_use_id: None,
                input: json!({"command": "ls"}),
            },
            ext: None,
        })
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "toolu_01");
            assert_eq!(name, "bash");
            assert_eq!(input, &json!({"command": "ls"}));
        }
        _ => panic!("expected ToolUse block"),
    }
}

#[test]
fn receipt_to_response_partial_outcome_is_max_tokens() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Partial)
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("max_tokens"));
}

#[test]
fn receipt_to_response_failed_outcome_no_stop_reason() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Failed)
        .build();
    let resp: MessagesResponse = receipt.into();
    assert!(resp.stop_reason.is_none());
}

#[test]
fn receipt_to_response_cache_tokens() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(100),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(30),
            request_units: None,
            estimated_cost_usd: None,
        })
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.usage.cache_read_input_tokens, Some(50));
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(30));
}

#[test]
fn receipt_to_response_thinking_blocks() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), json!(true));
    ext.insert("signature".into(), json!("sig_xyz"));

    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Reasoning...".into(),
            },
            ext: Some(ext),
        })
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "The answer.".into(),
            },
            ext: None,
        })
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.content.len(), 2);
    match &resp.content[0] {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "Reasoning...");
            assert_eq!(signature.as_deref(), Some("sig_xyz"));
        }
        _ => panic!("expected Thinking block"),
    }
    match &resp.content[1] {
        ContentBlock::Text { text } => assert_eq!(text, "The answer."),
        _ => panic!("expected Text block"),
    }
}

#[test]
fn receipt_to_response_model_from_usage_raw() {
    let receipt = ReceiptBuilder::new("sidecar:claude")
        .outcome(Outcome::Complete)
        .usage_raw(json!({
            "model": "claude-opus-4-20250514",
            "input_tokens": 42
        }))
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

// ═══════════════════════════════════════════════════════════════════════════
// Dialect layer: map_work_order / map_response / map_stream_event
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_map_work_order_uses_task() {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    let config = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &config);
    assert_eq!(req.messages.len(), 1);
    assert!(req.messages[0].content.contains("Fix the bug"));
}

#[test]
fn dialect_map_response_text() {
    let resp = ClaudeResponse {
        id: "msg_1".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text {
            text: "Result".into(),
        }],
        stop_reason: Some("end_turn".into()),
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Result"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_text_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "chunk".into(),
        },
    };
    let abp_events = dialect::map_stream_event(&event);
    assert_eq!(abp_events.len(), 1);
    match &abp_events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "chunk"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_message_start() {
    let event = ClaudeStreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_s".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: None,
        },
    };
    let abp_events = dialect::map_stream_event(&event);
    assert_eq!(abp_events.len(), 1);
    assert!(matches!(
        &abp_events[0].kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[test]
fn dialect_map_stream_event_message_stop() {
    let event = ClaudeStreamEvent::MessageStop {};
    let abp_events = dialect::map_stream_event(&event);
    assert_eq!(abp_events.len(), 1);
    assert!(matches!(
        &abp_events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn dialect_map_stream_event_error() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "api_error".into(),
            message: "Something broke".into(),
        },
    };
    let abp_events = dialect::map_stream_event(&event);
    assert_eq!(abp_events.len(), 1);
    match &abp_events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert!(message.contains("api_error"));
            assert!(message.contains("Something broke"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn dialect_map_stream_event_ping_produces_no_events() {
    let event = ClaudeStreamEvent::Ping {};
    let abp_events = dialect::map_stream_event(&event);
    assert!(abp_events.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Stop reason mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stop_reason_end_turn() {
    assert_eq!(
        dialect::parse_stop_reason("end_turn"),
        Some(ClaudeStopReason::EndTurn)
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::EndTurn),
        "end_turn"
    );
}

#[test]
fn stop_reason_tool_use() {
    assert_eq!(
        dialect::parse_stop_reason("tool_use"),
        Some(ClaudeStopReason::ToolUse)
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::ToolUse),
        "tool_use"
    );
}

#[test]
fn stop_reason_max_tokens() {
    assert_eq!(
        dialect::parse_stop_reason("max_tokens"),
        Some(ClaudeStopReason::MaxTokens)
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::MaxTokens),
        "max_tokens"
    );
}

#[test]
fn stop_reason_stop_sequence() {
    assert_eq!(
        dialect::parse_stop_reason("stop_sequence"),
        Some(ClaudeStopReason::StopSequence)
    );
}

#[test]
fn stop_reason_unknown_returns_none() {
    assert_eq!(dialect::parse_stop_reason("unknown"), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_roundtrip_text_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "hello".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext["dialect"], "claude");
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn passthrough_fidelity_multiple_events() {
    let events = vec![
        ClaudeStreamEvent::Ping {},
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta { text: "a".into() },
        },
        ClaudeStreamEvent::MessageStop {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

// ═══════════════════════════════════════════════════════════════════════════
// Lowering: Claude ↔ IR
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lowering_user_text_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello".into(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].content, "Hello");
}

#[test]
fn lowering_system_prompt_extracted() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hi".into(),
    }];
    let conv = lowering::to_ir(&msgs, Some("Be helpful"));
    let sys = lowering::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("Be helpful"));
}

#[test]
fn lowering_system_skipped_in_from_ir() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Test".into(),
    }];
    let conv = lowering::to_ir(&msgs, Some("System prompt"));
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
// Capability manifest
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_has_streaming() {
    let manifest = dialect::capability_manifest();
    assert!(manifest.get(&Capability::Streaming).is_some());
}

#[test]
fn capability_manifest_has_tool_support() {
    let manifest = dialect::capability_manifest();
    assert!(manifest.get(&Capability::ToolRead).is_some());
    assert!(manifest.get(&Capability::ToolWrite).is_some());
    assert!(manifest.get(&Capability::ToolBash).is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// map_tool_result helper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_tool_result_success() {
    let msg = dialect::map_tool_result("toolu_01", "file contents here", false);
    assert_eq!(msg.role, "user");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "toolu_01");
            assert_eq!(content.as_deref(), Some("file contents here"));
            assert!(is_error.is_none());
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn map_tool_result_error() {
    let msg = dialect::map_tool_result("toolu_02", "permission denied", true);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(*is_error, Some(true));
        }
        _ => panic!("expected ToolResult"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge cases and additional coverage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_request_messages_to_work_order() {
    let req = MessagesRequest {
        messages: vec![],
        ..minimal_request()
    };
    let wo: WorkOrder = req.into();
    // Should produce an empty task string (no user messages)
    assert!(wo.task.is_empty());
}

#[test]
fn usage_with_cache_serde() {
    let usage = Usage {
        input_tokens: 500,
        output_tokens: 250,
        cache_creation_input_tokens: Some(100),
        cache_read_input_tokens: Some(50),
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(json.contains("cache_creation_input_tokens"));
    assert!(json.contains("cache_read_input_tokens"));
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn usage_without_cache_omits_fields() {
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

#[test]
fn role_serde_user() {
    assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
}

#[test]
fn role_serde_assistant() {
    assert_eq!(
        serde_json::to_string(&Role::Assistant).unwrap(),
        "\"assistant\""
    );
}

#[test]
fn role_deserialize() {
    let role: Role = serde_json::from_str("\"user\"").unwrap();
    assert_eq!(role, Role::User);
    let role: Role = serde_json::from_str("\"assistant\"").unwrap();
    assert_eq!(role, Role::Assistant);
}

#[test]
fn cache_control_ephemeral() {
    let cc = CacheControl::ephemeral();
    assert_eq!(cc.cache_type, "ephemeral");
    let json = serde_json::to_string(&cc).unwrap();
    assert!(json.contains(r#""type":"ephemeral""#));
}
