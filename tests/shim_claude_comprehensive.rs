// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the `abp-shim-claude` crate.

use abp_claude_sdk::dialect::{
    self, ClaudeApiError, ClaudeContentBlock, ClaudeMessageDelta, ClaudeResponse,
    ClaudeStreamDelta, ClaudeStreamEvent, ClaudeUsage, ThinkingConfig,
};
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

fn full_request() -> MessageRequest {
    MessageRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 8192,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Hi there!".into(),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "How are you?".into(),
                }],
            },
        ],
        system: Some("You are helpful.".into()),
        temperature: Some(0.7),
        stop_sequences: Some(vec!["STOP".into()]),
        thinking: Some(ThinkingConfig::new(2048)),
        stream: Some(false),
    }
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_agent_event_with_ext(
    kind: AgentEventKind,
    ext: BTreeMap<String, serde_json::Value>,
) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Claude shim initialization and configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn client_default_creates_with_defaults() {
    let client = AnthropicClient::new();
    let dbg = format!("{client:?}");
    assert!(dbg.contains("AnthropicClient"));
    assert!(dbg.contains("claude"));
    assert!(dbg.contains("4096"));
}

#[test]
fn client_with_model_sets_model() {
    let client = AnthropicClient::with_model("claude-opus-4-20250514");
    let dbg = format!("{client:?}");
    assert!(dbg.contains("claude-opus-4-20250514"));
}

#[test]
fn client_with_custom_model_string() {
    let client = AnthropicClient::with_model("my-custom-model");
    let dbg = format!("{client:?}");
    assert!(dbg.contains("my-custom-model"));
}

#[test]
fn client_default_impl_matches_new() {
    let c1 = AnthropicClient::new();
    let c2 = AnthropicClient::default();
    let d1 = format!("{c1:?}");
    let d2 = format!("{c2:?}");
    assert_eq!(d1, d2);
}

#[test]
fn client_debug_does_not_expose_handler() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        Ok(MessageResponse {
            id: "test".into(),
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
        })
    }));
    let dbg = format!("{client:?}");
    assert!(dbg.contains("AnthropicClient"));
}

#[test]
fn client_set_stream_handler() {
    let mut client = AnthropicClient::new();
    client.set_stream_handler(Box::new(|_| Ok(vec![StreamEvent::Ping {}])));
    let dbg = format!("{client:?}");
    assert!(dbg.contains("AnthropicClient"));
}

#[test]
fn client_max_tokens_field() {
    let client = AnthropicClient::new();
    let dbg = format!("{client:?}");
    assert!(dbg.contains("4096"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request translation (Claude dialect → IR)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_to_claude_simple_text() {
    let req = simple_request("Hello world");
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.model, "claude-sonnet-4-20250514");
    assert_eq!(claude_req.max_tokens, 4096);
    assert_eq!(claude_req.messages.len(), 1);
    assert_eq!(claude_req.messages[0].role, "user");
    assert_eq!(claude_req.messages[0].content, "Hello world");
}

#[test]
fn request_to_claude_with_system_prompt() {
    let mut req = simple_request("Hi");
    req.system = Some("Be concise.".into());
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.system.as_deref(), Some("Be concise."));
}

#[test]
fn request_to_claude_without_system_prompt() {
    let req = simple_request("Hi");
    let claude_req = request_to_claude(&req);
    assert!(claude_req.system.is_none());
}

#[test]
fn request_to_claude_with_thinking() {
    let mut req = simple_request("Think");
    req.thinking = Some(ThinkingConfig::new(4096));
    let claude_req = request_to_claude(&req);
    assert!(claude_req.thinking.is_some());
    assert_eq!(claude_req.thinking.unwrap().budget_tokens, 4096);
}

#[test]
fn request_to_claude_multi_turn() {
    let req = full_request();
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages.len(), 3);
    assert_eq!(claude_req.messages[0].role, "user");
    assert_eq!(claude_req.messages[1].role, "assistant");
    assert_eq!(claude_req.messages[2].role, "user");
}

#[test]
fn request_to_claude_preserves_model() {
    let mut req = simple_request("Test");
    req.model = "claude-opus-4-20250514".into();
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.model, "claude-opus-4-20250514");
}

#[test]
fn request_to_claude_preserves_max_tokens() {
    let mut req = simple_request("Test");
    req.max_tokens = 1024;
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.max_tokens, 1024);
}

