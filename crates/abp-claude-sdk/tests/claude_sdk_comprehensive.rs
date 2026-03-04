#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use std::collections::BTreeMap;

use abp_claude_sdk::dialect::{
    self, CanonicalToolDef, ClaudeApiError, ClaudeCacheControl, ClaudeConfig, ClaudeContentBlock,
    ClaudeImageSource, ClaudeMessage, ClaudeMessageDelta, ClaudeRequest, ClaudeResponse,
    ClaudeStopReason, ClaudeStreamDelta, ClaudeStreamEvent, ClaudeSystemBlock, ClaudeToolDef,
    ClaudeUsage, ThinkingConfig,
};
use abp_claude_sdk::lowering;
use abp_claude_sdk::messages::{
    CacheControl, ContentBlock, ImageSource, Message, MessageContent, MessageDelta,
    MessagesRequest, MessagesResponse, Metadata, Role, StreamDelta, StreamEvent, SystemBlock,
    SystemMessage, Tool, Usage,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, Outcome, ReceiptBuilder, RuntimeConfig, SupportLevel,
    UsageNormalized, WorkOrderBuilder,
};
use serde_json::json;

// ===========================================================================
// Module 1: dialect constants and helpers
// ===========================================================================

#[test]
fn dialect_version_is_set() {
    assert_eq!(dialect::DIALECT_VERSION, "claude/v0.1");
}

#[test]
fn default_model_is_set() {
    assert_eq!(dialect::DEFAULT_MODEL, "claude-sonnet-4-20250514");
}

#[test]
fn to_canonical_model_prepends_anthropic() {
    assert_eq!(
        dialect::to_canonical_model("claude-sonnet-4-20250514"),
        "anthropic/claude-sonnet-4-20250514"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn from_canonical_model_no_prefix_returns_unchanged() {
    assert_eq!(
        dialect::from_canonical_model("claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn to_and_from_canonical_model_roundtrip() {
    let vendor = "claude-opus-4-20250514";
    let canonical = dialect::to_canonical_model(vendor);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, vendor);
}

#[test]
fn is_known_model_recognizes_sonnet() {
    assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
}

#[test]
fn is_known_model_recognizes_opus() {
    assert!(dialect::is_known_model("claude-opus-4-20250514"));
}

#[test]
fn is_known_model_recognizes_haiku() {
    assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
}

#[test]
fn is_known_model_rejects_unknown() {
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model(""));
}

#[test]
fn is_known_model_recognizes_latest_variants() {
    assert!(dialect::is_known_model("claude-sonnet-4-latest"));
    assert!(dialect::is_known_model("claude-opus-4-latest"));
    assert!(dialect::is_known_model("claude-3-5-haiku-latest"));
    assert!(dialect::is_known_model("claude-4-latest"));
}

// ===========================================================================
// Module 2: CapabilityManifest
// ===========================================================================

#[test]
fn capability_manifest_contains_streaming() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_tool_read_write_edit() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolWrite),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolEdit),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_server_unsupported() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_checkpointing_emulated() {
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Checkpointing),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn capability_manifest_has_expected_entry_count() {
    let m = dialect::capability_manifest();
    assert!(m.len() >= 13);
}

// ===========================================================================
// Module 3: Tool definition conversion
// ===========================================================================

#[test]
fn tool_def_to_claude_maps_fields() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let claude = dialect::tool_def_to_claude(&canonical);
    assert_eq!(claude.name, "read_file");
    assert_eq!(claude.description, "Read a file");
    assert_eq!(claude.input_schema, json!({"type": "object"}));
}

#[test]
fn tool_def_from_claude_maps_fields() {
    let claude = ClaudeToolDef {
        name: "bash".into(),
        description: "Run bash".into(),
        input_schema: json!({"type": "object", "properties": {}}),
    };
    let canonical = dialect::tool_def_from_claude(&claude);
    assert_eq!(canonical.name, "bash");
    assert_eq!(canonical.parameters_schema, claude.input_schema);
}

#[test]
fn tool_def_roundtrip_canonical_claude_canonical() {
    let original = CanonicalToolDef {
        name: "grep".into(),
        description: "Search files".into(),
        parameters_schema: json!({"type": "object", "properties": {"pattern": {"type": "string"}}}),
    };
    let claude = dialect::tool_def_to_claude(&original);
    let back = dialect::tool_def_from_claude(&claude);
    assert_eq!(back, original);
}

