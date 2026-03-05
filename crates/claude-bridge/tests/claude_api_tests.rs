// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for Claude Messages API types, SSE parsing, and translation to ABP IR.

use serde_json::{json, Value};

use claude_bridge::claude_types::*;
use claude_bridge::sse::SseParser;

// ═══════════════════════════════════════════════════════════════════
// 1. Claude types — serde round-trips
// ═══════════════════════════════════════════════════════════════════

mod types_serde {
    use super::*;

    #[test]
    fn role_roundtrip() {
        let json_str = serde_json::to_string(&Role::User).unwrap();
        assert_eq!(json_str, r#""user""#);
        let rt: Role = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rt, Role::User);

        let json_str = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json_str, r#""assistant""#);
    }

    #[test]
    fn text_content_block_roundtrip() {
        let block = ContentBlock::Text {
            text: "Hello".into(),
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "text");
        assert_eq!(v["text"], "Hello");
        let rt: ContentBlock = serde_json::from_value(v).unwrap();
        assert_eq!(rt, block);
    }

    #[test]
    fn tool_use_block_roundtrip() {
        let block = ContentBlock::ToolUse {
            id: "toolu_01".into(),
            name: "read_file".into(),
            input: json!({"path": "/src/main.rs"}),
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_use");
        assert_eq!(v["name"], "read_file");
        assert_eq!(v["input"]["path"], "/src/main.rs");
        let rt: ContentBlock = serde_json::from_value(v).unwrap();
        assert_eq!(rt, block);
    }

    #[test]
    fn tool_result_block_roundtrip() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_01".into(),
            content: Some("file contents here".into()),
            is_error: None,
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["tool_use_id"], "toolu_01");
        assert!(v.get("is_error").is_none());
        let rt: ContentBlock = serde_json::from_value(v).unwrap();
        assert_eq!(rt, block);
    }

    #[test]
    fn tool_result_error_block() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_02".into(),
            content: Some("permission denied".into()),
            is_error: Some(true),
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["is_error"], true);
    }

    #[test]
    fn thinking_block_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "Let me analyze this...".into(),
            signature: Some("sig123".into()),
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "thinking");
        assert_eq!(v["thinking"], "Let me analyze this...");
        assert_eq!(v["signature"], "sig123");
        let rt: ContentBlock = serde_json::from_value(v).unwrap();
        assert_eq!(rt, block);
    }

    #[test]
    fn thinking_block_no_signature() {
        let block = ContentBlock::Thinking {
            thinking: "hmm".into(),
            signature: None,
        };
        let v = serde_json::to_value(&block).unwrap();
        assert!(v.get("signature").is_none());
    }

    #[test]
    fn image_base64_block_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBOR...".into(),
            },
        };
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "image");
        assert_eq!(v["source"]["type"], "base64");
        assert_eq!(v["source"]["media_type"], "image/png");
    }

    #[test]
    fn message_content_text_untagged() {
        let mc: MessageContent = serde_json::from_value(json!("hello")).unwrap();
        assert!(matches!(mc, MessageContent::Text(s) if s == "hello"));
    }

    #[test]
    fn message_content_blocks_untagged() {
        let mc: MessageContent =
            serde_json::from_value(json!([{"type": "text", "text": "hi"}])).unwrap();
        match mc {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "hi"));
            }
            _ => panic!("expected Blocks"),
        }
    }

    #[test]
    fn usage_roundtrip() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: None,
        };
        let v = serde_json::to_value(usage).unwrap();
        assert_eq!(v["input_tokens"], 100);
        assert_eq!(v["output_tokens"], 50);
        assert_eq!(v["cache_creation_input_tokens"], 10);
        assert!(v.get("cache_read_input_tokens").is_none());
        let rt: Usage = serde_json::from_value(v).unwrap();
        assert_eq!(rt, usage);
    }

    #[test]
    fn usage_minimal() {
        let v = json!({"input_tokens": 5, "output_tokens": 3});
        let usage: Usage = serde_json::from_value(v).unwrap();
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.output_tokens, 3);
        assert!(usage.cache_creation_input_tokens.is_none());
        assert!(usage.cache_read_input_tokens.is_none());
    }

    #[test]
    fn messages_request_minimal() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("hello".into()),
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
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "claude-sonnet-4-20250514");
        assert_eq!(v["max_tokens"], 1024);
        // Optional fields should not appear
        assert!(v.get("system").is_none());
        assert!(v.get("tools").is_none());
        assert!(v.get("temperature").is_none());
    }

    #[test]
    fn messages_request_full() {
        let req = MessagesRequest {
            model: "claude-sonnet-4-20250514".into(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("task".into()),
            }],
            max_tokens: 4096,
            system: Some(SystemMessage::Text("You are helpful.".into())),
            tools: Some(vec![ToolDefinition {
                name: "read_file".into(),
                description: "Read a file".into(),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }]),
            metadata: Some(RequestMetadata {
                user_id: Some("user-1".into()),
            }),
            stream: Some(true),
            stop_sequences: Some(vec!["STOP".into()]),
            temperature: Some(0.7),
            top_p: Some(0.9),
            top_k: Some(40),
            tool_choice: Some(ToolChoice::Auto {}),
            thinking: Some(ThinkingConfig {
                thinking_type: "enabled".into(),
                budget_tokens: 2048,
            }),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["system"], "You are helpful.");
        assert!(v["tools"].is_array());
        assert_eq!(v["stream"], true);
        assert_eq!(v["temperature"], 0.7);
        assert_eq!(v["thinking"]["budget_tokens"], 2048);
    }

    #[test]
    fn messages_response_roundtrip() {
        let resp = MessagesResponse {
            id: "msg_01".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Hello!".into(),
            }],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["id"], "msg_01");
        assert_eq!(v["type"], "message");
        assert_eq!(v["stop_reason"], "end_turn");
        let rt: MessagesResponse = serde_json::from_value(v).unwrap();
        assert_eq!(rt.id, "msg_01");
    }

    #[test]
    fn stop_reason_variants() {
        let cases = [
            (StopReason::EndTurn, "end_turn"),
            (StopReason::MaxTokens, "max_tokens"),
            (StopReason::StopSequence, "stop_sequence"),
            (StopReason::ToolUse, "tool_use"),
        ];
        for (variant, expected) in cases {
            let v = serde_json::to_value(variant).unwrap();
            assert_eq!(v.as_str().unwrap(), expected);
        }
    }

    #[test]
    fn tool_choice_variants() {
        let auto = ToolChoice::Auto {};
        let v = serde_json::to_value(&auto).unwrap();
        assert_eq!(v["type"], "auto");

        let any = ToolChoice::Any {};
        let v = serde_json::to_value(&any).unwrap();
        assert_eq!(v["type"], "any");

        let specific = ToolChoice::Tool {
            name: "edit".into(),
        };
        let v = serde_json::to_value(&specific).unwrap();
        assert_eq!(v["type"], "tool");
        assert_eq!(v["name"], "edit");
    }

    #[test]
    fn system_message_text() {
        let sys: SystemMessage = serde_json::from_value(json!("Be helpful.")).unwrap();
        assert!(matches!(sys, SystemMessage::Text(t) if t == "Be helpful."));
    }

    #[test]
    fn system_message_blocks() {
        let sys: SystemMessage = serde_json::from_value(
            json!([{"type": "text", "text": "Be helpful.", "cache_control": {"type": "ephemeral"}}]),
        )
        .unwrap();
        match sys {
            SystemMessage::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
            }
            _ => panic!("expected Blocks"),
        }
    }

    #[test]
    fn api_error_roundtrip() {
        let err = ApiError {
            error_type: "invalid_request_error".into(),
            message: "max_tokens too large".into(),
        };
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["type"], "invalid_request_error");
        let rt: ApiError = serde_json::from_value(v).unwrap();
        assert_eq!(rt, err);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 2. Stream events — serde