#[test]
fn request_to_claude_structured_content_serialized() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![Message {
            role: Role::User,
            content: vec![
                ContentBlock::Text {
                    text: "Look at this".into(),
                },
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type: "image/png".into(),
                        data: "abc123".into(),
                    },
                },
            ],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let claude_req = request_to_claude(&req);
    // Multi-block content is JSON-serialized
    let blocks: Vec<ClaudeContentBlock> =
        serde_json::from_str(&claude_req.messages[0].content).unwrap();
    assert_eq!(blocks.len(), 2);
}

#[test]
fn request_to_claude_single_text_message_is_plain() {
    let req = simple_request("Plain text");
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages[0].content, "Plain text");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response translation (IR → Claude dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_from_claude_basic() {
    let claude_resp = ClaudeResponse {
        id: "msg_test123".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text {
            text: "Hello!".into(),
        }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.id, "msg_test123");
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.model, "claude-sonnet-4-20250514");
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(resp.usage.input_tokens, 10);
    assert_eq!(resp.usage.output_tokens, 20);
}

#[test]
fn response_from_claude_no_usage() {
    let claude_resp = ClaudeResponse {
        id: "msg_nousage".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: None,
        usage: None,
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.usage.input_tokens, 0);
    assert_eq!(resp.usage.output_tokens, 0);
}

#[test]
fn response_from_claude_with_cache_usage() {
    let claude_resp = ClaudeResponse {
        id: "msg_cache".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: None,
        usage: Some(ClaudeUsage {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation_input_tokens: Some(50),
            cache_read_input_tokens: Some(30),
        }),
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(50));
    assert_eq!(resp.usage.cache_read_input_tokens, Some(30));
}

#[test]
fn response_from_claude_stop_sequence_is_none() {
    let claude_resp = ClaudeResponse {
        id: "msg_x".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: Some("stop_sequence".into()),
        usage: None,
    };
    let resp = response_from_claude(&claude_resp);
    assert!(resp.stop_sequence.is_none());
}