#[test]
fn canonical_tool_def_serde_roundtrip() {
    let def = CanonicalToolDef {
        name: "write_file".into(),
        description: "Write a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: CanonicalToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

#[test]
fn claude_tool_def_serde_roundtrip() {
    let def = ClaudeToolDef {
        name: "bash".into(),
        description: "Execute command".into(),
        input_schema: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: ClaudeToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

// ===========================================================================
// Module 4: ThinkingConfig
// ===========================================================================

#[test]
fn thinking_config_new_sets_type() {
    let tc = ThinkingConfig::new(10000);
    assert_eq!(tc.thinking_type, "enabled");
    assert_eq!(tc.budget_tokens, 10000);
}

#[test]
fn thinking_config_serde_roundtrip() {
    let tc = ThinkingConfig::new(5000);
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn thinking_config_json_has_type_field() {
    let tc = ThinkingConfig::new(8000);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "enabled");
    assert_eq!(v["budget_tokens"], 8000);
}

// ===========================================================================
// Module 5: ClaudeConfig
// ===========================================================================

#[test]
fn claude_config_default_values() {
    let cfg = ClaudeConfig::default();
    assert!(cfg.api_key.is_empty());
    assert!(cfg.base_url.contains("anthropic.com"));
    assert_eq!(cfg.model, "claude-sonnet-4-20250514");
    assert_eq!(cfg.max_tokens, 4096);
    assert!(cfg.system_prompt.is_none());
    assert!(cfg.thinking.is_none());
}

#[test]
fn claude_config_serde_roundtrip() {
    let cfg = ClaudeConfig {
        api_key: "sk-ant-test".into(),
        base_url: "https://api.anthropic.com/v1".into(),
        model: "claude-opus-4-20250514".into(),
        max_tokens: 8192,
        system_prompt: Some("Be precise.".into()),
        thinking: Some(ThinkingConfig::new(4096)),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.api_key, cfg.api_key);
    assert_eq!(parsed.model, cfg.model);
}

#[test]
fn claude_config_omits_none_thinking() {
    let cfg = ClaudeConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("thinking"));
}

// ===========================================================================
// Module 6: ClaudeContentBlock variants serde
// ===========================================================================

#[test]
fn content_block_text_serde_roundtrip() {
    let block = ClaudeContentBlock::Text {
        text: "Hello world".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_text_json_has_type_text() {
    let block = ClaudeContentBlock::Text {
        text: "test".into(),
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "text");
}

#[test]
fn content_block_tool_use_serde_roundtrip() {
    let block = ClaudeContentBlock::ToolUse {
        id: "tu_123".into(),
        name: "read_file".into(),
        input: json!({"path": "main.rs"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_tool_result_serde_roundtrip() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_456".into(),
        content: Some("file data".into()),
        is_error: Some(false),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_tool_result_no_content_no_error() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_789".into(),
        content: None,
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    // None fields should be omitted
    assert!(!json.contains("\"content\""));
    assert!(!json.contains("\"is_error\""));
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_thinking_serde_roundtrip() {
    let block = ClaudeContentBlock::Thinking {
        thinking: "Let me consider...".into(),
        signature: Some("sig_abc".into()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_thinking_no_signature() {
    let block = ClaudeContentBlock::Thinking {
        thinking: "Reasoning here".into(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("\"signature\""));
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_image_base64_serde_roundtrip() {
    let block = ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "aGVsbG8=".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_image_url_serde_roundtrip() {
    let block = ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

// ===========================================================================
// Module 7: ClaudeImageSource
// ===========================================================================

#[test]
fn image_source_base64_has_type_field() {
    let src = ClaudeImageSource::Base64 {
        media_type: "image/jpeg".into(),
        data: "data".into(),
    };
    let v = serde_json::to_value(&src).unwrap();
    assert_eq!(v["type"], "base64");
}

#[test]
fn image_source_url_has_type_field() {
    let src = ClaudeImageSource::Url {
        url: "https://img.test/a.png".into(),
    };
    let v = serde_json::to_value(&src).unwrap();
    assert_eq!(v["type"], "url");
}

// ===========================================================================
// Module 8: ClaudeSystemBlock and ClaudeCacheControl
// ===========================================================================

#[test]
fn cache_control_ephemeral() {
    let cc = ClaudeCacheControl::ephemeral();
    assert_eq!(cc.cache_type, "ephemeral");
}

#[test]
fn cache_control_serde_roundtrip() {
    let cc = ClaudeCacheControl::ephemeral();
    let json = serde_json::to_string(&cc).unwrap();
    let parsed: ClaudeCacheControl = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, cc);
}

#[test]
fn cache_control_json_has_type_field() {
    let cc = ClaudeCacheControl::ephemeral();
    let v = serde_json::to_value(&cc).unwrap();
    assert_eq!(v["type"], "ephemeral");
}

#[test]
fn system_block_text_serde_roundtrip() {
    let block = ClaudeSystemBlock::Text {
        text: "System prompt".into(),
        cache_control: Some(ClaudeCacheControl::ephemeral()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn system_block_text_no_cache_control() {
    let block = ClaudeSystemBlock::Text {
        text: "Prompt".into(),
        cache_control: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("cache_control"));
    let parsed: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

// ===========================================================================
// Module 9: ClaudeUsage
// ===========================================================================

#[test]
fn claude_usage_serde_roundtrip_full() {
    let usage = ClaudeUsage {
        input_tokens: 500,
        output_tokens: 250,
        cache_creation_input_tokens: Some(100),
        cache_read_input_tokens: Some(50),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn claude_usage_serde_minimal() {
    let usage = ClaudeUsage {
        input_tokens: 10,
        output_tokens: 20,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(!json.contains("cache_creation"));
    assert!(!json.contains("cache_read"));
    let parsed: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn claude_usage_zero_tokens() {
    let usage = ClaudeUsage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.input_tokens, 0);
    assert_eq!(parsed.output_tokens, 0);
}

// ===========================================================================
// Module 10: ClaudeStopReason
// ===========================================================================

#[test]
fn parse_stop_reason_end_turn() {
    assert_eq!(
        dialect::parse_stop_reason("end_turn"),
        Some(ClaudeStopReason::EndTurn)
    );
}

#[test]
fn parse_stop_reason_tool_use() {
    assert_eq!(
        dialect::parse_stop_reason("tool_use"),
        Some(ClaudeStopReason::ToolUse)
    );
}

#[test]
fn parse_stop_reason_max_tokens() {
    assert_eq!(
        dialect::parse_stop_reason("max_tokens"),
        Some(ClaudeStopReason::MaxTokens)
    );
}

#[test]
fn parse_stop_reason_stop_sequence() {
    assert_eq!(
        dialect::parse_stop_reason("stop_sequence"),
        Some(ClaudeStopReason::StopSequence)
    );
}

#[test]
fn parse_stop_reason_unknown_returns_none() {
    assert_eq!(dialect::parse_stop_reason("unknown"), None);
    assert_eq!(dialect::parse_stop_reason(""), None);
}

#[test]
fn map_stop_reason_end_turn() {
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::EndTurn),
        "end_turn"
    );
}

#[test]
fn map_stop_reason_tool_use() {
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::ToolUse),
        "tool_use"
    );
}

#[test]
fn map_stop_reason_max_tokens() {
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::MaxTokens),
        "max_tokens"
    );
}

#[test]
fn map_stop_reason_stop_sequence() {
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::StopSequence),
        "stop_sequence"
    );
}

#[test]
fn stop_reason_serde_roundtrip_all_variants() {
    for reason in &[
        ClaudeStopReason::EndTurn,
        ClaudeStopReason::ToolUse,
        ClaudeStopReason::MaxTokens,
        ClaudeStopReason::StopSequence,
    ] {
        let json = serde_json::to_string(reason).unwrap();
        let parsed: ClaudeStopReason = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, reason);
    }
}

#[test]
fn stop_reason_parse_and_map_roundtrip() {
    for s in &["end_turn", "tool_use", "max_tokens", "stop_sequence"] {
        let parsed = dialect::parse_stop_reason(s).unwrap();
        let back = dialect::map_stop_reason(parsed);
        assert_eq!(back, *s);
    }
}

// ===========================================================================
// Module 11: ClaudeRequest and ClaudeMessage
// ===========================================================================

#[test]
fn claude_request_serde_roundtrip() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system: Some("Be helpful".into()),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }],
        thinking: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, req.model);
    assert_eq!(parsed.max_tokens, req.max_tokens);
    assert_eq!(parsed.messages.len(), 1);
}

#[test]
fn claude_request_with_thinking() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system: None,
        messages: vec![],
        thinking: Some(ThinkingConfig::new(10000)),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("thinking"));
    let parsed: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.thinking.unwrap().budget_tokens, 10000);
}

#[test]
fn claude_message_serde_roundtrip() {
    let msg = ClaudeMessage {
        role: "assistant".into(),
        content: "Sure!".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: ClaudeMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, "assistant");
    assert_eq!(parsed.content, "Sure!");
}

// ===========================================================================
// Module 12: ClaudeResponse
// ===========================================================================

#[test]
fn claude_response_serde_roundtrip() {
    let resp = ClaudeResponse {
        id: "msg_test".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text {
            text: "Hello".into(),
        }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn claude_response_empty_content() {
    let resp = ClaudeResponse {
        id: "msg_empty".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![],
        stop_reason: None,
        usage: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.len(), 0);
}

// ===========================================================================
// Module 13: ClaudeApiError
// ===========================================================================

#[test]
fn claude_api_error_serde_roundtrip() {
    let err = ClaudeApiError {
        error_type: "invalid_request_error".into(),
        message: "Model not found".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let parsed: ClaudeApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, err);
}

#[test]
fn claude_api_error_json_uses_type_field() {
    let err = ClaudeApiError {
        error_type: "overloaded_error".into(),
        message: "Too many requests".into(),
    };
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["type"], "overloaded_error");
}

// ===========================================================================
// Module 14: Streaming events serde
// ===========================================================================

#[test]
fn stream_event_message_start_serde() {
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
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("message_start"));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_start_serde() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("content_block_start"));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_delta_text_serde() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "Hello".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_delta_input_json_serde() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"path"#.into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_delta_thinking_serde() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "Hmm...".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_delta_signature_serde() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::SignatureDelta {
            signature: "partial_sig".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_content_block_stop_serde() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 2 };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("content_block_stop"));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_message_delta_serde() {
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

#[test]
fn stream_event_message_delta_no_usage() {
    let event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: None,
            stop_sequence: Some("###".into()),
        },
        usage: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_message_stop_serde() {
    let event = ClaudeStreamEvent::MessageStop {};
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("message_stop"));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_ping_serde() {
    let event = ClaudeStreamEvent::Ping {};
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("ping"));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

#[test]
fn stream_event_error_serde() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "rate_limit_error".into(),
            message: "Too many requests".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("error"));
    let parsed: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);
}

// ===========================================================================
// Module 15: ClaudeStreamDelta variants
// ===========================================================================

#[test]
fn stream_delta_text_delta_serde() {
    let delta = ClaudeStreamDelta::TextDelta {
        text: "fragment".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "text_delta");
}

#[test]
fn stream_delta_input_json_delta_serde() {
    let delta = ClaudeStreamDelta::InputJsonDelta {
        partial_json: r#"{"ke"#.into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "input_json_delta");
}

#[test]
fn stream_delta_thinking_delta_serde() {
    let delta = ClaudeStreamDelta::ThinkingDelta {
        thinking: "reasoning".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "thinking_delta");
}

#[test]
fn stream_delta_signature_delta_serde() {
    let delta = ClaudeStreamDelta::SignatureDelta {
        signature: "sig_part".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "signature_delta");
}

// ===========================================================================
// Module 16: ClaudeMessageDelta
// ===========================================================================

#[test]
fn message_delta_serde_roundtrip() {
    let delta = ClaudeMessageDelta {
        stop_reason: Some("end_turn".into()),
        stop_sequence: Some("###".into()),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let parsed: ClaudeMessageDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, delta);
}

#[test]
fn message_delta_all_none() {
    let delta = ClaudeMessageDelta {
        stop_reason: None,
        stop_sequence: None,
    };
    let json = serde_json::to_string(&delta).unwrap();
    assert!(!json.contains("stop_reason"));
    assert!(!json.contains("stop_sequence"));
}

// ===========================================================================
// Module 17: map_work_order
// ===========================================================================

#[test]
fn map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Implement feature X").build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(req.messages[0].content.contains("Implement feature X"));
    assert_eq!(req.model, cfg.model);
    assert_eq!(req.max_tokens, cfg.max_tokens);
}

#[test]
fn map_work_order_uses_model_override() {
    let wo = WorkOrderBuilder::new("task")
        .model("claude-opus-4-20250514")
        .build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "claude-opus-4-20250514");
}

#[test]
fn map_work_order_with_system_prompt() {
    let mut cfg = ClaudeConfig::default();
    cfg.system_prompt = Some("Be concise.".into());
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.system.as_deref(), Some("Be concise."));
}

