#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive integration tests for all public types in `abp-sdk-types`.

use std::collections::BTreeMap;

use abp_sdk_types::claude::*;
use abp_sdk_types::codex::*;
use abp_sdk_types::common::*;
use abp_sdk_types::convert::*;
use abp_sdk_types::copilot::*;
use abp_sdk_types::gemini::*;
use abp_sdk_types::kimi::*;
use abp_sdk_types::openai::*;
use abp_sdk_types::*;

// ── Dialect enum ────────────────────────────────────────────────────────

#[test]
fn dialect_all_returns_six_variants() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
    assert!(all.contains(&Dialect::OpenAi));
    assert!(all.contains(&Dialect::Claude));
    assert!(all.contains(&Dialect::Gemini));
    assert!(all.contains(&Dialect::Kimi));
    assert!(all.contains(&Dialect::Codex));
    assert!(all.contains(&Dialect::Copilot));
}

#[test]
fn dialect_label_matches_display() {
    for d in Dialect::all() {
        assert_eq!(d.label(), d.to_string());
    }
}

#[test]
fn dialect_serde_all_variants() {
    let expected = [
        (Dialect::OpenAi, "\"open_ai\""),
        (Dialect::Claude, "\"claude\""),
        (Dialect::Gemini, "\"gemini\""),
        (Dialect::Kimi, "\"kimi\""),
        (Dialect::Codex, "\"codex\""),
        (Dialect::Copilot, "\"copilot\""),
    ];
    for (variant, json_str) in expected {
        let serialized = serde_json::to_string(&variant).unwrap();
        assert_eq!(serialized, json_str);
        let back: Dialect = serde_json::from_str(&serialized).unwrap();
        assert_eq!(variant, back);
    }
}

#[test]
fn dialect_clone_copy_eq_hash() {
    let d = Dialect::OpenAi;
    let d2 = d;
    let d3 = d.clone();
    assert_eq!(d, d2);
    assert_eq!(d, d3);
    let mut set = std::collections::HashSet::new();
    set.insert(d);
    assert!(set.contains(&d2));
}

#[test]
fn dialect_debug_format() {
    let dbg = format!("{:?}", Dialect::Claude);
    assert_eq!(dbg, "Claude");
}

// ── ModelConfig ─────────────────────────────────────────────────────────

#[test]
fn model_config_default_is_empty() {
    let cfg = ModelConfig::default();
    assert_eq!(cfg.model, "");
    assert!(cfg.max_tokens.is_none());
    assert!(cfg.temperature.is_none());
    assert!(cfg.top_p.is_none());
    assert!(cfg.stop_sequences.is_none());
    assert!(cfg.extra.is_empty());
}

#[test]
fn model_config_full_roundtrip() {
    let mut extra = BTreeMap::new();
    extra.insert("key".into(), serde_json::json!("value"));
    let cfg = ModelConfig {
        model: "gpt-4o".into(),
        max_tokens: Some(8192),
        temperature: Some(0.9),
        top_p: Some(0.95),
        stop_sequences: Some(vec!["STOP".into(), "END".into()]),
        extra,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ModelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn model_config_skip_serializing_none_fields() {
    let cfg = ModelConfig {
        model: "m".into(),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("top_p"));
    assert!(!json.contains("stop_sequences"));
    assert!(!json.contains("extra"));
}

#[test]
fn model_config_unicode_model_name() {
    let cfg = ModelConfig {
        model: "模型-ñ-🤖".into(),
        ..Default::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ModelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "模型-ñ-🤖");
}

// ── CanonicalToolDef ────────────────────────────────────────────────────

#[test]
fn canonical_tool_def_roundtrip() {
    let def = CanonicalToolDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters_schema: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let back: CanonicalToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(def, back);
}

#[test]
fn canonical_tool_def_empty_name() {
    let def = CanonicalToolDef {
        name: "".into(),
        description: "".into(),
        parameters_schema: serde_json::json!({}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let back: CanonicalToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "");
}

#[test]
fn canonical_tool_def_clone_debug() {
    let def = CanonicalToolDef {
        name: "tool".into(),
        description: "desc".into(),
        parameters_schema: serde_json::json!(null),
    };
    let cloned = def.clone();
    assert_eq!(def, cloned);
    let dbg = format!("{:?}", def);
    assert!(dbg.contains("tool"));
}

// ── DialectRequest ──────────────────────────────────────────────────────

#[test]
fn dialect_request_dialect_and_model_for_all_variants() {
    let openai_req = DialectRequest::OpenAi(OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
        stream: None,
    });
    assert_eq!(openai_req.dialect(), Dialect::OpenAi);
    assert_eq!(openai_req.model(), "gpt-4o");

    let claude_req = DialectRequest::Claude(ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 1024,
        system: None,
        messages: vec![],
        tools: None,
        thinking: None,
        stream: None,
    });
    assert_eq!(claude_req.dialect(), Dialect::Claude);
    assert_eq!(claude_req.model(), "claude-sonnet-4-20250514");

    let gemini_req = DialectRequest::Gemini(GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    });
    assert_eq!(gemini_req.dialect(), Dialect::Gemini);
    assert_eq!(gemini_req.model(), "gemini-2.5-flash");

    let kimi_req = DialectRequest::Kimi(KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    });
    assert_eq!(kimi_req.dialect(), Dialect::Kimi);
    assert_eq!(kimi_req.model(), "moonshot-v1-8k");

    let codex_req = DialectRequest::Codex(CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: None,
    });
    assert_eq!(codex_req.dialect(), Dialect::Codex);
    assert_eq!(codex_req.model(), "codex-mini-latest");

    let copilot_req = DialectRequest::Copilot(CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        turn_history: vec![],
        references: vec![],
    });
    assert_eq!(copilot_req.dialect(), Dialect::Copilot);
    assert_eq!(copilot_req.model(), "gpt-4o");
}

#[test]
fn dialect_request_tagged_serde() {
    let req = DialectRequest::Kimi(KimiRequest {
        model: "moonshot-v1-8k".into(),
        messages: vec![],
        max_tokens: None,
        temperature: None,
        stream: None,
        tools: None,
        use_search: None,
    });
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"dialect\":\"kimi\""));
    let back: DialectRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

// ── DialectResponse ─────────────────────────────────────────────────────