#[test]
fn response_from_claude_multiple_content_blocks() {
    let claude_resp = ClaudeResponse {
        id: "msg_multi".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "First".into(),
            },
            ClaudeContentBlock::Text {
                text: "Second".into(),
            },
        ],
        stop_reason: None,
        usage: None,
    };
    let resp = response_from_claude(&claude_resp);
    assert_eq!(resp.content.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Tool use roundtrip (tool_use / tool_result blocks)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_block_roundtrip() {
    let block = ContentBlock::ToolUse {
        id: "tu_abc".into(),
        name: "read_file".into(),
        input: json!({"path": "src/main.rs"}),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn tool_result_block_roundtrip() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_abc".into(),
        content: Some("file contents".into()),
        is_error: Some(false),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn tool_result_with_error_roundtrip() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("Permission denied".into()),
        is_error: Some(true),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn tool_result_no_content_roundtrip() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu_empty".into(),
        content: None,
        is_error: None,
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn tool_use_complex_input() {
    let block = ContentBlock::ToolUse {
        id: "tu_complex".into(),
        name: "execute_command".into(),
        input: json!({
            "command": "ls -la",
            "timeout": 30,
            "env": {"PATH": "/usr/bin"},
            "nested": [1, 2, {"key": "val"}]
        }),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn tool_use_empty_input() {
    let block = ContentBlock::ToolUse {
        id: "tu_noinput".into(),
        name: "get_time".into(),
        input: json!({}),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[tokio::test]
async fn tool_use_in_response_via_handler() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|req| {
        let events = vec![make_agent_event(AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("tu_999".into()),
            parent_tool_use_id: None,
            input: json!({"path": "out.txt", "content": "data"}),
        })];
        Ok(response_from_events(&events, &req.model, None))
    }));
    let resp = client.create(simple_request("Write it")).await.unwrap();
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    assert!(matches!(&resp.content[0], ContentBlock::ToolUse { name, .. } if name == "write_file"));
}

#[tokio::test]
async fn multiple_tool_uses_in_response() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|req| {
        let events = vec![
            make_agent_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "a.rs"}),
            }),
            make_agent_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_2".into()),
                parent_tool_use_id: None,
                input: json!({"path": "b.rs"}),
            }),
        ];
        Ok(response_from_events(&events, &req.model, None))
    }));
    let resp = client.create(simple_request("Read both")).await.unwrap();
    assert_eq!(resp.content.len(), 2);
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn tool_use_message_conversion() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("result data".into()),
            is_error: Some(false),
        }],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "user");
    // ToolResult is structured so serialized as JSON
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msg.content).unwrap();
    assert_eq!(blocks.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Thinking/reasoning blocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_block_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "Let me reason about this...".into(),
        signature: Some("sig_xyz".into()),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn thinking_block_no_signature() {
    let block = ContentBlock::Thinking {
        thinking: "Reasoning...".into(),
        signature: None,
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn thinking_block_serde_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "Step 1: analyze\nStep 2: respond".into(),
        signature: Some("sig_abc123".into()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn thinking_config_new() {
    let tc = ThinkingConfig::new(8192);
    assert_eq!(tc.thinking_type, "enabled");
    assert_eq!(tc.budget_tokens, 8192);
}

#[test]
fn thinking_config_serde() {
    let tc = ThinkingConfig::new(4096);
    let json = serde_json::to_string(&tc).unwrap();
    let back: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[tokio::test]
async fn thinking_blocks_in_response_from_events() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), serde_json::Value::Bool(true));
    ext.insert(
        "signature".into(),
        serde_json::Value::String("sig_test".into()),
    );
    let events = vec![
        make_agent_event_with_ext(
            AgentEventKind::AssistantMessage {
                text: "Internal reasoning".into(),
            },
            ext,
        ),
        make_agent_event(AgentEventKind::AssistantMessage {
            text: "Final answer".into(),
        }),
    ];
    let resp = response_from_events(&events, "test-model", None);
    assert_eq!(resp.content.len(), 2);
    match &resp.content[0] {
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "Internal reasoning");
            assert_eq!(signature.as_deref(), Some("sig_test"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
    assert!(matches!(&resp.content[1], ContentBlock::Text { .. }));
}

#[test]
fn thinking_event_without_signature() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), serde_json::Value::Bool(true));
    let events = vec![make_agent_event_with_ext(
        AgentEventKind::AssistantMessage {
            text: "Just thinking".into(),
        },
        ext,
    )];
    let resp = response_from_events(&events, "test", None);
    match &resp.content[0] {
        ContentBlock::Thinking { signature, .. } => assert!(signature.is_none()),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Content blocks (text, image, tool_use, tool_result)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn text_block_roundtrip() {
    let block = ContentBlock::Text {
        text: "Hello world".into(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn text_block_empty() {
    let block = ContentBlock::Text {
        text: String::new(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn text_block_unicode() {
    let block = ContentBlock::Text {
        text: "こんにちは 🌍 مرحبا".into(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn text_block_with_newlines() {
    let block = ContentBlock::Text {
        text: "line1\nline2\nline3".into(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn image_base64_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        },
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn image_url_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Url {
            url: "https://example.com/image.png".into(),
        },
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn image_jpeg_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "/9j/4AAQ".into(),
        },
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn image_webp_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/webp".into(),
            data: "UklGR".into(),
        },
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn image_gif_roundtrip() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/gif".into(),
            data: "R0lGODlh".into(),
        },
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn content_block_serde_text() {
    let block = ContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"text\""));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_serde_tool_use() {
    let block = ContentBlock::ToolUse {
        id: "tu1".into(),
        name: "bash".into(),
        input: json!({"cmd": "ls"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"tool_use\""));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_serde_tool_result() {
    let block = ContentBlock::ToolResult {
        tool_use_id: "tu1".into(),
        content: Some("output".into()),
        is_error: Some(false),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"tool_result\""));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_serde_image() {
    let block = ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"image\""));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_serde_thinking() {
    let block = ContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"thinking\""));
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn mixed_content_message_to_ir() {
    let msg = Message {
        role: Role::User,
        content: vec![
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/jpeg".into(),
                    data: "base64data".into(),
                },
            },
            ContentBlock::Text {
                text: "What is this?".into(),
            },
        ],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "user");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&claude_msg.content).unwrap();
    assert_eq!(blocks.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Streaming events handling
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn streaming_produces_canonical_event_sequence() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("Hi")).await.unwrap();
    let events = stream.collect_all().await;

    assert!(events.len() >= 5);
    assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(
        events.last().unwrap(),
        StreamEvent::MessageStop {}
    ));
}

#[tokio::test]
async fn streaming_has_content_block_lifecycle() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("Test")).await.unwrap();
    let events = stream.collect_all().await;

    let has_start = events
        .iter()
        .any(|e| matches!(e, StreamEvent::ContentBlockStart { .. }));
    let has_delta = events
        .iter()
        .any(|e| matches!(e, StreamEvent::ContentBlockDelta { .. }));
    let has_stop = events
        .iter()
        .any(|e| matches!(e, StreamEvent::ContentBlockStop { .. }));
    assert!(has_start);
    assert!(has_delta);
    assert!(has_stop);
}

#[tokio::test]
async fn streaming_has_ping() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("Ping")).await.unwrap();
    let events = stream.collect_all().await;
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Ping {})));
}