#[test]
fn map_work_order_with_thinking() {
    let mut cfg = ClaudeConfig::default();
    cfg.thinking = Some(ThinkingConfig::new(8000));
    let wo = WorkOrderBuilder::new("task").build();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.thinking.is_some());
    assert_eq!(req.thinking.unwrap().budget_tokens, 8000);
}

// ===========================================================================
// Module 18: map_response
// ===========================================================================

fn make_response(content: Vec<ClaudeContentBlock>) -> ClaudeResponse {
    ClaudeResponse {
        id: "msg_test".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content,
        stop_reason: Some("end_turn".into()),
        usage: None,
    }
}

#[test]
fn map_response_text_block() {
    let resp = make_response(vec![ClaudeContentBlock::Text {
        text: "Answer".into(),
    }]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "Answer"
    ));
}

#[test]
fn map_response_tool_use_block() {
    let resp = make_response(vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: json!({"cmd": "ls"}),
    }]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::ToolCall { tool_name, .. } if tool_name == "bash"
    ));
}

#[test]
fn map_response_tool_result_block() {
    let resp = make_response(vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("output".into()),
        is_error: Some(false),
    }]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolResult { .. }));
}

#[test]
fn map_response_tool_result_error() {
    let resp = make_response(vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("fail".into()),
        is_error: Some(true),
    }]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolResult { is_error, .. } => assert!(*is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_response_tool_result_none_content() {
    let resp = make_response(vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_nil".into(),
        content: None,
        is_error: None,
    }]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            output, is_error, ..
        } => {
            assert!(!is_error);
            assert_eq!(*output, serde_json::Value::String(String::new()));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_response_thinking_block() {
    let resp = make_response(vec![ClaudeContentBlock::Thinking {
        thinking: "Let me think".into(),
        signature: Some("sig".into()),
    }]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Let me think"),
        other => panic!("expected AssistantMessage for thinking, got {other:?}"),
    }
    let ext = events[0].ext.as_ref().unwrap();
    assert_eq!(ext["thinking"], serde_json::Value::Bool(true));
    assert_eq!(ext["signature"], json!("sig"));
}

#[test]
fn map_response_thinking_without_signature() {
    let resp = make_response(vec![ClaudeContentBlock::Thinking {
        thinking: "Hmm".into(),
        signature: None,
    }]);
    let events = dialect::map_response(&resp);
    let ext = events[0].ext.as_ref().unwrap();
    assert!(!ext.contains_key("signature"));
}

#[test]
fn map_response_image_block_produces_no_events() {
    let resp = make_response(vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    }]);
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_empty_content() {
    let resp = make_response(vec![]);
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_multiple_blocks() {
    let resp = make_response(vec![
        ClaudeContentBlock::Text {
            text: "I'll help.".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read".into(),
            input: json!({}),
        },
    ]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

// ===========================================================================
// Module 19: map_stream_event
// ===========================================================================

#[test]
fn map_stream_event_text_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "word".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "word"
    ));
}

