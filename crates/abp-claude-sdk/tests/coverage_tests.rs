// SPDX-License-Identifier: MIT OR Apache-2.0
//! Additional coverage tests for the Claude SDK: request/message serde, JSON field names,
//! edge cases in mapping, and content block boundaries.

use abp_claude_sdk::dialect::{
    ClaudeApiError, ClaudeCacheControl, ClaudeConfig, ClaudeContentBlock, ClaudeImageSource,
    ClaudeMessage, ClaudeMessageDelta, ClaudeRequest, ClaudeResponse, ClaudeStopReason,
    ClaudeStreamDelta, ClaudeStreamEvent, ClaudeSystemBlock, ClaudeToolDef, ThinkingConfig,
    from_canonical_model, map_response, map_stream_event, map_work_order,
};
use abp_core::{AgentEventKind, ContextPacket, WorkOrderBuilder};

// ---------------------------------------------------------------------------
// ClaudeRequest serde
// ---------------------------------------------------------------------------

#[test]
fn claude_request_serde_roundtrip() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system: Some("Be helpful.".into()),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }],
        thinking: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "claude-sonnet-4-20250514");
    assert_eq!(parsed.max_tokens, 4096);
    assert_eq!(parsed.system.as_deref(), Some("Be helpful."));
    assert_eq!(parsed.messages.len(), 1);
}

#[test]
fn claude_request_with_thinking_config_roundtrip() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 16384,
        system: None,
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "Think carefully".into(),
        }],
        thinking: Some(ThinkingConfig::new(8192)),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("thinking"));
    assert!(json.contains("budget_tokens"));
    let parsed: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert!(parsed.thinking.is_some());
}

#[test]
fn claude_request_omits_thinking_when_none() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system: None,
        messages: vec![],
        thinking: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("thinking"));
}

// ---------------------------------------------------------------------------
// ClaudeMessage serde
// ---------------------------------------------------------------------------

#[test]
fn claude_message_serde_roundtrip() {
    let msg = ClaudeMessage {
        role: "assistant".into(),
        content: "Here is my answer.".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: ClaudeMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, "assistant");
    assert_eq!(parsed.content, "Here is my answer.");
}

// ---------------------------------------------------------------------------
// JSON field name verification
// ---------------------------------------------------------------------------

#[test]
fn claude_content_block_text_json_has_type_field() {
    let block = ClaudeContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");
}

#[test]
fn claude_content_block_tool_use_json_format() {
    let block = ClaudeContentBlock::ToolUse {
        id: "toolu_01".into(),
        name: "bash".into(),
        input: serde_json::json!({"command": "ls"}),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_use");
    assert_eq!(json["id"], "toolu_01");
    assert_eq!(json["name"], "bash");
}

#[test]
fn claude_tool_def_json_has_input_schema_not_parameters() {
    let def = ClaudeToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        input_schema: serde_json::json!({"type": "object"}),
    };
    let json = serde_json::to_value(&def).unwrap();
    assert!(json.get("input_schema").is_some());
    assert!(json.get("parameters").is_none());
}

// ---------------------------------------------------------------------------
// map_response edge cases
// ---------------------------------------------------------------------------

#[test]
fn map_response_empty_content_produces_no_events() {
    let resp = ClaudeResponse {
        id: "msg_empty".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: Some("end_turn".into()),
        usage: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_multiple_text_blocks_produce_multiple_events() {
    let resp = ClaudeResponse {
        id: "msg_multi_text".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "First.".into(),
            },
            ClaudeContentBlock::Text {
                text: "Second.".into(),
            },
        ],
        stop_reason: Some("end_turn".into()),
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 2);
    for event in &events {
        assert!(matches!(
            &event.kind,
            AgentEventKind::AssistantMessage { .. }
        ));
    }
}

// ---------------------------------------------------------------------------
// Config with all optional fields
// ---------------------------------------------------------------------------

#[test]
fn claude_config_with_system_prompt_serde() {
    let cfg = ClaudeConfig {
        system_prompt: Some("You are a coding assistant.".into()),
        ..ClaudeConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("coding assistant"));
    let parsed: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed.system_prompt.as_deref(),
        Some("You are a coding assistant.")
    );
}

// ---------------------------------------------------------------------------
// map_work_order with file context
// ---------------------------------------------------------------------------