#[tokio::test]
async fn streaming_has_message_delta_with_stop_reason() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("X")).await.unwrap();
    let events = stream.collect_all().await;

    let delta = events.iter().find_map(|e| match e {
        StreamEvent::MessageDelta { delta, .. } => Some(delta),
        _ => None,
    });
    assert!(delta.is_some());
    assert_eq!(delta.unwrap().stop_reason.as_deref(), Some("end_turn"));
}

#[tokio::test]
async fn streaming_text_delta_contains_content() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("Reply")).await.unwrap();
    let events = stream.collect_all().await;

    let text = events.iter().find_map(|e| match e {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::TextDelta { text },
            ..
        } => Some(text.clone()),
        _ => None,
    });
    assert!(text.is_some());
    assert!(!text.unwrap().is_empty());
}

#[tokio::test]
async fn streaming_usage_in_message_delta() {
    let client = AnthropicClient::new();
    let stream = client.create_stream(simple_request("Y")).await.unwrap();
    let events = stream.collect_all().await;

    let usage = events.iter().find_map(|e| match e {
        StreamEvent::MessageDelta { usage, .. } => usage.clone(),
        _ => None,
    });
    assert!(usage.is_some());
}

#[tokio::test]
async fn event_stream_size_hint_correct() {
    let stream = EventStream::from_vec(vec![
        StreamEvent::Ping {},
        StreamEvent::MessageStop {},
        StreamEvent::Ping {},
    ]);
    assert_eq!(stream.size_hint(), (3, Some(3)));
}

#[tokio::test]
async fn event_stream_empty() {
    let stream = EventStream::from_vec(vec![]);
    let events = stream.collect_all().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn event_stream_collects_all() {
    let input_events = vec![
        StreamEvent::Ping {},
        StreamEvent::Ping {},
        StreamEvent::MessageStop {},
    ];
    let stream = EventStream::from_vec(input_events.clone());
    let collected = stream.collect_all().await;
    assert_eq!(collected.len(), 3);
}

#[tokio::test]
async fn event_stream_via_stream_ext() {
    let mut stream = EventStream::from_vec(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}]);
    let first = StreamExt::next(&mut stream).await;
    assert!(first.is_some());
    assert!(matches!(first.unwrap(), StreamEvent::Ping {}));
    let second = StreamExt::next(&mut stream).await;
    assert!(matches!(second.unwrap(), StreamEvent::MessageStop {}));
    let third = StreamExt::next(&mut stream).await;
    assert!(third.is_none());
}

#[tokio::test]
async fn custom_stream_handler() {
    let mut client = AnthropicClient::new();
    client.set_stream_handler(Box::new(|_| {
        Ok(vec![StreamEvent::Ping {}, StreamEvent::MessageStop {}])
    }));
    let stream = client.create_stream(simple_request("Z")).await.unwrap();
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Model mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_canonical_model_adds_prefix() {
    let canonical = dialect::to_canonical_model("claude-sonnet-4-20250514");
    assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
}

#[test]
fn from_canonical_model_strips_prefix() {
    let vendor = dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514");
    assert_eq!(vendor, "claude-sonnet-4-20250514");
}

#[test]
fn from_canonical_model_no_prefix() {
    let vendor = dialect::from_canonical_model("claude-sonnet-4-20250514");
    assert_eq!(vendor, "claude-sonnet-4-20250514");
}

#[test]
fn canonical_model_roundtrip() {
    let original = "claude-opus-4-20250514";
    let canonical = dialect::to_canonical_model(original);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, original);
}

#[test]
fn is_known_model_sonnet() {
    assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
}

#[test]
fn is_known_model_opus() {
    assert!(dialect::is_known_model("claude-opus-4-20250514"));
}

#[test]
fn is_known_model_haiku() {
    assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
}

#[test]
fn is_known_model_unknown() {
    assert!(!dialect::is_known_model("gpt-4"));
}

#[test]
fn is_known_model_empty() {
    assert!(!dialect::is_known_model(""));
}

#[test]
fn is_known_model_latest_variants() {
    assert!(dialect::is_known_model("claude-sonnet-4-latest"));
    assert!(dialect::is_known_model("claude-opus-4-latest"));
    assert!(dialect::is_known_model("claude-3-5-haiku-latest"));
}