#[test]
fn map_stream_event_thinking_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "reason".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(events[0].ext.as_ref().unwrap().contains_key("thinking"));
}

#[test]
fn map_stream_event_message_start() {
    let event = ClaudeStreamEvent::MessageStart {
        message: ClaudeResponse {
            id: "msg_x".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            usage: None,
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn map_stream_event_message_stop() {
    let event = ClaudeStreamEvent::MessageStop {};
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn map_stream_event_error() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "api_error".into(),
            message: "Something went wrong".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert!(message.contains("api_error"));
            assert!(message.contains("Something went wrong"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn map_stream_event_ping_produces_empty() {
    let event = ClaudeStreamEvent::Ping {};
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn map_stream_event_content_block_stop_produces_empty() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 0 };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn map_stream_event_input_json_delta_produces_empty() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: "partial".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn map_stream_event_tool_use_content_block_start() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 1,
        content_block: ClaudeContentBlock::ToolUse {
            id: "tu_s".into(),
            name: "bash".into(),
            input: json!({}),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn map_stream_event_text_content_block_start_produces_empty() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

// ===========================================================================
// Module 20: map_tool_result
// ===========================================================================

#[test]
fn map_tool_result_success() {
    let msg = dialect::map_tool_result("tu_1", "output data", false);
    assert_eq!(msg.role, "user");
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    assert_eq!(blocks.len(), 1);
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
    let msg = dialect::map_tool_result("tu_err", "not found", true);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(*is_error, Some(true));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_tool_result_empty_output() {
    let msg = dialect::map_tool_result("tu_empty", "", false);
    let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &blocks[0] {
        ClaudeContentBlock::ToolResult { content, .. } => {
            assert_eq!(content.as_deref(), Some(""));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ===========================================================================
// Module 21: Passthrough fidelity
// ===========================================================================

#[test]
fn passthrough_event_roundtrip_text_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "hello".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext["dialect"], json!("claude"));
    assert!(ext.contains_key("raw_message"));
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn passthrough_event_roundtrip_message_stop() {
    let event = ClaudeStreamEvent::MessageStop {};
    let wrapped = dialect::to_passthrough_event(&event);
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn passthrough_event_roundtrip_error() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "err".into(),
            message: "bad".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn from_passthrough_event_returns_none_without_ext() {
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "test".into(),
        },
        ext: None,
    };
    assert!(dialect::from_passthrough_event(&event).is_none());
}

#[test]
fn from_passthrough_event_returns_none_without_raw_message() {
    let mut ext = BTreeMap::new();
    ext.insert("dialect".into(), json!("claude"));
    let event = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: "test".into(),
        },
        ext: Some(ext),
    };
    assert!(dialect::from_passthrough_event(&event).is_none());
}

#[test]
fn verify_passthrough_fidelity_all_event_types() {
    let events = vec![
        ClaudeStreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_1".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        },
        ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Text {
                text: String::new(),
            },
        },
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta { text: "hi".into() },
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
        ClaudeStreamEvent::Ping {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn verify_passthrough_fidelity_empty_events() {
    assert!(dialect::verify_passthrough_fidelity(&[]));
}

// ===========================================================================
// Module 22: messages module - Role
// ===========================================================================

#[test]
fn role_user_serde() {
    let json = serde_json::to_string(&Role::User).unwrap();
    assert_eq!(json, "\"user\"");
    let parsed: Role = serde_json::from_str("\"user\"").unwrap();
    assert_eq!(parsed, Role::User);
}

#[test]
fn role_assistant_serde() {
    let json = serde_json::to_string(&Role::Assistant).unwrap();
    assert_eq!(json, "\"assistant\"");
    let parsed: Role = serde_json::from_str("\"assistant\"").unwrap();
    assert_eq!(parsed, Role::Assistant);
}

// ===========================================================================
// Module 23: messages module - MessageContent
// ===========================================================================

#[test]
fn message_content_text_serde() {
    let content = MessageContent::Text("Hello".into());
    let v = serde_json::to_value(&content).unwrap();
    assert_eq!(v, json!("Hello"));
}

#[test]
fn message_content_blocks_serde() {
    let content = MessageContent::Blocks(vec![ContentBlock::Text { text: "Hi".into() }]);
    let json = serde_json::to_string(&content).unwrap();
    let parsed: MessageContent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, content);
}

#[test]
fn message_content_empty_blocks() {
    let content = MessageContent::Blocks(vec![]);
    let json = serde_json::to_string(&content).unwrap();
    assert_eq!(json, "[]");
}

// ===========================================================================
// Module 24: messages module - SystemMessage
// ===========================================================================

#[test]
fn system_message_text_serde() {
    let sys = SystemMessage::Text("Be helpful".into());
    let v = serde_json::to_value(&sys).unwrap();
    assert_eq!(v, json!("Be helpful"));
}

#[test]
fn system_message_blocks_serde() {
    let sys = SystemMessage::Blocks(vec![SystemBlock::Text {
        text: "Prompt".into(),
        cache_control: Some(CacheControl::ephemeral()),
    }]);
    let json = serde_json::to_string(&sys).unwrap();
    let parsed: SystemMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, sys);
}

// ===========================================================================
// Module 25: messages module - Metadata
// ===========================================================================

#[test]
fn metadata_with_user_id() {
    let meta = Metadata {
        user_id: Some("u_123".into()),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let parsed: Metadata = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, meta);
}

#[test]
fn metadata_no_user_id() {
    let meta = Metadata { user_id: None };
    let json = serde_json::to_string(&meta).unwrap();
    assert!(!json.contains("user_id"));
}

// ===========================================================================
// Module 26: messages module - MessagesRequest
// ===========================================================================

#[test]
fn messages_request_minimal() {
    let req = MessagesRequest {
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
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn messages_request_all_optional_fields() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![],
        max_tokens: 8192,
        system: Some(SystemMessage::Text("Be nice".into())),
        tools: Some(vec![Tool {
            name: "test".into(),
            description: "Test tool".into(),
            input_schema: json!({"type": "object"}),
        }]),
        metadata: Some(Metadata {
            user_id: Some("user_1".into()),
        }),
        stream: Some(true),
        stop_sequences: Some(vec!["STOP".into()]),
        temperature: Some(0.5),
        top_p: Some(0.95),
        top_k: Some(50),
        tool_choice: None,
        thinking: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: MessagesRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn messages_request_omits_none_fields() {
    let req = MessagesRequest {
        model: "m".into(),
        messages: vec![],
        max_tokens: 100,
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
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("\"system\""));
    assert!(!json.contains("\"tools\""));
    assert!(!json.contains("\"stream\""));
    assert!(!json.contains("\"temperature\""));
    assert!(!json.contains("\"top_p\""));
    assert!(!json.contains("\"top_k\""));
}

// ===========================================================================
// Module 27: messages module - MessagesResponse
// ===========================================================================

#[test]
fn messages_response_serde_roundtrip() {
    let resp = MessagesResponse {
        id: "msg_abc".into(),
        response_type: "message".into(),
        role: "assistant".into(),
        content: vec![ContentBlock::Text { text: "Hi".into() }],
        model: "claude-sonnet-4-20250514".into(),
        stop_reason: Some("end_turn".into()),
        stop_sequence: None,
        usage: Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: MessagesResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn messages_response_type_field_is_renamed() {
    let resp = MessagesResponse {
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

// ===========================================================================
// Module 28: From<MessagesRequest> for WorkOrder
// ===========================================================================

#[test]
fn messages_request_to_work_order_basic_conversion() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text("Fix bug".into()),
        }],
        max_tokens: 4096,
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
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.task, "Fix bug");
    assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
}

#[test]
fn messages_request_to_work_order_extracts_system() {
    let req = MessagesRequest {
        model: "m".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text("task".into()),
        }],
        max_tokens: 100,
        system: Some(SystemMessage::Text("Be precise".into())),
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
    let wo: abp_core::WorkOrder = req.into();
    assert_eq!(wo.config.vendor["system"], json!("Be precise"));
}

#[test]
fn messages_request_to_work_order_with_tools() {
    let req = MessagesRequest {
        model: "m".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text("task".into()),
        }],
        max_tokens: 100,
        system: None,
        tools: Some(vec![Tool {
            name: "bash".into(),
            description: "Run commands".into(),
            input_schema: json!({"type": "object"}),
        }]),
        metadata: None,
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        tool_choice: None,
        thinking: None,
    };
    let wo: abp_core::WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("tools"));
}

#[test]
fn messages_request_to_work_order_with_metadata() {
    let req = MessagesRequest {
        model: "m".into(),
        messages: vec![Message {
            role: Role::User,
            content: MessageContent::Text("task".into()),
        }],
        max_tokens: 100,
        system: None,
        tools: None,
        metadata: Some(Metadata {
            user_id: Some("u_1".into()),
        }),
        stream: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        tool_choice: None,
        thinking: None,
    };
    let wo: abp_core::WorkOrder = req.into();
    assert!(wo.config.vendor.contains_key("metadata"));
}

// ===========================================================================
// Module 29: From<Receipt> for MessagesResponse
// ===========================================================================

#[test]
fn receipt_to_response_complete() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(100),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        })
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Done!".into(),
            },
            ext: None,
        })
        .build();
    let resp: MessagesResponse = receipt.into();
    assert!(resp.id.starts_with("msg_"));
    assert_eq!(resp.response_type, "message");
    assert_eq!(resp.role, "assistant");
    assert_eq!(resp.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(resp.usage.input_tokens, 200);
    assert_eq!(resp.usage.output_tokens, 100);
}