#[test]
fn dialect_response_dialect_for_all_variants() {
    let openai = DialectResponse::OpenAi(OpenAiResponse {
        id: "id".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    });
    assert_eq!(openai.dialect(), Dialect::OpenAi);

    let kimi = DialectResponse::Kimi(KimiResponse {
        id: "id".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    });
    assert_eq!(kimi.dialect(), Dialect::Kimi);

    let codex = DialectResponse::Codex(CodexResponse {
        id: "id".into(),
        model: "codex-mini-latest".into(),
        output: vec![],
        usage: None,
        status: None,
    });
    assert_eq!(codex.dialect(), Dialect::Codex);

    let copilot = DialectResponse::Copilot(CopilotResponse {
        message: "hi".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    });
    assert_eq!(copilot.dialect(), Dialect::Copilot);
}

// ── DialectStreamChunk ──────────────────────────────────────────────────

#[test]
fn dialect_stream_chunk_dialect_all_variants() {
    let openai = DialectStreamChunk::OpenAi(OpenAiStreamChunk {
        id: "id".into(),
        object: "chat.completion.chunk".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    });
    assert_eq!(openai.dialect(), Dialect::OpenAi);

    let claude = DialectStreamChunk::Claude(ClaudeStreamEvent::Ping {});
    assert_eq!(claude.dialect(), Dialect::Claude);

    let kimi = DialectStreamChunk::Kimi(KimiStreamChunk {
        id: "id".into(),
        object: "chat.completion.chunk".into(),
        model: "m".into(),
        choices: vec![],
        usage: None,
        refs: None,
    });
    assert_eq!(kimi.dialect(), Dialect::Kimi);

    let codex = DialectStreamChunk::Codex(CodexStreamEvent::Error {
        message: "err".into(),
        code: None,
    });
    assert_eq!(codex.dialect(), Dialect::Codex);

    let copilot = DialectStreamChunk::Copilot(CopilotStreamEvent::Done {});
    assert_eq!(copilot.dialect(), Dialect::Copilot);
}

// ── common::Role ────────────────────────────────────────────────────────

#[test]
fn role_serde_all_variants() {
    let roles = [
        (Role::System, "\"system\""),
        (Role::User, "\"user\""),
        (Role::Assistant, "\"assistant\""),
        (Role::Tool, "\"tool\""),
    ];
    for (role, expected_json) in roles {
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, expected_json);
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn role_display_all_variants() {
    assert_eq!(Role::System.to_string(), "system");
    assert_eq!(Role::User.to_string(), "user");
    assert_eq!(Role::Assistant.to_string(), "assistant");
    assert_eq!(Role::Tool.to_string(), "tool");
}

#[test]
fn role_clone_copy_eq_hash() {
    let r = Role::User;
    let r2 = r;
    let r3 = r.clone();
    assert_eq!(r, r2);
    assert_eq!(r, r3);
    let mut set = std::collections::HashSet::new();
    set.insert(r);
    assert!(set.contains(&Role::User));
}

// ── common::TokenUsage ─────────────────────────────────────────────────

#[test]
fn token_usage_default() {
    let u = TokenUsage::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.total_tokens.is_none());
}

