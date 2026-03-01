// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the Claude dialect mapping, model names, capabilities, and tool formats.

use abp_claude_sdk::dialect::{
    CanonicalToolDef, ClaudeApiError, ClaudeCacheControl, ClaudeConfig, ClaudeContentBlock,
    ClaudeImageSource, ClaudeMessageDelta, ClaudeResponse, ClaudeStreamDelta, ClaudeStreamEvent,
    ClaudeSystemBlock, ClaudeToolDef, ClaudeUsage, DEFAULT_MODEL, DIALECT_VERSION,
    capability_manifest, from_canonical_model, is_known_model, map_stream_event, map_tool_result,
    to_canonical_model, tool_def_from_claude, tool_def_to_claude,
};
use abp_core::{AgentEventKind, Capability, SupportLevel};

// ---------------------------------------------------------------------------
// Model-name mapping
// ---------------------------------------------------------------------------

#[test]
fn model_roundtrip_known() {
    let vendor = "claude-sonnet-4-20250514";
    let canonical = to_canonical_model(vendor);
    assert_eq!(canonical, "anthropic/claude-sonnet-4-20250514");
    let back = from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn model_roundtrip_opus() {
    let canonical = to_canonical_model("claude-opus-4-20250514");
    assert_eq!(canonical, "anthropic/claude-opus-4-20250514");
    assert_eq!(from_canonical_model(&canonical), "claude-opus-4-20250514");
}

#[test]
fn unknown_model_roundtrips() {
    let canonical = to_canonical_model("claude-future-5");
    assert_eq!(canonical, "anthropic/claude-future-5");
    assert_eq!(from_canonical_model(&canonical), "claude-future-5");
}

#[test]
fn from_canonical_without_prefix_passes_through() {
    assert_eq!(from_canonical_model("bare-model"), "bare-model");
}

#[test]
fn is_known_model_recognises_known() {
    assert!(is_known_model("claude-sonnet-4-20250514"));
    assert!(is_known_model("claude-haiku-3-5-20241022"));
    assert!(!is_known_model("totally-unknown"));
}

// ---------------------------------------------------------------------------
// Default model & version
// ---------------------------------------------------------------------------

#[test]
fn default_model_is_sonnet() {
    assert_eq!(DEFAULT_MODEL, "claude-sonnet-4-20250514");
}

#[test]
fn dialect_version_is_set() {
    assert!(DIALECT_VERSION.starts_with("claude/"));
}

// ---------------------------------------------------------------------------
// Capability mapping
// ---------------------------------------------------------------------------

#[test]
fn capability_manifest_has_streaming_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_mcp_client_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_server_unsupported() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_hooks_native() {
    let m = capability_manifest();
    assert!(matches!(
        m.get(&Capability::HooksPreToolUse),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::HooksPostToolUse),
        Some(SupportLevel::Native)
    ));
}

// ---------------------------------------------------------------------------
// Tool-format conversion
// ---------------------------------------------------------------------------

#[test]
fn tool_def_roundtrip() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file from disk".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    };
    let claude = tool_def_to_claude(&canonical);
    assert_eq!(claude.name, "read_file");
    assert_eq!(claude.description, "Read a file from disk");

    let back = tool_def_from_claude(&claude);
    assert_eq!(back, canonical);
}

#[test]
fn claude_tool_def_serde_roundtrip() {
    let def = ClaudeToolDef {
        name: "bash".into(),
        description: "Run a bash command".into(),
        input_schema: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: ClaudeToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

// ---------------------------------------------------------------------------
// Serde roundtrip of dialect types
// ---------------------------------------------------------------------------

#[test]
fn claude_config_serde_roundtrip() {
    let cfg = ClaudeConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, cfg.model);
    assert_eq!(parsed.base_url, cfg.base_url);
    assert_eq!(parsed.max_tokens, cfg.max_tokens);
}

#[test]
fn claude_response_serde_roundtrip() {
    let resp = ClaudeResponse {
        id: "msg_rt".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "Hello!".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
        ],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 20,
            output_tokens: 10,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "msg_rt");
    assert_eq!(parsed.content.len(), 2);
    assert_eq!(parsed.usage.unwrap().output_tokens, 10);
}

// ---------------------------------------------------------------------------
// Stream event serialization
// ---------------------------------------------------------------------------

#[test]
fn stream_event_ping_roundtrip() {
    let event = ClaudeStreamEvent::Ping {};
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"ping\""));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_message_stop_roundtrip() {
    let event = ClaudeStreamEvent::MessageStop {};
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"message_stop\""));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_error_roundtrip() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "overloaded_error".into(),
            message: "API is overloaded".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_start_roundtrip() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"content_block_start\""));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_stop_roundtrip() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 1 };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_message_delta_roundtrip() {
    let event = ClaudeStreamEvent::MessageDelta {
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
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

// ---------------------------------------------------------------------------
// Delta content handling
// ---------------------------------------------------------------------------

#[test]
fn stream_delta_text_roundtrip() {
    let delta = ClaudeStreamDelta::TextDelta {
        text: "Hello ".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    assert!(json.contains("\"type\":\"text_delta\""));
    let parsed: ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, delta);
}

#[test]
fn stream_delta_input_json_roundtrip() {
    let delta = ClaudeStreamDelta::InputJsonDelta {
        partial_json: r#"{"path":"#.into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    assert!(json.contains("\"type\":\"input_json_delta\""));
    let parsed: ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, delta);
}

#[test]
fn stream_delta_thinking_roundtrip() {
    let delta = ClaudeStreamDelta::ThinkingDelta {
        thinking: "Let me consider...".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    assert!(json.contains("\"type\":\"thinking_delta\""));
    let parsed: ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, delta);
}

#[test]
fn stream_delta_signature_roundtrip() {
    let delta = ClaudeStreamDelta::SignatureDelta {
        signature: "abc123sig".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    assert!(json.contains("\"type\":\"signature_delta\""));
    let parsed: ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, delta);
}

#[test]
fn content_block_delta_event_roundtrip() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "world".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

// ---------------------------------------------------------------------------
// Tool result block mapping
// ---------------------------------------------------------------------------

#[test]
fn tool_result_content_block_roundtrip() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_abc".into(),
        content: Some("file contents here".into()),
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"tool_result\""));
    assert!(json.contains("\"tool_use_id\":\"tu_abc\""));
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn tool_result_with_error_flag() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("command not found".into()),
        is_error: Some(true),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"is_error\":true"));
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn map_tool_result_creates_user_message() {
    let msg = map_tool_result("tu_1", "success output", false);
    assert_eq!(msg.role, "user");
    assert!(msg.content.contains("tool_result"));
    assert!(msg.content.contains("tu_1"));
    assert!(msg.content.contains("success output"));
}