#[test]
fn receipt_to_response_partial() {
    let receipt = ReceiptBuilder::new("backend")
        .outcome(Outcome::Partial)
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("max_tokens"));
}

#[test]
fn receipt_to_response_failed() {
    let receipt = ReceiptBuilder::new("backend")
        .outcome(Outcome::Failed)
        .build();
    let resp: MessagesResponse = receipt.into();
    assert!(resp.stop_reason.is_none());
}

#[test]
fn receipt_to_response_with_tool_use() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "x.rs"}),
            },
            ext: None,
        })
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.stop_reason.as_deref(), Some("tool_use"));
    assert!(matches!(
        &resp.content[0],
        ContentBlock::ToolUse { name, .. } if name == "read_file"
    ));
}

#[test]
fn receipt_to_response_thinking_block() {
    let mut ext = BTreeMap::new();
    ext.insert("thinking".into(), json!(true));
    ext.insert("signature".into(), json!("sig_x"));
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "thinking...".into(),
            },
            ext: Some(ext),
        })
        .build();
    let resp: MessagesResponse = receipt.into();
    assert!(matches!(
        &resp.content[0],
        ContentBlock::Thinking { thinking, signature }
        if thinking == "thinking..." && signature.as_deref() == Some("sig_x")
    ));
}

#[test]
fn receipt_to_response_extracts_model_from_usage_raw() {
    let receipt = ReceiptBuilder::new("sidecar:claude")
        .outcome(Outcome::Complete)
        .usage_raw(json!({"model": "claude-opus-4-20250514"}))
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.model, "claude-opus-4-20250514");
}