#[test]
fn token_usage_large_values() {
    let u = TokenUsage {
        input_tokens: Some(u64::MAX),
        output_tokens: Some(u64::MAX),
        total_tokens: Some(u64::MAX),
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: TokenUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

#[test]
fn token_usage_partial_fields() {
    let u = TokenUsage {
        input_tokens: Some(100),
        output_tokens: None,
        total_tokens: Some(100),
    };
    let json = serde_json::to_string(&u).unwrap();
    assert!(!json.contains("output_tokens"));
    let back: TokenUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

// ── common::FinishReason ────────────────────────────────────────────────

#[test]
fn finish_reason_serde_all_variants() {
    let variants = [
        (FinishReason::Stop, "\"stop\""),
        (FinishReason::ToolUse, "\"tool_use\""),
        (FinishReason::MaxTokens, "\"max_tokens\""),
        (FinishReason::StopSequence, "\"stop_sequence\""),
        (FinishReason::ContentFilter, "\"content_filter\""),
    ];
    for (v, expected) in variants {
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, expected);
        let back: FinishReason = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn finish_reason_clone_eq_hash() {
    let f = FinishReason::ToolUse;
    let f2 = f.clone();
    assert_eq!(f, f2);
    let mut set = std::collections::HashSet::new();
    set.insert(f);
    assert!(set.contains(&FinishReason::ToolUse));
}

// ── convert::Message ────────────────────────────────────────────────────

#[test]
fn convert_message_roundtrip() {
    let msg = Message {
        role: "user".into(),
        content: Some("hello".into()),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn convert_message_with_tool_call_id() {
    let msg = Message {
        role: "tool".into(),
        content: Some("result".into()),
        tool_call_id: Some("call_123".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("call_123"));
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn convert_message_empty_content() {
    let msg = Message {
        role: "assistant".into(),
        content: Some("".into()),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content, Some("".into()));
}

#[test]
fn convert_message_none_content_skipped() {
    let msg = Message {
        role: "user".into(),
        content: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("content"));
}

// ── convert::ToolDefinition ─────────────────────────────────────────────

#[test]
fn tool_definition_roundtrip() {
    let td = ToolDefinition {
        name: "run_cmd".into(),
        description: "Run a shell command".into(),
        parameters: serde_json::json!({"type": "object"}),
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(td, back);
}

#[test]
fn tool_definition_special_chars_in_name() {
    let td = ToolDefinition {
        name: "my-tool_v2.0".into(),
        description: "A tool with <special> & \"chars\"".into(),
        parameters: serde_json::json!(null),
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: ToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(td, back);
}

// ── convert::ConversionError ────────────────────────────────────────────

#[test]
fn conversion_error_unsupported_field_display_and_serde() {
    let err = ConversionError::UnsupportedField {
        field: "system".into(),
        dialect: Dialect::Claude,
    };
    assert!(err.to_string().contains("system"));
    assert!(err.to_string().contains("Claude"));
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_incompatible_type_display_and_serde() {
    let err = ConversionError::IncompatibleType {
        source_type: "string".into(),
        target_type: "number".into(),
    };
    let s = err.to_string();
    assert!(s.contains("string"));
    assert!(s.contains("number"));
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_missing_required_field_display_and_serde() {
    let err = ConversionError::MissingRequiredField {
        field: "model".into(),
    };
    assert!(err.to_string().contains("model"));
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_content_too_long_display_and_serde() {
    let err = ConversionError::ContentTooLong {
        max: 500,
        actual: 1000,
    };
    let s = err.to_string();
    assert!(s.contains("500"));
    assert!(s.contains("1000"));
    let json = serde_json::to_string(&err).unwrap();
    let back: ConversionError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn conversion_error_is_std_error() {
    let err = ConversionError::MissingRequiredField { field: "x".into() };
    let _: &dyn std::error::Error = &err;
}

// ── convert::ConversionReport ───────────────────────────────────────────

#[test]
fn conversion_report_new_defaults() {
    let r = ConversionReport::new(Dialect::OpenAi, Dialect::Claude);
    assert_eq!(r.source, Dialect::OpenAi);
    assert_eq!(r.target, Dialect::Claude);
    assert_eq!(r.conversions, 0);
    assert!(r.warnings.is_empty());
    assert!(r.errors.is_empty());
    assert!(r.is_lossless);
    assert!(r.is_ok());
}

#[test]
fn conversion_report_is_ok_false_with_errors() {
    let mut r = ConversionReport::new(Dialect::Gemini, Dialect::Kimi);
    assert!(r.is_ok());
    r.errors.push(ConversionError::MissingRequiredField {
        field: "role".into(),
    });
    assert!(!r.is_ok());
}

#[test]
fn conversion_report_serde_roundtrip() {
    let mut r = ConversionReport::new(Dialect::Codex, Dialect::Copilot);
    r.conversions = 5;
    r.warnings.push("lossy".into());
    r.is_lossless = false;
    let json = serde_json::to_string(&r).unwrap();
    let back: ConversionReport = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn conversion_report_warnings_skipped_when_empty() {
    let r = ConversionReport::new(Dialect::OpenAi, Dialect::Gemini);
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("warnings"));
    assert!(!json.contains("errors"));
}

// ── convert::RoleMapper ─────────────────────────────────────────────────

#[test]
fn role_mapper_openai_to_gemini_all_supported() {
    assert_eq!(
        RoleMapper::map_role("user", Dialect::OpenAi, Dialect::Gemini).unwrap(),
        "user"
    );
    assert_eq!(
        RoleMapper::map_role("assistant", Dialect::OpenAi, Dialect::Gemini).unwrap(),
        "model"
    );
}

#[test]
fn role_mapper_gemini_model_to_openai_assistant() {
    assert_eq!(
        RoleMapper::map_role("model", Dialect::Gemini, Dialect::OpenAi).unwrap(),
        "assistant"
    );
}

#[test]
fn role_mapper_openai_system_to_claude_fails() {
    let err = RoleMapper::map_role("system", Dialect::OpenAi, Dialect::Claude).unwrap_err();
    assert!(matches!(err, ConversionError::UnsupportedField { .. }));
}

#[test]
fn role_mapper_openai_tool_to_gemini_fails() {
    let err = RoleMapper::map_role("tool", Dialect::OpenAi, Dialect::Gemini).unwrap_err();
    assert!(matches!(err, ConversionError::UnsupportedField { .. }));
}

#[test]
fn role_mapper_unknown_role_is_incompatible() {
    let err = RoleMapper::map_role("narrator", Dialect::OpenAi, Dialect::OpenAi).unwrap_err();
    assert!(matches!(err, ConversionError::IncompatibleType { .. }));
}

#[test]
fn role_mapper_codex_kimi_copilot_behave_like_openai() {
    for dialect in [Dialect::Codex, Dialect::Kimi, Dialect::Copilot] {
        assert_eq!(
            RoleMapper::map_role("system", dialect, Dialect::OpenAi).unwrap(),
            "system"
        );
        assert_eq!(
            RoleMapper::map_role("tool", dialect, Dialect::OpenAi).unwrap(),
            "tool"
        );
        assert_eq!(
            RoleMapper::map_role("user", Dialect::OpenAi, dialect).unwrap(),
            "user"
        );
        assert_eq!(
            RoleMapper::map_role("assistant", Dialect::OpenAi, dialect).unwrap(),
            "assistant"
        );
    }
}

#[test]
fn role_mapper_claude_user_to_openai() {
    assert_eq!(
        RoleMapper::map_role("user", Dialect::Claude, Dialect::OpenAi).unwrap(),
        "user"
    );
    assert_eq!(
        RoleMapper::map_role("assistant", Dialect::Claude, Dialect::OpenAi).unwrap(),
        "assistant"
    );
}

#[test]
fn role_mapper_claude_unknown_role() {
    let err = RoleMapper::map_role("system", Dialect::Claude, Dialect::OpenAi).unwrap_err();
    assert!(matches!(err, ConversionError::IncompatibleType { .. }));
}

#[test]
fn role_mapper_gemini_unknown_role() {
    let err = RoleMapper::map_role("tool", Dialect::Gemini, Dialect::OpenAi).unwrap_err();
    assert!(matches!(err, ConversionError::IncompatibleType { .. }));
}

// ── OpenAI types ────────────────────────────────────────────────────────

#[test]
fn openai_message_roundtrip() {
    let msg = OpenAiMessage {
        role: "user".into(),
        content: Some("hello world".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: OpenAiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn openai_message_with_tool_calls() {
    let msg = OpenAiMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAiFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"a.txt"}"#.into(),
            },
        }]),
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: OpenAiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn openai_tool_def_roundtrip() {
    let td = OpenAiToolDef {
        tool_type: "function".into(),
        function: OpenAiFunctionDef {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        },
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: OpenAiToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(td, back);
}

#[test]
fn openai_tool_choice_mode_variants() {
    for mode in [
        ToolChoiceMode::None,
        ToolChoiceMode::Auto,
        ToolChoiceMode::Required,
    ] {
        let tc = ToolChoice::Mode(mode.clone());
        let json = serde_json::to_string(&tc).unwrap();
        let back: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(tc, back);
    }
}

#[test]
fn openai_tool_choice_function_variant() {
    let tc = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "my_func".into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn openai_response_format_all_variants() {
    let text = ResponseFormat::Text {};
    let json_obj = ResponseFormat::JsonObject {};
    let json_schema = ResponseFormat::JsonSchema {
        json_schema: serde_json::json!({"type": "object"}),
    };
    for rf in [text, json_obj, json_schema] {
        let json = serde_json::to_string(&rf).unwrap();
        let back: ResponseFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(rf, back);
    }
}

#[test]
fn openai_request_full_roundtrip() {
    let req = OpenAiRequest {
        model: "gpt-4o".into(),
        messages: vec![
            OpenAiMessage {
                role: "system".into(),
                content: Some("Be helpful".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAiMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        tools: Some(vec![OpenAiToolDef {
            tool_type: "function".into(),
            function: OpenAiFunctionDef {
                name: "f".into(),
                description: "d".into(),
                parameters: serde_json::json!({}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        temperature: Some(0.5),
        max_tokens: Some(2048),
        response_format: Some(ResponseFormat::JsonObject {}),
        stream: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: OpenAiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn openai_response_with_usage() {
    let resp = OpenAiResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAiChoice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".into(),
                content: Some("Hi!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(OpenAiUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: OpenAiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn openai_stream_delta_default() {
    let delta = OpenAiStreamDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn openai_stream_chunk_roundtrip() {
    let chunk = OpenAiStreamChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAiStreamChoice {
            index: 0,
            delta: OpenAiStreamDelta {
                role: Some("assistant".into()),
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: OpenAiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn openai_config_default() {
    let cfg = OpenAiConfig::default();
    assert_eq!(cfg.base_url, "https://api.openai.com/v1");
    assert_eq!(cfg.model, "gpt-4o");
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
}

// ── Claude types ────────────────────────────────────────────────────────

#[test]
fn claude_message_roundtrip() {
    let msg = ClaudeMessage {
        role: "user".into(),
        content: "Hello Claude!".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ClaudeMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn claude_content_block_all_variants() {
    let blocks: Vec<ClaudeContentBlock> = vec![
        ClaudeContentBlock::Text {
            text: "Hello".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: serde_json::json!({"q": "rust"}),
        },
        ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("found".into()),
            is_error: Some(false),
        },
        ClaudeContentBlock::Thinking {
            thinking: "Let me reason...".into(),
            signature: Some("sig".into()),
        },
    ];
    for block in blocks {
        let json = serde_json::to_string(&block).unwrap();
        let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }
}

#[test]
fn claude_content_block_tool_result_minimal() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: None,
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("content"));
    assert!(!json.contains("is_error"));
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn claude_thinking_config_roundtrip() {
    let tc = ThinkingConfig {
        thinking_type: "enabled".into(),
        budget_tokens: 10000,
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn claude_request_full_roundtrip() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 8192,
        system: Some("Be helpful".into()),
        messages: vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }],
        tools: Some(vec![ClaudeToolDef {
            name: "search".into(),
            description: "Search".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }]),
        thinking: Some(ThinkingConfig {
            thinking_type: "enabled".into(),
            budget_tokens: 5000,
        }),
        stream: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn claude_response_roundtrip() {
    let resp = ClaudeResponse {
        id: "msg_abc".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text { text: "Hi!".into() }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 50,
            output_tokens: 20,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(5),
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn claude_stream_delta_all_variants() {
    let deltas: Vec<ClaudeStreamDelta> = vec![
        ClaudeStreamDelta::TextDelta {
            text: "chunk".into(),
        },
        ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"k"#.into(),
        },
        ClaudeStreamDelta::ThinkingDelta {
            thinking: "hmm".into(),
        },
        ClaudeStreamDelta::SignatureDelta {
            signature: "abc".into(),
        },
    ];
    for delta in deltas {
        let json = serde_json::to_string(&delta).unwrap();
        let back: ClaudeStreamDelta = serde_json::from_str(&json).unwrap();
        assert_eq!(delta, back);
    }
}

#[test]
fn claude_stream_event_all_variants() {
    let events: Vec<ClaudeStreamEvent> = vec![
        ClaudeStreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        },
        ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Text { text: "".into() },
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
        ClaudeStreamEvent::Error {
            error: ClaudeApiError {
                error_type: "overloaded_error".into(),
                message: "Overloaded".into(),
            },
        },
    ];
    for event in events {
        let json = serde_json::to_string(&event).unwrap();
        let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }
}

#[test]
fn claude_api_error_roundtrip() {
    let err = ClaudeApiError {
        error_type: "invalid_request_error".into(),
        message: "Missing required field".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ClaudeApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn claude_message_delta_roundtrip() {
    let delta = ClaudeMessageDelta {
        stop_reason: Some("max_tokens".into()),
        stop_sequence: Some("\n\nHuman:".into()),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: ClaudeMessageDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(delta, back);
}

#[test]
fn claude_config_default() {
    let cfg = ClaudeConfig::default();
    assert_eq!(cfg.base_url, "https://api.anthropic.com/v1");
    assert!(cfg.model.contains("claude"));
    assert_eq!(cfg.max_tokens, 4096);
    assert!(cfg.system_prompt.is_none());
    assert!(cfg.thinking.is_none());
}

#[test]
fn claude_usage_cache_fields_optional() {
    let usage = ClaudeUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&usage).unwrap();
    assert!(!json.contains("cache_creation"));
    assert!(!json.contains("cache_read"));
    let back: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

// ── Gemini types ────────────────────────────────────────────────────────

#[test]
fn gemini_part_all_variants() {
    let parts: Vec<GeminiPart> = vec![
        GeminiPart::Text("hello".into()),
        GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "iVBOR...".into(),
        }),
        GeminiPart::FunctionCall {
            name: "search".into(),
            args: serde_json::json!({"q": "rust"}),
        },
        GeminiPart::FunctionResponse {
            name: "search".into(),
            response: serde_json::json!({"results": []}),
        },
    ];
    for part in parts {
        let json = serde_json::to_string(&part).unwrap();
        let back: GeminiPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }
}

#[test]
fn gemini_content_roundtrip() {
    let content = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi".into())],
    };
    let json = serde_json::to_string(&content).unwrap();
    let back: GeminiContent = serde_json::from_str(&json).unwrap();
    assert_eq!(content, back);
}

#[test]
fn gemini_function_calling_mode_all_variants() {
    for mode in [
        FunctionCallingMode::Auto,
        FunctionCallingMode::Any,
        FunctionCallingMode::None,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: FunctionCallingMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}

#[test]
fn gemini_function_calling_mode_screaming_snake_case() {
    assert_eq!(
        serde_json::to_string(&FunctionCallingMode::Auto).unwrap(),
        "\"AUTO\""
    );
    assert_eq!(
        serde_json::to_string(&FunctionCallingMode::Any).unwrap(),
        "\"ANY\""
    );
    assert_eq!(
        serde_json::to_string(&FunctionCallingMode::None).unwrap(),
        "\"NONE\""
    );
}

#[test]
fn gemini_harm_category_all_variants() {
    let cats = [
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
    ];
    for cat in cats {
        let json = serde_json::to_string(&cat).unwrap();
        let back: HarmCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, back);
    }
}

#[test]
fn gemini_harm_block_threshold_all_variants() {
    let thresholds = [
        HarmBlockThreshold::BlockNone,
        HarmBlockThreshold::BlockMediumAndAbove,
        HarmBlockThreshold::BlockOnlyHigh,
    ];
    for t in thresholds {
        let json = serde_json::to_string(&t).unwrap();
        let back: HarmBlockThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}

#[test]
fn gemini_safety_setting_roundtrip() {
    let ss = GeminiSafetySetting {
        category: HarmCategory::HarmCategoryHarassment,
        threshold: HarmBlockThreshold::BlockMediumAndAbove,
    };
    let json = serde_json::to_string(&ss).unwrap();
    let back: GeminiSafetySetting = serde_json::from_str(&json).unwrap();
    assert_eq!(ss, back);
}

#[test]
fn gemini_tool_config_roundtrip() {
    let tc = GeminiToolConfig {
        function_calling_config: GeminiFunctionCallingConfig {
            mode: FunctionCallingMode::Any,
            allowed_function_names: Some(vec!["search".into(), "read".into()]),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: GeminiToolConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn gemini_generation_config_default() {
    let gc = GeminiGenerationConfig::default();
    assert!(gc.max_output_tokens.is_none());
    assert!(gc.temperature.is_none());
    assert!(gc.top_p.is_none());
    assert!(gc.top_k.is_none());
    assert!(gc.stop_sequences.is_none());
}

#[test]
fn gemini_request_full_roundtrip() {
    let req = GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hi".into())],
        }],
        system_instruction: Some(GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be helpful".into())],
        }),
        generation_config: Some(GeminiGenerationConfig {
            max_output_tokens: Some(4096),
            temperature: Some(0.7),
            top_p: Some(0.95),
            top_k: Some(40),
            stop_sequences: Some(vec!["STOP".into()]),
        }),
        safety_settings: Some(vec![GeminiSafetySetting {
            category: HarmCategory::HarmCategoryHarassment,
            threshold: HarmBlockThreshold::BlockOnlyHigh,
        }]),
        tools: Some(vec![GeminiTool {
            function_declarations: vec![GeminiFunctionDeclaration {
                name: "search".into(),
                description: "Search".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
        }]),
        tool_config: Some(GeminiToolConfig {
            function_calling_config: GeminiFunctionCallingConfig {
                mode: FunctionCallingMode::Auto,
                allowed_function_names: None,
            },
        }),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: GeminiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn gemini_response_roundtrip() {
    let resp = GeminiResponse {
        candidates: vec![GeminiCandidate {
            content: GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hello!".into())],
            },
            finish_reason: Some("STOP".into()),
        }],
        usage_metadata: Some(GeminiUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: GeminiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn gemini_stream_chunk_empty_candidates() {
    let chunk = GeminiStreamChunk {
        candidates: vec![],
        usage_metadata: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: GeminiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn gemini_config_default() {
    let cfg = GeminiConfig::default();
    assert!(cfg.base_url.contains("googleapis.com"));
    assert_eq!(cfg.model, "gemini-2.5-flash");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
}

#[test]
fn gemini_inline_data_roundtrip() {
    let data = GeminiInlineData {
        mime_type: "image/jpeg".into(),
        data: "base64data==".into(),
    };
    let json = serde_json::to_string(&data).unwrap();
    let back: GeminiInlineData = serde_json::from_str(&json).unwrap();
    assert_eq!(data, back);
}

// ── Kimi types ──────────────────────────────────────────────────────────

#[test]
fn kimi_message_roundtrip() {
    let msg = KimiMessage {
        role: "user".into(),
        content: Some("Hello Kimi".into()),
        tool_call_id: None,
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: KimiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn kimi_tool_function_variant() {
    let tool = KimiTool::Function {
        function: KimiFunctionDef {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn kimi_tool_builtin_function_variant() {
    let tool = KimiTool::BuiltinFunction {
        function: KimiBuiltinFunction {
            name: "$web_search".into(),
        },
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: KimiTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn kimi_ref_roundtrip() {
    let r = KimiRef {
        index: 1,
        url: "https://example.com".into(),
        title: Some("Example".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn kimi_ref_without_title() {
    let r = KimiRef {
        index: 42,
        url: "https://example.com/page".into(),
        title: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("title"));
    let back: KimiRef = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn kimi_request_with_tools_and_search() {
    let req = KimiRequest {
        model: "moonshot-v1-128k".into(),
        messages: vec![KimiMessage {
            role: "user".into(),
            content: Some("Search for Rust".into()),
            tool_call_id: None,
            tool_calls: None,
        }],
        max_tokens: Some(8192),
        temperature: Some(0.3),
        stream: Some(true),
        tools: Some(vec![KimiTool::BuiltinFunction {
            function: KimiBuiltinFunction {
                name: "$web_search".into(),
            },
        }]),
        use_search: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn kimi_response_with_refs() {
    let resp = KimiResponse {
        id: "cmpl_abc".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: Some("Found: [1]".into()),
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(KimiUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        }),
        refs: Some(vec![KimiRef {
            index: 1,
            url: "https://rust-lang.org".into(),
            title: Some("Rust".into()),
        }]),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: KimiResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn kimi_chunk_delta_default() {
    let delta = KimiChunkDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
}

#[test]
fn kimi_stream_chunk_roundtrip() {
    let chunk = KimiStreamChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta {
                role: None,
                content: Some("token".into()),
            },
            finish_reason: None,
        }],
        usage: None,
        refs: None,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let back: KimiStreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, back);
}

#[test]
fn kimi_config_default() {
    let cfg = KimiConfig::default();
    assert!(cfg.base_url.contains("moonshot.cn"));
    assert_eq!(cfg.model, "moonshot-v1-8k");
    assert_eq!(cfg.max_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
    assert!(cfg.use_k1_reasoning.is_none());
}

// ── Codex types ─────────────────────────────────────────────────────────

#[test]
fn codex_input_item_message_roundtrip() {
    let item = CodexInputItem::Message {
        role: "user".into(),
        content: "Write tests".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let back: CodexInputItem = serde_json::from_str(&json).unwrap();
    assert_eq!(item, back);
}

#[test]
fn codex_tool_all_variants() {
    let tools: Vec<CodexTool> = vec![
        CodexTool::Function {
            function: CodexFunctionDef {
                name: "shell".into(),
                description: "Run command".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        },
        CodexTool::CodeInterpreter {},
        CodexTool::FileSearch {
            max_num_results: Some(10),
        },
    ];
    for tool in tools {
        let json = serde_json::to_string(&tool).unwrap();
        let back: CodexTool = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }
}

#[test]
fn codex_text_format_all_variants() {
    let formats: Vec<CodexTextFormat> = vec![
        CodexTextFormat::Text {},
        CodexTextFormat::JsonObject {},
        CodexTextFormat::JsonSchema {
            name: "my_schema".into(),
            schema: serde_json::json!({"type": "object"}),
            strict: true,
        },
    ];
    for fmt in formats {
        let json = serde_json::to_string(&fmt).unwrap();
        let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, back);
    }
}

#[test]
fn codex_network_access_default() {
    assert_eq!(NetworkAccess::default(), NetworkAccess::None);
}

#[test]
fn codex_file_access_default() {
    assert_eq!(FileAccess::default(), FileAccess::WorkspaceOnly);
}

#[test]
fn codex_sandbox_config_default() {
    let cfg = SandboxConfig::default();
    assert!(cfg.container_image.is_none());
    assert_eq!(cfg.networking, NetworkAccess::None);
    assert_eq!(cfg.file_access, FileAccess::WorkspaceOnly);
    assert_eq!(cfg.timeout_seconds, Some(300));
    assert!(cfg.env.is_empty());
}

#[test]
fn codex_sandbox_config_with_env() {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "/usr/bin".into());
    env.insert("HOME".into(), "/home/user".into());
    let cfg = SandboxConfig {
        container_image: Some("ubuntu:22.04".into()),
        networking: NetworkAccess::Full,
        file_access: FileAccess::Full,
        timeout_seconds: Some(600),
        env,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn codex_response_item_all_variants() {
    let items: Vec<CodexResponseItem> = vec![
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done".into(),
            }],
        },
        CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: Some("call_1".into()),
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
        CodexResponseItem::FunctionCallOutput {
            call_id: "call_1".into(),
            output: "file1\nfile2".into(),
        },
        CodexResponseItem::Reasoning {
            summary: vec![ReasoningSummary {
                text: "I need to check...".into(),
            }],
        },
    ];
    for item in items {
        let json = serde_json::to_string(&item).unwrap();
        let back: CodexResponseItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item, back);
    }
}

#[test]
fn codex_stream_delta_all_variants() {
    let deltas: Vec<CodexStreamDelta> = vec![
        CodexStreamDelta::OutputTextDelta {
            text: "hello".into(),
        },
        CodexStreamDelta::FunctionCallArgumentsDelta {
            delta: r#"{"cm"#.into(),
        },
    ];
    for d in deltas {
        let json = serde_json::to_string(&d).unwrap();
        let back: CodexStreamDelta = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn codex_stream_event_all_variants() {
    let resp = CodexResponse {
        id: "r1".into(),
        model: "codex-mini-latest".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let item = CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![],
    };
    let events: Vec<CodexStreamEvent> = vec![
        CodexStreamEvent::ResponseCreated {
            response: resp.clone(),
        },
        CodexStreamEvent::OutputItemAdded {
            output_index: 0,
            item: item.clone(),
        },
        CodexStreamEvent::OutputItemDelta {
            output_index: 0,
            delta: CodexStreamDelta::OutputTextDelta { text: "hi".into() },
        },
        CodexStreamEvent::OutputItemDone {
            output_index: 0,
            item: item.clone(),
        },
        CodexStreamEvent::ResponseCompleted { response: resp },
        CodexStreamEvent::Error {
            message: "something went wrong".into(),
            code: Some("rate_limit".into()),
        },
    ];
    for event in events {
        let json = serde_json::to_string(&event).unwrap();
        let back: CodexStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }
}

#[test]
fn codex_request_full_roundtrip() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Write hello world".into(),
        }],
        max_output_tokens: Some(4096),
        temperature: Some(0.0),
        tools: vec![
            CodexTool::CodeInterpreter {},
            CodexTool::FileSearch {
                max_num_results: None,
            },
        ],
        text: Some(CodexTextFormat::Text {}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn codex_config_default() {
    let cfg = CodexConfig::default();
    assert!(cfg.base_url.contains("openai.com"));
    assert_eq!(cfg.model, "codex-mini-latest");
    assert_eq!(cfg.max_output_tokens, Some(4096));
    assert!(cfg.temperature.is_none());
    assert_eq!(cfg.sandbox, SandboxConfig::default());
}

#[test]
fn codex_reasoning_summary_roundtrip() {
    let rs = ReasoningSummary {
        text: "Step 1: analyze the problem".into(),
    };
    let json = serde_json::to_string(&rs).unwrap();
    let back: ReasoningSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(rs, back);
}

// ── Copilot types ───────────────────────────────────────────────────────

#[test]
fn copilot_reference_type_all_variants() {
    let types = [
        CopilotReferenceType::File,
        CopilotReferenceType::Snippet,
        CopilotReferenceType::Repository,
        CopilotReferenceType::WebSearchResult,
    ];
    for t in types {
        let json = serde_json::to_string(&t).unwrap();
        let back: CopilotReferenceType = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}

#[test]
fn copilot_reference_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("label".into(), serde_json::json!("Source file"));
    meta.insert("uri".into(), serde_json::json!("file:///src/main.rs"));
    let r = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "file-1".into(),
        data: serde_json::json!({"path": "src/main.rs", "content": "fn main() {}"}),
        metadata: Some(meta),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn copilot_reference_without_metadata() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::Repository,
        id: "repo-1".into(),
        data: serde_json::json!({"owner": "octocat", "name": "hello-world"}),
        metadata: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("metadata"));
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn copilot_message_with_references() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "Look at this file".into(),
        name: Some("user1".into()),
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f0".into(),
            data: serde_json::json!({"path": "a.rs"}),
            metadata: None,
        }],
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: CopilotMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn copilot_tool_type_all_variants() {
    let types = [CopilotToolType::Function, CopilotToolType::Confirmation];
    for t in types {
        let json = serde_json::to_string(&t).unwrap();
        let back: CopilotToolType = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}

#[test]
fn copilot_tool_function_roundtrip() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
        }),
        confirmation: None,
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn copilot_tool_confirmation_roundtrip() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "conf_1".into(),
            title: "Delete file?".into(),
            message: "Are you sure you want to delete main.rs?".into(),
            accepted: None,
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn copilot_confirmation_accepted() {
    let c = CopilotConfirmation {
        id: "c1".into(),
        title: "Deploy?".into(),
        message: "Deploy to production?".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: CopilotConfirmation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.accepted, Some(true));
}

#[test]
fn copilot_function_call_roundtrip() {
    let fc = CopilotFunctionCall {
        name: "write_file".into(),
        arguments: r#"{"path":"a.rs","content":"fn main(){}"}"#.into(),
        id: Some("call_abc".into()),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let back: CopilotFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(fc, back);
}

#[test]
fn copilot_error_roundtrip() {
    let err = CopilotError {
        error_type: "rate_limit".into(),
        message: "Too many requests".into(),
        code: Some("429".into()),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: CopilotError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn copilot_turn_entry_roundtrip() {
    let entry = CopilotTurnEntry {
        request: "What is Rust?".into(),
        response: "Rust is a systems programming language.".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: CopilotTurnEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn copilot_request_with_turn_history() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "Tell me more".into(),
            name: None,
            copilot_references: vec![],
        }],
        tools: None,
        turn_history: vec![CopilotTurnEntry {
            request: "What is Rust?".into(),
            response: "A language.".into(),
        }],
        references: vec![CopilotReference {
            ref_type: CopilotReferenceType::WebSearchResult,
            id: "ws-1".into(),
            data: serde_json::json!({"url": "https://rust-lang.org"}),
            metadata: None,
        }],
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn copilot_response_full_roundtrip() {
    let resp = CopilotResponse {
        message: "Here you go".into(),
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::Snippet,
            id: "s1".into(),
            data: serde_json::json!({"code": "fn main(){}"}),
            metadata: None,
        }],
        copilot_errors: vec![CopilotError {
            error_type: "warning".into(),
            message: "Partial result".into(),
            code: None,
        }],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Apply?".into(),
            message: "Apply changes?".into(),
            accepted: Some(false),
        }),
        function_call: Some(CopilotFunctionCall {
            name: "apply_diff".into(),
            arguments: "{}".into(),
            id: None,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: CopilotResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn copilot_stream_event_all_variants() {
    let events: Vec<CopilotStreamEvent> = vec![
        CopilotStreamEvent::CopilotReferences {
            references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: serde_json::json!({}),
                metadata: None,
            }],
        },
        CopilotStreamEvent::CopilotErrors {
            errors: vec![CopilotError {
                error_type: "error".into(),
                message: "fail".into(),
                code: None,
            }],
        },
        CopilotStreamEvent::TextDelta {
            text: "Hello".into(),
        },
        CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "f".into(),
                arguments: "{}".into(),
                id: None,
            },
        },
        CopilotStreamEvent::CopilotConfirmation {
            confirmation: CopilotConfirmation {
                id: "c1".into(),
                title: "Ok?".into(),
                message: "Proceed?".into(),
                accepted: None,
            },
        },
        CopilotStreamEvent::Done {},
    ];
    for event in events {
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }
}

#[test]
fn copilot_config_default() {
    let cfg = CopilotConfig::default();
    assert!(cfg.base_url.contains("githubcopilot"));
    assert_eq!(cfg.model, "gpt-4o");
    assert!(cfg.system_prompt.is_none());
}

// ── Edge cases: unicode, empty strings, special chars ───────────────────

#[test]
fn unicode_in_openai_message_content() {
    let msg = OpenAiMessage {
        role: "user".into(),
        content: Some("こんにちは 🌍 مرحبا".into()),
        tool_calls: None,
        tool_call_id: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: OpenAiMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn empty_string_fields_in_claude_message() {
    let msg = ClaudeMessage {
        role: "".into(),
        content: "".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: ClaudeMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "");
    assert_eq!(back.content, "");
}

#[test]
fn special_chars_in_function_arguments() {
    let fc = OpenAiFunctionCall {
        name: "eval".into(),
        arguments: r#"{"code":"print(\"hello\\nworld\")"}"#.into(),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let back: OpenAiFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(fc, back);
}

#[test]
fn large_token_values_in_openai_usage() {
    let usage = OpenAiUsage {
        prompt_tokens: u64::MAX,
        completion_tokens: u64::MAX,
        total_tokens: u64::MAX,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: OpenAiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn zero_values_in_codex_usage() {
    let usage = CodexUsage {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: CodexUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

// ── Cross-type consistency checks ───────────────────────────────────────

#[test]
fn dialect_request_serde_tag_consistency() {
    // All DialectRequest variants serialize with "dialect" tag
    let variants: Vec<DialectRequest> = vec![
        DialectRequest::OpenAi(OpenAiRequest {
            model: "m".into(),
            messages: vec![],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            response_format: None,
            stream: None,
        }),
        DialectRequest::Claude(ClaudeRequest {
            model: "m".into(),
            max_tokens: 100,
            system: None,
            messages: vec![],
            tools: None,
            thinking: None,
            stream: None,
        }),
        DialectRequest::Gemini(GeminiRequest {
            model: "m".into(),
            contents: vec![],
            system_instruction: None,
            generation_config: None,
            safety_settings: None,
            tools: None,
            tool_config: None,
        }),
        DialectRequest::Kimi(KimiRequest {
            model: "m".into(),
            messages: vec![],
            max_tokens: None,
            temperature: None,
            stream: None,
            tools: None,
            use_search: None,
        }),
        DialectRequest::Codex(CodexRequest {
            model: "m".into(),
            input: vec![],
            max_output_tokens: None,
            temperature: None,
            tools: vec![],
            text: None,
        }),
        DialectRequest::Copilot(CopilotRequest {
            model: "m".into(),
            messages: vec![],
            tools: None,
            turn_history: vec![],
            references: vec![],
        }),
    ];
    for req in &variants {
        let json = serde_json::to_string(req).unwrap();
        assert!(
            json.contains("\"dialect\""),
            "Missing dialect tag in {}",
            json
        );
        let back: DialectRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, &back);
    }
}

#[test]
fn dialect_request_model_matches_inner_model() {
    let req = DialectRequest::Gemini(GeminiRequest {
        model: "gemini-2.5-flash".into(),
        contents: vec![],
        system_instruction: None,
        generation_config: None,
        safety_settings: None,
        tools: None,
        tool_config: None,
    });
    assert_eq!(req.model(), "gemini-2.5-flash");
}

#[test]
fn all_configs_roundtrip() {
    let openai = OpenAiConfig::default();
    let claude = ClaudeConfig::default();
    let gemini = GeminiConfig::default();
    let kimi = KimiConfig::default();
    let codex = CodexConfig::default();
    let copilot = CopilotConfig::default();

    let json_openai = serde_json::to_string(&openai).unwrap();
    assert_eq!(
        openai,
        serde_json::from_str::<OpenAiConfig>(&json_openai).unwrap()
    );

    let json_claude = serde_json::to_string(&claude).unwrap();
    assert_eq!(
        claude,
        serde_json::from_str::<ClaudeConfig>(&json_claude).unwrap()
    );

    let json_gemini = serde_json::to_string(&gemini).unwrap();
    assert_eq!(
        gemini,
        serde_json::from_str::<GeminiConfig>(&json_gemini).unwrap()
    );

    let json_kimi = serde_json::to_string(&kimi).unwrap();
    assert_eq!(
        kimi,
        serde_json::from_str::<KimiConfig>(&json_kimi).unwrap()
    );

    let json_codex = serde_json::to_string(&codex).unwrap();
    assert_eq!(
        codex,
        serde_json::from_str::<CodexConfig>(&json_codex).unwrap()
    );

    let json_copilot = serde_json::to_string(&copilot).unwrap();
    assert_eq!(
        copilot,
        serde_json::from_str::<CopilotConfig>(&json_copilot).unwrap()
    );
}

#[test]
fn network_access_serde_roundtrip() {
    for na in [NetworkAccess::None, NetworkAccess::Full] {
        let json = serde_json::to_string(&na).unwrap();
        let back: NetworkAccess = serde_json::from_str(&json).unwrap();
        assert_eq!(na, back);
    }
}

#[test]
fn file_access_serde_roundtrip() {
    for fa in [FileAccess::WorkspaceOnly, FileAccess::Full] {
        let json = serde_json::to_string(&fa).unwrap();
        let back: FileAccess = serde_json::from_str(&json).unwrap();
        assert_eq!(fa, back);
    }
}

#[test]
fn codex_file_search_no_max_results() {
    let tool = CodexTool::FileSearch {
        max_num_results: None,
    };
    let json = serde_json::to_string(&tool).unwrap();
    assert!(!json.contains("max_num_results"));
    let back: CodexTool = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn gemini_function_calling_config_no_allowed_names() {
    let cfg = GeminiFunctionCallingConfig {
        mode: FunctionCallingMode::Auto,
        allowed_function_names: None,
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("allowedFunctionNames"));
    let back: GeminiFunctionCallingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn codex_text_format_json_schema_strict_false() {
    let fmt = CodexTextFormat::JsonSchema {
        name: "output".into(),
        schema: serde_json::json!({"type": "object"}),
        strict: false,
    };
    let json = serde_json::to_string(&fmt).unwrap();
    let back: CodexTextFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(fmt, back);
}

#[test]
fn copilot_message_empty_references_skipped() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "Hi".into(),
        name: None,
        copilot_references: vec![],
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("copilot_references"));
}

#[test]
fn copilot_response_empty_collections_skipped() {
    let resp = CopilotResponse {
        message: "ok".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("copilot_references"));
    assert!(!json.contains("copilot_errors"));
    assert!(!json.contains("copilot_confirmation"));
    assert!(!json.contains("function_call"));
}

#[test]
fn kimi_tool_call_roundtrip() {
    let tc = KimiToolCall {
        id: "call_xyz".into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: "web_search".into(),
            arguments: r#"{"query":"test"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: KimiToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, back);
}

#[test]
fn claude_tool_def_roundtrip() {
    let td = ClaudeToolDef {
        name: "bash".into(),
        description: "Run a bash command".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string"}},
            "required": ["command"]
        }),
    };
    let json = serde_json::to_string(&td).unwrap();
    let back: ClaudeToolDef = serde_json::from_str(&json).unwrap();
    assert_eq!(td, back);
}

#[test]
fn gemini_function_declaration_roundtrip() {
    let fd = GeminiFunctionDeclaration {
        name: "get_weather".into(),
        description: "Get weather for a location".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"location": {"type": "string"}}
        }),
    };
    let json = serde_json::to_string(&fd).unwrap();
    let back: GeminiFunctionDeclaration = serde_json::from_str(&json).unwrap();
    assert_eq!(fd, back);
}

#[test]
fn openai_usage_roundtrip() {
    let u = OpenAiUsage {
        prompt_tokens: 42,
        completion_tokens: 17,
        total_tokens: 59,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: OpenAiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

#[test]
fn kimi_usage_roundtrip() {
    let u = KimiUsage {
        prompt_tokens: 200,
        completion_tokens: 100,
        total_tokens: 300,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: KimiUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

#[test]
fn gemini_usage_metadata_roundtrip() {
    let u = GeminiUsageMetadata {
        prompt_token_count: 50,
        candidates_token_count: 25,
        total_token_count: 75,
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: GeminiUsageMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(u, back);
}

#[test]
fn codex_content_part_output_text_roundtrip() {
    let cp = CodexContentPart::OutputText {
        text: "Result here".into(),
    };
    let json = serde_json::to_string(&cp).unwrap();
    let back: CodexContentPart = serde_json::from_str(&json).unwrap();
    assert_eq!(cp, back);
}

#[test]
fn openai_config_serde_roundtrip() {
    let cfg = OpenAiConfig {
        base_url: "https://custom.api/v1".into(),
        model: "gpt-5".into(),
        max_tokens: Some(16384),
        temperature: Some(1.5),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: OpenAiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn claude_config_serde_roundtrip() {
    let cfg = ClaudeConfig {
        base_url: "https://custom.anthropic/v1".into(),
        model: "claude-opus".into(),
        max_tokens: 16000,
        system_prompt: Some("Be concise".into()),
        thinking: Some(ThinkingConfig {
            thinking_type: "enabled".into(),
            budget_tokens: 20000,
        }),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}