#[test]
fn default_model_is_known() {
    assert!(dialect::is_known_model(dialect::DEFAULT_MODEL));
}

#[test]
fn model_preserved_in_work_order() {
    let req = simple_request("Test");
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[tokio::test]
async fn model_preserved_in_response() {
    let client = AnthropicClient::new();
    let mut req = simple_request("Test");
    req.model = "claude-opus-4-20250514".into();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Error translation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn shim_error_invalid_request_display() {
    let err = ShimError::InvalidRequest("bad input".into());
    assert_eq!(err.to_string(), "invalid request: bad input");
}

#[test]
fn shim_error_api_error_display() {
    let err = ShimError::ApiError {
        error_type: "rate_limit_error".into(),
        message: "Too many requests".into(),
    };
    assert_eq!(
        err.to_string(),
        "api error (rate_limit_error): Too many requests"
    );
}

#[test]
fn shim_error_internal_display() {
    let err = ShimError::Internal("something broke".into());
    assert_eq!(err.to_string(), "internal: something broke");
}

#[tokio::test]
async fn empty_messages_returns_invalid_request() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 4096,
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
async fn empty_messages_stream_returns_invalid_request() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 4096,
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

#[test]
fn api_error_serde_roundtrip() {
    let err = ApiError {
        error_type: "invalid_request_error".into(),
        message: "Invalid param".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[tokio::test]
async fn custom_handler_api_error() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        Err(ShimError::ApiError {
            error_type: "overloaded_error".into(),
            message: "Server is overloaded".into(),
        })
    }));
    let err = client.create(simple_request("test")).await.unwrap_err();
    match err {
        ShimError::ApiError {
            error_type,
            message,
        } => {
            assert_eq!(error_type, "overloaded_error");
            assert_eq!(message, "Server is overloaded");
        }
        other => panic!("expected ApiError, got {other:?}"),
    }
}