#[test]
fn receipt_to_response_cache_tokens() {
    let receipt = ReceiptBuilder::new("claude-sonnet-4-20250514")
        .outcome(Outcome::Complete)
        .usage(UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(250),
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            request_units: None,
            estimated_cost_usd: None,
        })
        .build();
    let resp: MessagesResponse = receipt.into();
    assert_eq!(resp.usage.cache_read_input_tokens, Some(100));
    assert_eq!(resp.usage.cache_creation_input_tokens, Some(50));
}

// ===========================================================================
// Module 30: lowering module
// ===========================================================================

#[test]
fn lowering_to_ir_user_text() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello".into(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

#[test]
fn lowering_to_ir_assistant_text() {
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: "Sure!".into(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn lowering_to_ir_with_system_prompt() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hi".into(),
    }];
    let conv = lowering::to_ir(&msgs, Some("System"));
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "System");
}

#[test]
fn lowering_to_ir_empty_system_prompt_skipped() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hi".into(),
    }];
    let conv = lowering::to_ir(&msgs, Some(""));
    assert_eq!(conv.messages.len(), 1);
}

#[test]
fn lowering_from_ir_skips_system() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn lowering_extract_system_prompt() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let sys = lowering::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("instructions"));
}

#[test]
fn lowering_extract_system_prompt_none() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    assert!(lowering::extract_system_prompt(&conv).is_none());
}