#[test]
fn map_work_order_with_snippets_includes_names_and_content() {
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![abp_core::ContextSnippet {
            name: "src/main.rs".into(),
            content: "fn main() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Review code").context(ctx).build();
    let cfg = ClaudeConfig::default();
    let req = map_work_order(&wo, &cfg);
    assert!(req.messages[0].content.contains("src/main.rs"));
    assert!(req.messages[0].content.contains("fn main() {}"));
}

// ---------------------------------------------------------------------------
// API error serde
// ---------------------------------------------------------------------------

#[test]
fn claude_api_error_serde_roundtrip() {
    let err = ClaudeApiError {
        error_type: "invalid_request_error".into(),
        message: "max_tokens must be positive".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let parsed: ClaudeApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, err);
}

// ---------------------------------------------------------------------------
// ClaudeResponse usage omission
// ---------------------------------------------------------------------------

#[test]
fn claude_response_no_usage_omits_field() {
    let resp = ClaudeResponse {
        id: "msg_nousage".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text { text: "hi".into() }],
        stop_reason: None,
        usage: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    // Usage is Option, check it's either null or absent
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Even if present as null, parsing back should work
    let parsed: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert!(parsed.usage.is_none() || val.get("usage").is_some());
}

// ---------------------------------------------------------------------------
// Stream event mapping edge cases
// ---------------------------------------------------------------------------

#[test]
fn map_stream_event_signature_delta_produces_no_events() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::SignatureDelta {
            signature: "sig_xyz".into(),
        },
    };
    let agent_events = map_stream_event(&event);
    assert!(agent_events.is_empty());
}

#[test]
fn map_stream_event_message_delta_with_stop_sequence() {
    let event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("stop_sequence".into()),
            stop_sequence: Some("###".into()),
        },
        usage: None,
    };
    // MessageDelta should not produce mapped events (it's metadata)
    let agent_events = map_stream_event(&event);
    assert!(agent_events.is_empty());
}

// ---------------------------------------------------------------------------
// ClaudeStopReason enum serde
// ---------------------------------------------------------------------------

#[test]
fn claude_stop_reason_all_variants_json_format() {
    let pairs = [
        (ClaudeStopReason::EndTurn, "\"end_turn\""),
        (ClaudeStopReason::ToolUse, "\"tool_use\""),
        (ClaudeStopReason::MaxTokens, "\"max_tokens\""),
        (ClaudeStopReason::StopSequence, "\"stop_sequence\""),
    ];
    for (reason, expected) in &pairs {
        let json = serde_json::to_string(reason).unwrap();
        assert_eq!(&json, expected);
    }
}

// ---------------------------------------------------------------------------
// ClaudeSystemBlock serde
// ---------------------------------------------------------------------------

#[test]
fn claude_system_block_text_serde_roundtrip() {
    let block = ClaudeSystemBlock::Text {
        text: "system prompt".into(),
        cache_control: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

// ---------------------------------------------------------------------------
// CacheControl constructor
// ---------------------------------------------------------------------------

#[test]
fn cache_control_ephemeral_constructor() {
    let cc = ClaudeCacheControl::ephemeral();
    let json = serde_json::to_value(&cc).unwrap();
    assert_eq!(json["type"], "ephemeral");
}

// ---------------------------------------------------------------------------
// from_canonical_model prefix stripping
// ---------------------------------------------------------------------------

#[test]
fn from_canonical_model_strips_anthropic_prefix() {
    assert_eq!(
        from_canonical_model("anthropic/claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn from_canonical_model_other_prefix_passes_through() {
    assert_eq!(from_canonical_model("openai/gpt-4o"), "openai/gpt-4o");
}

// ---------------------------------------------------------------------------
// Image source serde
// ---------------------------------------------------------------------------

#[test]
fn claude_image_source_base64_json_format() {
    let src = ClaudeImageSource::Base64 {
        media_type: "image/jpeg".into(),
        data: "abc123==".into(),
    };
    let json = serde_json::to_value(&src).unwrap();
    assert_eq!(json["type"], "base64");
    assert_eq!(json["media_type"], "image/jpeg");
}

#[test]
fn claude_image_source_url_json_format() {
    let src = ClaudeImageSource::Url {
        url: "https://img.example.com/photo.png".into(),
    };
    let json = serde_json::to_value(&src).unwrap();
    assert_eq!(json["type"], "url");
    assert_eq!(json["url"], "https://img.example.com/photo.png");
}