#[tokio::test]
async fn custom_handler_internal_error() {
    let mut client = AnthropicClient::new();
    client.set_handler(Box::new(|_| {
        Err(ShimError::Internal("pipeline failed".into()))
    }));
    let err = client.create(simple_request("test")).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn custom_stream_handler_error() {
    let mut client = AnthropicClient::new();
    client.set_stream_handler(Box::new(|_| Err(ShimError::InvalidRequest("nope".into()))));
    let err = client
        .create_stream(simple_request("test"))
        .await
        .unwrap_err();
    assert!(matches!(err, ShimError::InvalidRequest(_)));
}

#[test]
fn error_stream_event_serde() {
    let event = StreamEvent::Error {
        error: ApiError {
            error_type: "overloaded_error".into(),
            message: "Busy".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn claude_api_error_stream_conversion() {
    let claude_event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "authentication_error".into(),
            message: "Invalid API key".into(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::Error { error } => {
            assert_eq!(error.error_type, "authentication_error");
            assert_eq!(error.message, "Invalid API key");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Edge cases: missing fields, invalid blocks, oversized requests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_from_events_empty_yields_no_content() {
    let resp = response_from_events(&[], "test-model", None);
    assert!(resp.content.is_empty());
    assert!(resp.stop_reason.is_none());
}

#[test]
fn response_from_events_ignores_non_matching_events() {
    let events = vec![
        make_agent_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        make_agent_event(AgentEventKind::Warning {
            message: "warn".into(),
        }),
        make_agent_event(AgentEventKind::FileChanged {
            path: "f.txt".into(),
            summary: "changed".into(),
        }),
    ];
    let resp = response_from_events(&events, "test", None);
    assert!(resp.content.is_empty());
}

#[test]
fn response_from_events_run_completed_sets_end_turn() {
    let events = vec![
        make_agent_event(AgentEventKind::AssistantMessage {
            text: "done".into(),
        }),
        make_agent_event(AgentEventKind::RunCompleted {
            message: "completed".into(),
        }),
    ];
    let resp = response_from_events(&events, "test", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn response_from_events_tool_call_then_run_completed() {
    let events = vec![
        make_agent_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tu_1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        make_agent_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];
    let resp = response_from_events(&events, "test", None);
    // tool_use stop reason takes precedence
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn response_from_events_assistant_message_defaults_end_turn() {
    let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    })];
    let resp = response_from_events(&events, "test", None);
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn response_from_events_id_format() {
    let events = vec![make_agent_event(AgentEventKind::AssistantMessage {
        text: "test".into(),
    })];
    let resp = response_from_events(&events, "m", None);
    assert!(resp.id.starts_with("msg_"));
}

#[test]
fn response_from_events_role_is_assistant() {
    let resp = response_from_events(&[], "m", None);
    assert_eq!(resp.role, "assistant");
}

#[test]
fn response_from_events_response_type_is_message() {
    let resp = response_from_events(&[], "m", None);
    assert_eq!(resp.response_type, "message");
}

#[test]
fn message_request_optional_fields_skip_serialization() {
    let req = simple_request("Hi");
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("system").is_none());
    assert!(json.get("temperature").is_none());
    assert!(json.get("stop_sequences").is_none());
    assert!(json.get("thinking").is_none());
    assert!(json.get("stream").is_none());
}

#[test]
fn message_request_full_serde_roundtrip() {
    let req = full_request();
    let json = serde_json::to_string(&req).unwrap();
    let back: MessageRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.max_tokens, req.max_tokens);
    assert_eq!(back.system, req.system);
    assert_eq!(back.temperature, req.temperature);
    assert_eq!(back.stop_sequences, req.stop_sequences);
    assert_eq!(back.messages.len(), req.messages.len());
}

#[test]
fn message_response_serde_roundtrip() {
    let resp = MessageResponse {
        id: "msg_roundtrip".into(),
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
        stop_sequence: None,
        usage: Usage {
            input_tokens: 50,
            output_tokens: 100,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(5),
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: MessageResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn usage_without_cache_fields() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 20,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    // cache fields should be skipped when None
    assert!(!json.contains("cache_creation"));
    assert!(!json.contains("cache_read"));
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn usage_with_cache_fields() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 200,
        cache_creation_input_tokens: Some(50),
        cache_read_input_tokens: Some(30),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn role_serde_roundtrip() {
    let user: Role = serde_json::from_str("\"user\"").unwrap();
    assert_eq!(user, Role::User);
    let assistant: Role = serde_json::from_str("\"assistant\"").unwrap();
    assert_eq!(assistant, Role::Assistant);
}

#[test]
fn message_to_ir_empty_content() {
    let msg = Message {
        role: Role::User,
        content: vec![],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "user");
    assert!(claude_msg.content.is_empty());
}

#[test]
fn message_to_ir_assistant_role() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "Response".into(),
        }],
    };
    let claude_msg = message_to_ir(&msg);
    assert_eq!(claude_msg.role, "assistant");
}

#[test]
fn request_to_work_order_extracts_task_from_last_message() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "First message".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Response".into(),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Last user message".into(),
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
    assert_eq!(wo.task, "Last user message");
}

#[test]
fn request_to_work_order_fallback_task() {
    let req = MessageRequest {
        model: "test".into(),
        max_tokens: 100,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: Some("result".into()),
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
    // No text block in last message → fallback
    assert_eq!(wo.task, "Claude shim request");
}

#[test]
fn request_to_work_order_with_temperature_sets_vendor() {
    let mut req = simple_request("test");
    req.temperature = Some(0.5);
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.5))
    );
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(4096))
    );
}