#[test]
fn lowering_roundtrip_text_messages() {
    let msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "Hi!".into(),
        },
    ];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello");
    assert_eq!(back[1].role, "assistant");
    assert_eq!(back[1].content, "Hi!");
}

#[test]
fn lowering_empty_messages() {
    let conv = lowering::to_ir(&[], None);
    assert!(conv.is_empty());
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn lowering_tool_use_to_ir_and_back() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "bash".into(),
        input: json!({"command": "ls"}),
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::ToolUse { name, .. } if name == "bash"
    ));
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    assert!(matches!(
        &parsed[0],
        ClaudeContentBlock::ToolUse { name, .. } if name == "bash"
    ));
}

#[test]
fn lowering_tool_result_to_ir_and_back() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("data".into()),
        is_error: None,
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tu_1"
    ));
}

#[test]
fn lowering_thinking_block_to_ir() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "reasoning".into(),
        signature: Some("sig".into()),
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Thinking { text } if text == "reasoning"
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
fn lowering_image_url_to_ir_as_text() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/x.png".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Text { text } => {
            assert!(text.contains("https://example.com/x.png"));
        }
        other => panic!("expected Text for URL image, got {other:?}"),
    }
}

// ===========================================================================
// Module 31: Re-export aliases in messages module
// ===========================================================================

