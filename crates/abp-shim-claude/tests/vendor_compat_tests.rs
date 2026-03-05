// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vendor-compatibility tests for the Claude (Anthropic) shim.
//!
//! Validates that the public surface mirrors the Anthropic SDK's type names
//! and JSON wire format, including vendor-compat aliases
//! (`MessageCreateParams`, `MessageStreamEvent`).

use abp_shim_claude::{
    AnthropicClient, ContentBlock, Message, MessageCreateParams, MessageRequest, MessageResponse,
    MessageStreamEvent, Role, ShimError, StreamDelta, StreamEvent,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Vendor-compat type alias existence and shape
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_create_params_alias_matches_request() {
    let req: MessageCreateParams = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };
    assert_eq!(req.model, "claude-sonnet-4-20250514");
}

#[test]
fn message_stream_event_alias_matches_stream_event() {
    let event: MessageStreamEvent = StreamEvent::Ping {};
    matches!(event, StreamEvent::Ping {});
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Client.create() pattern
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn anthropic_client_create_default_mock() {
    let client = AnthropicClient::with_model("claude-sonnet-4-20250514");
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "What is 2+2?".into(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert!(!resp.content.is_empty());
}

#[tokio::test]
async fn anthropic_client_create_stream_mock() {
    use tokio_stream::StreamExt;

    let client = AnthropicClient::with_model("claude-sonnet-4-20250514");
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Tell me a story".into(),
            }],
        }],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: Some(true),
    };

    let mut stream = client.create_stream(req).await.unwrap();
    let mut saw_message_start = false;
    let mut saw_message_stop = false;
    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::MessageStart { .. } => saw_message_start = true,
            StreamEvent::MessageStop {} => saw_message_stop = true,
            _ => {}
        }
    }
    assert!(saw_message_start, "should see message_start");
    assert!(saw_message_stop, "should see message_stop");
}

#[tokio::test]
async fn anthropic_client_rejects_empty_messages() {
    let client = AnthropicClient::new();
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
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

// ═══════════════════════════════════════════════════════════════════════════
// 3. Wire-format JSON fidelity — request
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_json_matches_anthropic_wire_format() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Explain quantum computing".into(),
            }],
        }],
        system: Some("You are a physicist.".into()),
        temperature: Some(0.5),
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "claude-sonnet-4-20250514");
    assert_eq!(v["max_tokens"], 4096);
    assert_eq!(v["system"], "You are a physicist.");
    assert_eq!(v["messages"][0]["role"], "user");
    assert_eq!(v["messages"][0]["content"][0]["type"], "text");
    assert_eq!(v["temperature"], 0.5);
    // stream should be omitted when None
    assert!(v.get("stream").is_none());
}

#[test]
fn request_with_tool_use_content_json() {
    let req = MessageRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        messages: vec![
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "What's the weather?".into(),
                }],
            },
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "toolu_01".into(),
                    name: "get_weather".into(),
                    input: json!({"location": "NYC"}),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_01".into(),
                    content: Some("Sunny, 72°F".into()),
                    is_error: None,
                }],
            },
        ],
        system: None,
        temperature: None,
        stop_sequences: None,
        thinking: None,
        stream: None,
    };

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["messages"][1]["content"][0]["type"], "tool_use");
    assert_eq!(v["messages"][1]["content"][0]["name"], "get_weather");
    assert_eq!(v["messages"][2]["content"][0]["type"], "tool_result");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Wire-format JSON fidelity — response
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_from_real_anthropic_json() {
    let json_str = r#"{
        "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello! How can I help?"}],
        "model": "claude-sonnet-4-20250514",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 12,
            "output_tokens": 8
        }
    }"#;

    let resp: MessageResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(resp.id, "msg_01XFDUDYJgAACzvnptvVoYEL");
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(resp.usage.input_tokens, 12);
    assert_eq!(resp.usage.output_tokens, 8);
    match &resp.content[0] {
        ContentBlock::Text { text } => assert_eq!(text, "Hello! How can I help?"),
        _ => panic!("expected text block"),
    }
}

#[test]
fn response_with_tool_use_from_json() {
    let json_str = r#"{
        "id": "msg_abc",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "tool_use",
            "id": "toolu_01",
            "name": "get_weather",
            "input": {"location": "NYC"}
        }],
        "model": "claude-sonnet-4-20250514",
        "stop_reason": "tool_use",
        "stop_sequence": null,
        "usage": {"input_tokens": 20, "output_tokens": 30}
    }"#;

    let resp: MessageResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "toolu_01");
            assert_eq!(name, "get_weather");
            assert_eq!(input["location"], "NYC");
        }
        _ => panic!("expected tool_use block"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Streaming event wire format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_message_start_from_json() {
    let json_str = r#"{
        "type": "message_start",
        "message": {
            "id": "msg_abc",
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 0}
        }
    }"#;

    let event: StreamEvent = serde_json::from_str(json_str).unwrap();
    match event {
        StreamEvent::MessageStart { message } => {
            assert_eq!(message.id, "msg_abc");
            assert!(message.content.is_empty());
        }
        _ => panic!("expected message_start"),
    }
}

#[test]
fn stream_event_content_block_delta_text() {
    let json_str = r#"{
        "type": "content_block_delta",
        "index": 0,
        "delta": {"type": "text_delta", "text": "Hello"}
    }"#;

    let event: StreamEvent = serde_json::from_str(json_str).unwrap();
    match event {
        StreamEvent::ContentBlockDelta { index, delta } => {
            assert_eq!(index, 0);
            match delta {
                StreamDelta::TextDelta { text } => assert_eq!(text, "Hello"),
                _ => panic!("expected text_delta"),
            }
        }
        _ => panic!("expected content_block_delta"),
    }
}

#[test]
fn stream_event_message_delta_with_stop_reason() {
    let json_str = r#"{
        "type": "message_delta",
        "delta": {"stop_reason": "end_turn"},
        "usage": {"input_tokens": 10, "output_tokens": 25}
    }"#;

    let event: StreamEvent = serde_json::from_str(json_str).unwrap();
    match event {
        StreamEvent::MessageDelta { delta, usage } => {
            assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
            assert_eq!(usage.unwrap().output_tokens, 25);
        }
        _ => panic!("expected message_delta"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Content block roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_block_roundtrip() {
    let block = ContentBlock::Thinking {
        thinking: "Let me reason about this...".into(),
        signature: Some("sig_abc".into()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}