#[test]
fn map_tool_result_with_error() {
    let msg = map_tool_result("tu_err", "failed", true);
    assert_eq!(msg.role, "user");
    assert!(msg.content.contains("is_error"));
}

// ---------------------------------------------------------------------------
// Thinking block serialization
// ---------------------------------------------------------------------------

#[test]
fn thinking_block_roundtrip() {
    let block = ClaudeContentBlock::Thinking {
        thinking: "I need to analyze the code structure.".into(),
        signature: Some("sig_abc".into()),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"thinking\""));
    assert!(json.contains("\"signature\":\"sig_abc\""));
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn thinking_block_without_signature() {
    let block = ClaudeContentBlock::Thinking {
        thinking: "Considering options...".into(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("signature"));
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

// ---------------------------------------------------------------------------
// Image block serialization
// ---------------------------------------------------------------------------

#[test]
fn image_block_base64_roundtrip() {
    let block = ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"image\""));
    assert!(json.contains("\"media_type\":\"image/png\""));
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn image_block_url_roundtrip() {
    let block = ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

// ---------------------------------------------------------------------------
// Cache control serialization
// ---------------------------------------------------------------------------

#[test]
fn system_block_without_cache_control() {
    let block = ClaudeSystemBlock::Text {
        text: "You are a helpful assistant.".into(),
        cache_control: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"type\":\"text\""));
    assert!(!json.contains("cache_control"));
    let parsed: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn system_block_with_ephemeral_cache_control() {
    let block = ClaudeSystemBlock::Text {
        text: "System prompt".into(),
        cache_control: Some(ClaudeCacheControl::ephemeral()),
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(json.contains("\"cache_control\""));
    assert!(json.contains("\"ephemeral\""));
    let parsed: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn usage_with_cache_tokens() {
    let usage = ClaudeUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: Some(80),
        cache_read_input_tokens: Some(20),
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(json.contains("\"cache_creation_input_tokens\":80"));
    assert!(json.contains("\"cache_read_input_tokens\":20"));
    let parsed: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn usage_without_cache_tokens_omits_fields() {
    let usage = ClaudeUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(!json.contains("cache_creation"));
    assert!(!json.contains("cache_read"));
}

// ---------------------------------------------------------------------------
// Stream event → AgentEvent mapping
// ---------------------------------------------------------------------------

#[test]
fn stream_text_delta_maps_to_assistant_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "Hello".into(),
        },
    };
    let agent_events = map_stream_event(&event);
    assert_eq!(agent_events.len(), 1);
    match &agent_events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn stream_input_json_delta_maps_to_nothing() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"pa"#.into(),
        },
    };
    let agent_events = map_stream_event(&event);
    assert!(agent_events.is_empty());
}

#[test]
fn stream_message_start_maps_to_run_started() {
    let event = ClaudeStreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_1".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: None,
        },
    };
    let agent_events = map_stream_event(&event);
    assert_eq!(agent_events.len(), 1);
    assert!(matches!(
        &agent_events[0].kind,
        AgentEventKind::RunStarted { .. }
    ));
}

#[test]
fn stream_message_stop_maps_to_run_completed() {
    let event = ClaudeStreamEvent::MessageStop {};
    let agent_events = map_stream_event(&event);
    assert_eq!(agent_events.len(), 1);
    assert!(matches!(
        &agent_events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn stream_error_maps_to_error_event() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "rate_limit_error".into(),
            message: "Too many requests".into(),
        },
    };
    let agent_events = map_stream_event(&event);
    assert_eq!(agent_events.len(), 1);
    match &agent_events[0].kind {
        AgentEventKind::Error { message } => {
            assert!(message.contains("rate_limit_error"));
            assert!(message.contains("Too many requests"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stream_ping_maps_to_nothing() {
    let event = ClaudeStreamEvent::Ping {};
    let agent_events = map_stream_event(&event);
    assert!(agent_events.is_empty());
}

#[test]
fn stream_content_block_stop_maps_to_nothing() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 0 };
    let agent_events = map_stream_event(&event);
    assert!(agent_events.is_empty());
}

// ---------------------------------------------------------------------------
// Updated model list
// ---------------------------------------------------------------------------

#[test]
fn is_known_model_recognises_claude_4() {
    assert!(is_known_model("claude-4-20250714"));
    assert!(is_known_model("claude-4-latest"));
    assert!(is_known_model("claude-opus-4-latest"));
}