#[test]
fn re_export_content_block_is_claude_content_block() {
    let block: ContentBlock = ContentBlock::Text {
        text: "test".into(),
    };
    let as_claude: ClaudeContentBlock = block;
    assert!(matches!(as_claude, ClaudeContentBlock::Text { .. }));
}

#[test]
fn re_export_usage_is_claude_usage() {
    let u: Usage = Usage {
        input_tokens: 1,
        output_tokens: 2,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let as_claude: ClaudeUsage = u;
    assert_eq!(as_claude.input_tokens, 1);
}

// ===========================================================================
// Module 32: lib.rs constants
// ===========================================================================

#[test]
fn backend_name_constant() {
    assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
}

#[test]
fn host_script_relative_constant() {
    assert_eq!(abp_claude_sdk::HOST_SCRIPT_RELATIVE, "hosts/claude/host.js");
}

#[test]
fn default_node_command_constant() {
    assert_eq!(abp_claude_sdk::DEFAULT_NODE_COMMAND, "node");
}

#[test]
fn sidecar_script_resolves_path() {
    let root = std::path::Path::new("/fake/root");
    let script = abp_claude_sdk::sidecar_script(root);
    assert_eq!(script, root.join("hosts/claude/host.js"));
}

// ===========================================================================
// Module 33: Edge cases and special scenarios
// ===========================================================================

#[test]
fn content_block_text_empty_string() {
    let block = ClaudeContentBlock::Text {
        text: String::new(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_tool_use_empty_input() {
    let block = ClaudeContentBlock::ToolUse {
        id: "tu_x".into(),
        name: "no_args".into(),
        input: json!({}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn content_block_tool_use_complex_input() {
    let block = ClaudeContentBlock::ToolUse {
        id: "tu_complex".into(),
        name: "complex".into(),
        input: json!({
            "nested": {"key": "value"},
            "array": [1, 2, 3],
            "null_val": null,
            "bool_val": true
        }),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn claude_message_empty_content() {
    let msg = ClaudeMessage {
        role: "user".into(),
        content: String::new(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: ClaudeMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content, "");
}

#[test]
fn claude_response_multiple_content_blocks() {
    let resp = ClaudeResponse {
        id: "msg_multi".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Thinking {
                thinking: "reasoning".into(),
                signature: Some("sig".into()),
            },
            ClaudeContentBlock::Text {
                text: "Answer".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "bash".into(),
                input: json!({"cmd": "ls"}),
            },
        ],
        stop_reason: Some("tool_use".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_creation_input_tokens: Some(200),
            cache_read_input_tokens: Some(100),
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.len(), 3);
    assert_eq!(parsed, resp);
}

#[test]
fn map_work_order_with_snippets() {
    let ctx = abp_core::ContextPacket {
        files: vec![],
        snippets: vec![abp_core::ContextSnippet {
            name: "readme".into(),
            content: "# Project".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Summarize docs").context(ctx).build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.messages[0].content.contains("readme"));
    assert!(req.messages[0].content.contains("# Project"));
}

#[test]
fn lowering_multi_turn_conversation() {
    let msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "Hi!".into(),
        },
        ClaudeMessage {
            role: "user".into(),
            content: "Thanks".into(),
        },
    ];
    let conv = lowering::to_ir(&msgs, Some("Be friendly"));
    assert_eq!(conv.messages.len(), 4);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[3].role, IrRole::User);
}

#[test]
fn lowering_tool_result_no_content_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_nil".into(),
        content: None,
        is_error: None,
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult { content, .. } => assert!(content.is_none()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn lowering_tool_result_error_flag_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("error msg".into()),
        is_error: Some(true),
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => assert_eq!(*is_error, Some(true)),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn unicode_content_roundtrip() {
    let block = ClaudeContentBlock::Text {
        text: "こんにちは世界 🌍 émojis ñ".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn special_chars_in_tool_name() {
    let block = ClaudeContentBlock::ToolUse {
        id: "tu_sp".into(),
        name: "my-tool_v2.0".into(),
        input: json!({}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let parsed: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, block);
}

#[test]
fn large_token_values() {
    let usage = ClaudeUsage {
        input_tokens: u64::MAX,
        output_tokens: u64::MAX,
        cache_creation_input_tokens: Some(u64::MAX),
        cache_read_input_tokens: Some(u64::MAX),
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}