// ═══════════════════════════════════════════════════════════════════

mod stream_events {
    use super::*;

    fn sample_message_start() -> Value {
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_01",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-20250514",
                "stop_reason": null,
                "usage": {"input_tokens": 25, "output_tokens": 0}
            }
        })
    }

    #[test]
    fn parse_message_start() {
        let event: StreamEvent = serde_json::from_value(sample_message_start()).unwrap();
        match event {
            StreamEvent::MessageStart { message } => {
                assert_eq!(message.id, "msg_01");
                assert_eq!(message.usage.input_tokens, 25);
            }
            _ => panic!("expected MessageStart"),
        }
    }

    #[test]
    fn parse_content_block_start_text() {
        let v = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        assert!(matches!(
            event,
            StreamEvent::ContentBlockStart { index: 0, .. }
        ));
    }

    #[test]
    fn parse_content_block_start_tool_use() {
        let v = json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "tool_use", "id": "toolu_01", "name": "edit", "input": {}}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        match event {
            StreamEvent::ContentBlockStart {
                index: 1,
                content_block: ContentBlock::ToolUse { id, name, .. },
            } => {
                assert_eq!(id, "toolu_01");
                assert_eq!(name, "edit");
            }
            _ => panic!("expected ContentBlockStart with ToolUse"),
        }
    }

    #[test]
    fn parse_text_delta() {
        let v = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        match event {
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta { text },
            } => {
                assert_eq!(text, "Hello");
            }
            _ => panic!("expected text delta"),
        }
    }

    #[test]
    fn parse_input_json_delta() {
        let v = json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        match event {
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::InputJsonDelta { partial_json },
                ..
            } => {
                assert_eq!(partial_json, "{\"path\":");
            }
            _ => panic!("expected input_json_delta"),
        }
    }

    #[test]
    fn parse_thinking_delta() {
        let v = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "Let me think..."}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        assert!(matches!(
            event,
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::ThinkingDelta { .. },
                ..
            }
        ));
    }

    #[test]
    fn parse_signature_delta() {
        let v = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "signature_delta", "signature": "abc"}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        assert!(matches!(
            event,
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::SignatureDelta { .. },
                ..
            }
        ));
    }

    #[test]
    fn parse_content_block_stop() {
        let v = json!({"type": "content_block_stop", "index": 0});
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        assert!(matches!(event, StreamEvent::ContentBlockStop { index: 0 }));
    }

    #[test]
    fn parse_message_delta() {
        let v = json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"input_tokens": 0, "output_tokens": 42}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        match event {
            StreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
                assert_eq!(usage.unwrap().output_tokens, 42);
            }
            _ => panic!("expected MessageDelta"),
        }
    }

    #[test]
    fn parse_message_stop() {
        let v = json!({"type": "message_stop"});
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        assert!(matches!(event, StreamEvent::MessageStop {}));
    }

    #[test]
    fn parse_ping() {
        let v = json!({"type": "ping"});
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        assert!(matches!(event, StreamEvent::Ping {}));
    }

    #[test]
    fn parse_error_event() {
        let v = json!({
            "type": "error",
            "error": {"type": "overloaded_error", "message": "server busy"}
        });
        let event: StreamEvent = serde_json::from_value(v).unwrap();
        match event {
            StreamEvent::Error { error } => {
                assert_eq!(error.error_type, "overloaded_error");
                assert_eq!(error.message, "server busy");
            }
            _ => panic!("expected Error"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 3. SSE parser
// ═══════════════════════════════════════════════════════════════════

mod sse_parsing {
    use super::*;

    #[test]
    fn parse_single_event() {
        let text = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-20250514\",\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n";
        let events = SseParser::parse_all(text).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
    }

    #[test]
    fn parse_multiple_events() {
        let text = "\
event: ping\n\
data: {\"type\":\"ping\"}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";
        let events = SseParser::parse_all(text).unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], StreamEvent::Ping {}));
        assert!(matches!(events[1], StreamEvent::ContentBlockDelta { .. }));
        assert!(matches!(events[2], StreamEvent::MessageStop {}));
    }

    #[test]
    fn incremental_feed() {
        let mut parser = SseParser::new();
        assert!(parser.feed_line("event: ping").unwrap().is_none());
        assert!(parser
            .feed_line("data: {\"type\":\"ping\"}")
            .unwrap()
            .is_none());
        let event = parser.feed_line("").unwrap().unwrap();
        assert!(matches!(event, StreamEvent::Ping {}));
    }

    #[test]
    fn comment_lines_ignored() {
        let text = ": this is a comment\nevent: ping\ndata: {\"type\":\"ping\"}\n\n";
        let events = SseParser::parse_all(text).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn empty_input() {
        let events = SseParser::parse_all("").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn blank_lines_only() {
        let events = SseParser::parse_all("\n\n\n").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn trailing_event_no_blank_line() {
        let text = "event: ping\ndata: {\"type\":\"ping\"}";
        let events = SseParser::parse_all(text).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn invalid_json_returns_error() {
        let text = "event: foo\ndata: not-json\n\n";
        let result = SseParser::parse_all(text);
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════
// 4. Translation (normalized feature)
// ═══════════════════════════════════════════════════════════════════

#[cfg(feature = "normalized")]
mod translation {
    use super::*;
    use abp_core::ir::{IrContentBlock, IrRole, IrUsage};
    use claude_bridge::translate::*;

    // ── Role mapping ────────────────────────────────────────────────

    #[test]
    fn role_user_roundtrip() {
        assert_eq!(role_to_ir(Role::User), IrRole::User);
        assert_eq!(role_from_ir(IrRole::User), Role::User);
    }

    #[test]
    fn role_assistant_roundtrip() {
        assert_eq!(role_to_ir(Role::Assistant), IrRole::Assistant);
        assert_eq!(role_from_ir(IrRole::Assistant), Role::Assistant);
    }

    #[test]
    fn role_system_maps_to_user() {
        assert_eq!(role_from_ir(IrRole::System), Role::User);
    }

    #[test]
    fn role_tool_maps_to_user() {
        assert_eq!(role_from_ir(IrRole::Tool), Role::User);
    }

    // ── Content block mapping ───────────────────────────────────────

    #[test]
    fn text_block_to_ir() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let ir = content_block_to_ir(&block);
        assert_eq!(
            ir,
            IrContentBlock::Text {
                text: "hello".into()
            }
        );
    }

    #[test]
    fn text_block_from_ir() {
        let ir = IrContentBlock::Text {
            text: "world".into(),
        };
        let block = content_block_from_ir(&ir);
        assert_eq!(
            block,
            ContentBlock::Text {
                text: "world".into()
            }
        );
    }

    #[test]
    fn tool_use_block_to_ir() {
        let block = ContentBlock::ToolUse {
            id: "t1".into(),
            name: "read".into(),
            input: json!({"path": "/a"}),
        };
        let ir = content_block_to_ir(&block);
        match ir {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "t1");
                assert_eq!(name, "read");
                assert_eq!(input["path"], "/a");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn tool_use_block_roundtrip() {
        let block = ContentBlock::ToolUse {
            id: "t1".into(),
            name: "edit".into(),
            input: json!({"file": "x.rs"}),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn tool_result_to_ir_with_content() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: Some("result text".into()),
            is_error: None,
        };
        let ir = content_block_to_ir(&block);
        match ir {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "t1");
                assert_eq!(content.len(), 1);
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_to_ir_error() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "t2".into(),
            content: Some("fail".into()),
            is_error: Some(true),
        };
        let ir = content_block_to_ir(&block);
        match ir {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_to_ir_no_content() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "t3".into(),
            content: None,
            is_error: None,
        };
        let ir = content_block_to_ir(&block);
        match ir {
            IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_from_ir_omits_empty_content() {
        let ir = IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![],
            is_error: false,
        };
        let block = content_block_from_ir(&ir);
        match block {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(content.is_none());
                assert!(is_error.is_none());
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn thinking_block_to_ir() {
        let block = ContentBlock::Thinking {
            thinking: "analyzing...".into(),
            signature: Some("sig".into()),
        };
        let ir = content_block_to_ir(&block);
        assert_eq!(
            ir,
            IrContentBlock::Thinking {
                text: "analyzing...".into()
            }
        );
    }

    #[test]
    fn thinking_block_from_ir() {
        let ir = IrContentBlock::Thinking {
            text: "deep thought".into(),
        };
        let block = content_block_from_ir(&ir);
        match block {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "deep thought");
                assert!(signature.is_none());
            }
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn image_base64_to_ir() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc123".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        match ir {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "abc123");
            }
            _ => panic!("expected Image"),
        }
    }

    #[test]
    fn image_url_to_ir_becomes_text() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        match ir {
            IrContentBlock::Text { text } => {
                assert!(text.contains("https://example.com/img.png"));
            }
            _ => panic!("expected Text fallback for URL image"),
        }
    }

    #[test]
    fn image_from_ir_roundtrip() {
        let ir = IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data".into(),
        };
        let block = content_block_from_ir(&ir);
        let back = content_block_to_ir(&block);
        assert_eq!(back, ir);
    }

    // ── Message mapping ─────────────────────────────────────────────

    #[test]
    fn message_text_to_ir() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text("hi".into()),
        };
        let ir = message_to_ir(&msg);
        assert_eq!(ir.role, IrRole::User);
        assert_eq!(ir.text_content(), "hi");
    }

    #[test]
    fn message_blocks_to_ir() {
        let msg = Message {
            role: Role::Assistant,
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Here:".into(),
                },
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({}),
                },
            ]),
        };
        let ir = message_to_ir(&msg);
        assert_eq!(ir.role, IrRole::Assistant);
        assert_eq!(ir.content.len(), 2);
    }

    #[test]
    fn message_roundtrip() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::Text {
                text: "task".into(),
            }]),
        };
        let ir = message_to_ir(&msg);
        let back = message_from_ir(&ir);
        assert_eq!(back.role, msg.role);
    }

    // ── Conversation mapping ────────────────────────────────────────

    #[test]
    fn conversation_with_system() {
        let messages = vec![Message {
            role: Role::User,
            content: MessageContent::Text("do it".into()),
        }];
        let sys = SystemMessage::Text("you are helpful".into());
        let conv = conversation_to_ir(&messages, Some(&sys));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "you are helpful");
        assert_eq!(conv.messages[1].role, IrRole::User);
    }

    #[test]
    fn conversation_without_system() {
        let messages = vec![
            Message {
                role: Role::User,
                content: MessageContent::Text("hello".into()),
            },
            Message {
                role: Role::Assistant,
                content: MessageContent::Text("hi".into()),
            },
        ];
        let conv = conversation_to_ir(&messages, None);
        assert_eq!(conv.len(), 2);
        assert!(conv.system_message().is_none());
    }

    #[test]
    fn conversation_system_blocks() {
        let sys = SystemMessage::Blocks(vec![
            SystemBlock::Text {
                text: "line1".into(),
                cache_control: None,
            },
            SystemBlock::Text {
                text: "line2".into(),
                cache_control: None,
            },
        ]);
        let conv = conversation_to_ir(&[], Some(&sys));
        assert_eq!(conv.messages[0].text_content(), "line1\nline2");
    }

    // ── Tool definition mapping ─────────────────────────────────────

    #[test]
    fn tool_def_roundtrip() {
        let def = ToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file".into(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let ir = tool_def_to_ir(&def);
        assert_eq!(ir.name, "write_file");
        assert_eq!(ir.description, "Write content to a file");
        let back = tool_def_from_ir(&ir);
        assert_eq!(back, def);
    }

    // ── Usage mapping ───────────────────────────────────────────────

    #[test]
    fn usage_to_ir_basic() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let ir = usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
        assert_eq!(ir.cache_read_tokens, 0);
        assert_eq!(ir.cache_write_tokens, 0);
    }

    #[test]
    fn usage_to_ir_with_cache() {
        let usage = Usage {
            input_tokens: 200,
            output_tokens: 100,
            cache_creation_input_tokens: Some(50),
            cache_read_input_tokens: Some(30),
        };
        let ir = usage_to_ir(&usage);
        assert_eq!(ir.cache_read_tokens, 30);
        assert_eq!(ir.cache_write_tokens, 50);
    }

    #[test]
    fn usage_from_ir_basic() {
        let ir = IrUsage::from_io(100, 50);
        let usage = usage_from_ir(&ir);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert!(usage.cache_creation_input_tokens.is_none());
        assert!(usage.cache_read_input_tokens.is_none());
    }

    #[test]
    fn usage_from_ir_with_cache() {
        let ir = IrUsage::with_cache(200, 100, 30, 50);
        let usage = usage_from_ir(&ir);
        assert_eq!(usage.cache_creation_input_tokens, Some(50));
        assert_eq!(usage.cache_read_input_tokens, Some(30));
    }

    #[test]
    fn usage_roundtrip() {
        let original = Usage {
            input_tokens: 500,
            output_tokens: 250,
            cache_creation_input_tokens: Some(100),
            cache_read_input_tokens: Some(75),
        };
        let ir = usage_to_ir(&original);
        let back = usage_from_ir(&ir);
        assert_eq!(back, original);
    }

    #[test]
    fn merge_usage_both_have_cache() {
        let a = Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: Some(2),
            cache_read_input_tokens: Some(3),
        };
        let b = Usage {
            input_tokens: 20,
            output_tokens: 10,
            cache_creation_input_tokens: Some(4),
            cache_read_input_tokens: Some(6),
        };
        let merged = merge_usage(&a, &b);
        assert_eq!(merged.input_tokens, 30);
        assert_eq!(merged.output_tokens, 15);
        assert_eq!(merged.cache_creation_input_tokens, Some(6));
        assert_eq!(merged.cache_read_input_tokens, Some(9));
    }

    #[test]
    fn merge_usage_one_has_cache() {
        let a = Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: Some(2),
            cache_read_input_tokens: None,
        };
        let b = Usage {
            input_tokens: 20,
            output_tokens: 10,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let merged = merge_usage(&a, &b);
        assert_eq!(merged.cache_creation_input_tokens, Some(2));
        assert!(merged.cache_read_input_tokens.is_none());
    }

    // ── Response mapping ────────────────────────────────────────────

    #[test]
    fn test_response_to_ir_message() {
        let resp = MessagesResponse {
            id: "msg_01".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![
                ContentBlock::Text {
                    text: "Here's the file:".into(),
                },
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "main.rs"}),
                },
            ],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: Some("tool_use".into()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let msg = response_to_ir_message(&resp);
        assert_eq!(msg.role, IrRole::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert!(
            msg.content[0]
                == IrContentBlock::Text {
                    text: "Here's the file:".into()
                }
        );
    }

    #[test]
    fn extract_usage_from_response() {
        let resp = MessagesResponse {
            id: "msg_01".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage {
                input_tokens: 42,
                output_tokens: 17,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let ir = extract_usage(&resp);
        assert_eq!(ir.input_tokens, 42);
        assert_eq!(ir.output_tokens, 17);
        assert_eq!(ir.total_tokens, 59);
    }

    // ── Stream accumulator ──────────────────────────────────────────

    #[test]
    fn accumulator_basic_text_stream() {
        let events: Vec<StreamEvent> = vec![
            serde_json::from_value(json!({
                "type": "message_start",
                "message": {
                    "id": "msg_01", "type": "message", "role": "assistant",
                    "content": [], "model": "claude-sonnet-4-20250514",
                    "stop_reason": null,
                    "usage": {"input_tokens": 10, "output_tokens": 0}
                }
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": " world"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_stop", "index": 0
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"input_tokens": 0, "output_tokens": 15}
            }))
            .unwrap(),
            serde_json::from_value(json!({"type": "message_stop"})).unwrap(),
        ];

        let mut accum = StreamAccumulator::new();
        let mut deltas = Vec::new();
        for event in &events {
            if let Some(frag) = accum.feed(event) {
                deltas.push(frag);
            }
        }

        assert_eq!(
            deltas,
            vec![
                StreamFragment::TextDelta("Hello".into()),
                StreamFragment::TextDelta(" world".into()),
            ]
        );
        assert_eq!(accum.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(accum.model.as_deref(), Some("claude-sonnet-4-20250514"));

        let (blocks, usage) = accum.finish();
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            IrContentBlock::Text {
                text: "Hello world".into()
            }
        );
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 15);
    }

    #[test]
    fn accumulator_tool_use_stream() {
        let events: Vec<StreamEvent> = vec![
            serde_json::from_value(json!({
                "type": "message_start",
                "message": {
                    "id": "msg_02", "type": "message", "role": "assistant",
                    "content": [], "model": "claude-sonnet-4-20250514",
                    "stop_reason": null,
                    "usage": {"input_tokens": 20, "output_tokens": 0}
                }
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_01", "name": "read_file", "input": {}}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "\"/src/main.rs\"}"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_stop", "index": 0
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"},
                "usage": {"input_tokens": 0, "output_tokens": 30}
            }))
            .unwrap(),
        ];

        let mut accum = StreamAccumulator::new();
        for event in &events {
            accum.feed(event);
        }

        let (blocks, _) = accum.finish();
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_01");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "/src/main.rs");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn accumulator_thinking_stream() {
        let events: Vec<StreamEvent> = vec![
            serde_json::from_value(json!({
                "type": "message_start",
                "message": {
                    "id": "msg_03", "type": "message", "role": "assistant",
                    "content": [], "model": "claude-sonnet-4-20250514",
                    "stop_reason": null,
                    "usage": {"input_tokens": 5, "output_tokens": 0}
                }
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "thinking", "thinking": ""}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": "Let me think"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": " about this"}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "type": "content_block_stop", "index": 0
            }))
            .unwrap(),
        ];

        let mut accum = StreamAccumulator::new();
        let mut thinking_deltas = Vec::new();
        for event in &events {
            if let Some(StreamFragment::ThinkingDelta(t)) = accum.feed(event) {
                thinking_deltas.push(t);
            }
        }

        assert_eq!(thinking_deltas, vec!["Let me think", " about this"]);
        let (blocks, _) = accum.finish();
        assert_eq!(
            blocks[0],
            IrContentBlock::Thinking {
                text: "Let me think about this".into()
            }
        );
    }

    #[test]
    fn accumulator_error_fragment() {
        let event: StreamEvent = serde_json::from_value(json!({
            "type": "error",
            "error": {"type": "overloaded_error", "message": "busy"}
        }))
        .unwrap();

        let mut accum = StreamAccumulator::new();
        let frag = accum.feed(&event);
        match frag {
            Some(StreamFragment::Error(e)) => {
                assert_eq!(e.error_type, "overloaded_error");
            }
            _ => panic!("expected Error fragment"),
        }
    }

    // ── Error translation ───────────────────────────────────────────

    #[test]
    fn auth_error_to_bridge() {
        let err = ApiError {
            error_type: "authentication_error".into(),
            message: "invalid key".into(),
        };
        let bridge = api_error_to_bridge(&err);
        let msg = bridge.to_string();
        assert!(msg.contains("authentication"), "got: {msg}");
    }

    #[test]
    fn invalid_request_to_bridge() {
        let err = ApiError {
            error_type: "invalid_request_error".into(),
            message: "bad param".into(),
        };
        let bridge = api_error_to_bridge(&err);
        assert!(matches!(bridge, claude_bridge::BridgeError::Config(_)));
    }

    #[test]
    fn rate_limit_to_bridge() {
        let err = ApiError {
            error_type: "rate_limit_error".into(),
            message: "slow down".into(),
        };
        let bridge = api_error_to_bridge(&err);
        assert!(matches!(bridge, claude_bridge::BridgeError::Run(_)));
        assert!(bridge.to_string().contains("rate limited"));
    }

    #[test]
    fn overloaded_to_bridge() {
        let err = ApiError {
            error_type: "overloaded_error".into(),
            message: "too many requests".into(),
        };
        let bridge = api_error_to_bridge(&err);
        assert!(bridge.to_string().contains("overloaded"));
    }

    #[test]
    fn server_error_to_bridge() {
        let err = ApiError {
            error_type: "api_error".into(),
            message: "internal".into(),
        };
        let bridge = api_error_to_bridge(&err);
        assert!(bridge.to_string().contains("server error"));
    }

    #[test]
    fn unknown_error_to_bridge() {
        let err = ApiError {
            error_type: "some_new_error".into(),
            message: "something new".into(),
        };
        let bridge = api_error_to_bridge(&err);
        let msg = bridge.to_string();
        assert!(msg.contains("some_new_error"));
    }

    // ── Request construction ────────────────────────────────────────

    #[test]
    fn task_to_request_basic() {
        let req = task_to_request("fix the bug", "claude-sonnet-4-20250514", 4096);
        assert_eq!(req.model, "claude-sonnet-4-20250514");
        assert_eq!(req.max_tokens, 4096);
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, Role::User);
        match &req.messages[0].content {
            MessageContent::Text(t) => assert_eq!(t, "fix the bug"),
            _ => panic!("expected Text content"),
        }
        assert!(req.system.is_none());
        assert!(req.tools.is_none());
    }
}