#[test]
fn request_to_work_order_with_stop_sequences() {
    let mut req = simple_request("test");
    req.temperature = Some(0.5);
    req.stop_sequences = Some(vec!["STOP".into(), "END".into()]);
    let wo = request_to_work_order(&req);
    let stops = wo.config.vendor.get("stop_sequences").unwrap();
    let arr = stops.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn request_to_work_order_without_temperature_no_vendor() {
    let req = simple_request("test");
    let wo = request_to_work_order(&req);
    // Without temperature, vendor map is default (empty)
    assert!(!wo.config.vendor.contains_key("temperature"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream event conversions from Claude SDK types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_from_claude_message_start() {
    let claude_resp = ClaudeResponse {
        id: "msg_test".into(),
        model: "test".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: None,
        usage: None,
    };
    let claude_event = ClaudeStreamEvent::MessageStart {
        message: claude_resp,
    };
    let shim_event = stream_event_from_claude(&claude_event);
    assert!(matches!(shim_event, StreamEvent::MessageStart { .. }));
}

#[test]
fn stream_event_from_claude_content_block_start() {
    let claude_event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
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
fn stream_event_from_claude_text_delta() {
    let claude_event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "hello".into(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::ContentBlockDelta {
            index,
            delta: StreamDelta::TextDelta { text },
        } => {
            assert_eq!(index, 0);
            assert_eq!(text, "hello");
        }
        other => panic!("expected ContentBlockDelta/TextDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_input_json_delta() {
    let claude_event = ClaudeStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: "{\"path\":".into(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::InputJsonDelta { partial_json },
            ..
        } => {
            assert_eq!(partial_json, "{\"path\":");
        }
        other => panic!("expected InputJsonDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_thinking_delta() {
    let claude_event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "hmm...".into(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::ThinkingDelta { thinking },
            ..
        } => {
            assert_eq!(thinking, "hmm...");
        }
        other => panic!("expected ThinkingDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_signature_delta() {
    let claude_event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::SignatureDelta {
            signature: "sig_partial".into(),
        },
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::ContentBlockDelta {
            delta: StreamDelta::SignatureDelta { signature },
            ..
        } => {
            assert_eq!(signature, "sig_partial");
        }
        other => panic!("expected SignatureDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_content_block_stop() {
    let claude_event = ClaudeStreamEvent::ContentBlockStop { index: 2 };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::ContentBlockStop { index } => assert_eq!(index, 2),
        other => panic!("expected ContentBlockStop, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_message_delta() {
    let claude_event = ClaudeStreamEvent::MessageDelta {
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
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
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
    let claude_event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("max_tokens".into()),
            stop_sequence: None,
        },
        usage: None,
    };
    let shim_event = stream_event_from_claude(&claude_event);
    match shim_event {
        StreamEvent::MessageDelta { usage, .. } => {
            assert!(usage.is_none());
        }
        other => panic!("expected MessageDelta, got {other:?}"),
    }
}

#[test]
fn stream_event_from_claude_message_stop() {
    let shim_event = stream_event_from_claude(&ClaudeStreamEvent::MessageStop {});
    assert!(matches!(shim_event, StreamEvent::MessageStop {}));
}

#[test]
fn stream_event_from_claude_ping() {
    let shim_event = stream_event_from_claude(&ClaudeStreamEvent::Ping {});
    assert!(matches!(shim_event, StreamEvent::Ping {}));
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream event serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_serde_ping() {
    let event = StreamEvent::Ping {};
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_event_serde_message_stop() {
    let event = StreamEvent::MessageStop {};
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_event_serde_content_block_stop() {
    let event = StreamEvent::ContentBlockStop { index: 5 };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_event_serde_text_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::TextDelta {
            text: "world".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_event_serde_input_json_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 1,
        delta: StreamDelta::InputJsonDelta {
            partial_json: "\"file.rs\"}".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_event_serde_thinking_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::ThinkingDelta {
            thinking: "let me think".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_event_serde_signature_delta() {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::SignatureDelta {
            signature: "partial_sig".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

#[test]
fn stream_event_serde_message_delta() {
    let event = StreamEvent::MessageDelta {
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
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Stop reason mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_stop_reason_end_turn() {
    assert_eq!(
        dialect::parse_stop_reason("end_turn"),
        Some(dialect::ClaudeStopReason::EndTurn)
    );
}

#[test]
fn parse_stop_reason_tool_use() {
    assert_eq!(
        dialect::parse_stop_reason("tool_use"),
        Some(dialect::ClaudeStopReason::ToolUse)
    );
}

#[test]
fn parse_stop_reason_max_tokens() {
    assert_eq!(
        dialect::parse_stop_reason("max_tokens"),
        Some(dialect::ClaudeStopReason::MaxTokens)
    );
}

#[test]
fn parse_stop_reason_stop_sequence() {
    assert_eq!(
        dialect::parse_stop_reason("stop_sequence"),
        Some(dialect::ClaudeStopReason::StopSequence)
    );
}

#[test]
fn parse_stop_reason_unknown() {
    assert!(dialect::parse_stop_reason("unknown_reason").is_none());
}

#[test]
fn parse_stop_reason_empty() {
    assert!(dialect::parse_stop_reason("").is_none());
}

#[test]
fn map_stop_reason_roundtrip() {
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

// ═══════════════════════════════════════════════════════════════════════════
// Capability manifest
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_has_streaming() {
    let manifest = dialect::capability_manifest();
    assert!(manifest.contains_key(&abp_core::Capability::Streaming));
}

#[test]
fn capability_manifest_has_tools() {
    let manifest = dialect::capability_manifest();
    assert!(manifest.contains_key(&abp_core::Capability::ToolRead));
    assert!(manifest.contains_key(&abp_core::Capability::ToolWrite));
    assert!(manifest.contains_key(&abp_core::Capability::ToolEdit));
    assert!(manifest.contains_key(&abp_core::Capability::ToolBash));
}

#[test]
fn capability_manifest_mcp_server_unsupported() {
    let manifest = dialect::capability_manifest();
    let level = manifest.get(&abp_core::Capability::McpServer);
    assert!(level.is_some());
    let json = serde_json::to_value(level.unwrap()).unwrap();
    assert_eq!(json, serde_json::Value::String("unsupported".into()));
}

// ═══════════════════════════════════════════════════════════════════════════
// Tool definition conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_to_claude_roundtrip() {
    let canonical = dialect::CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file from the workspace".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            },
            "required": ["path"]
        }),
    };
    let claude_def = dialect::tool_def_to_claude(&canonical);
    let back = dialect::tool_def_from_claude(&claude_def);
    assert_eq!(canonical, back);
}

#[test]
fn tool_def_to_claude_fields() {
    let canonical = dialect::CanonicalToolDef {
        name: "bash".into(),
        description: "Run a command".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let claude = dialect::tool_def_to_claude(&canonical);
    assert_eq!(claude.name, "bash");
    assert_eq!(claude.description, "Run a command");
    assert_eq!(claude.input_schema, json!({"type": "object"}));
}

// ═══════════════════════════════════════════════════════════════════════════
// End-to-end roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn full_roundtrip_simple() {
    let client = AnthropicClient::new();
    let resp = client.create(simple_request("Hello")).await.unwrap();
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert!(!resp.content.is_empty());
    assert!(resp.stop_reason.is_some());
}

#[tokio::test]
async fn full_roundtrip_multi_turn() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "What is 2+2?".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text { text: "4".into() }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "And 3+3?".into(),
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
}

#[tokio::test]
async fn full_streaming_roundtrip() {
    let client = AnthropicClient::new();
    let stream = client
        .create_stream(simple_request("Stream me"))
        .await
        .unwrap();
    let events = stream.collect_all().await;
    assert!(!events.is_empty());
    // First event is MessageStart
    assert!(matches!(&events[0], StreamEvent::MessageStart { .. }));
    // Last event is MessageStop
    assert!(matches!(
        events.last().unwrap(),
        StreamEvent::MessageStop {}
    ));
}

#[test]
fn large_content_block_roundtrip() {
    let big_text = "x".repeat(100_000);
    let block = ContentBlock::Text {
        text: big_text.clone(),
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    match back {
        ContentBlock::Text { text } => assert_eq!(text.len(), 100_000),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn many_messages_conversion() {
    let messages: Vec<Message> = (0..100)
        .map(|i| Message {
            role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            content: vec![ContentBlock::Text {
                text: format!("Message {i}"),
            }],
        })
        .collect();
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages,
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    let claude_req = request_to_claude(&req);
    assert_eq!(claude_req.messages.len(), 100);
}

#[test]
fn special_characters_in_text_block() {
    let block = ContentBlock::Text {
        text: "Hello \"world\" & <tag> \n\t\r\0".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn deeply_nested_tool_input() {
    let input = json!({
        "a": {"b": {"c": {"d": {"e": {"f": "deep"}}}}}
    });
    let block = ContentBlock::ToolUse {
        id: "tu_deep".into(),
        name: "deep_tool".into(),
        input,
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn tool_use_null_input() {
    let block = ContentBlock::ToolUse {
        id: "tu_null".into(),
        name: "null_tool".into(),
        input: serde_json::Value::Null,
    };
    let ir = content_block_to_ir(&block);
    let back = content_block_from_ir(&ir);
    assert_eq!(block, back);
}

#[test]
fn message_delta_payload_all_none() {
    let payload = MessageDeltaPayload {
        stop_reason: None,
        stop_sequence: None,
    };
    let json = serde_json::to_string(&payload).unwrap();
    let back: MessageDeltaPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(payload, back);
}

#[test]
fn message_delta_payload_with_stop_sequence() {
    let payload = MessageDeltaPayload {
        stop_reason: Some("stop_sequence".into()),
        stop_sequence: Some("END".into()),
    };
    let json = serde_json::to_string(&payload).unwrap();
    let back: MessageDeltaPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(payload, back);
}

#[test]
fn image_source_base64_serde() {
    let source = ImageSource::Base64 {
        media_type: "image/png".into(),
        data: "AAAA".into(),
    };
    let json = serde_json::to_string(&source).unwrap();
    let back: ImageSource = serde_json::from_str(&json).unwrap();
    assert_eq!(source, back);
}

#[test]
fn image_source_url_serde() {
    let source = ImageSource::Url {
        url: "https://example.com/img.jpg".into(),
    };
    let json = serde_json::to_string(&source).unwrap();
    let back: ImageSource = serde_json::from_str(&json).unwrap();
    assert_eq!(source, back);
}
