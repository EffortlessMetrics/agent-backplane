#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive mapper validation tests covering all mapping rules, validation,
//! and error handling for the abp-mapper crate.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEvent, AgentEventKind};
use abp_dialect::Dialect;
use abp_mapper::validation::{
    DefaultMappingValidator, MappingValidator, PipelineResult, RoundtripResult, ValidationIssue,
    ValidationPipeline, ValidationResult, ValidationSeverity,
};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, ClaudeToOpenAiMapper, CodexClaudeIrMapper,
    DialectRequest, DialectResponse, GeminiKimiIrMapper, GeminiToOpenAiMapper, IdentityMapper,
    IrIdentityMapper, IrMapper, MapError, Mapper, MappingError, OpenAiClaudeIrMapper,
    OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
    OpenAiToClaudeMapper, OpenAiToGeminiMapper,
};
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use chrono::Utc;
use serde_json::{Value, json};

// ── Helpers ────────────────────────────────────────────────────────────────

fn simple_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "You are helpful.".into(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Text {
                text: "Hi there!".into(),
            }],
        ),
    ])
}

fn tool_call_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "What's the weather?".into(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "get_weather".into(),
                input: json!({"location": "NYC"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F, sunny".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

fn thinking_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "Explain quantum computing".into(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think about this...".into(),
                },
                IrContentBlock::Text {
                    text: "Quantum computing uses qubits.".into(),
                },
            ],
        ),
    ])
}

fn image_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBOR...".into(),
            },
        ],
    )])
}

fn complex_ir_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "You are an expert assistant.".into(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Analyze this image and use tools.".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/jpeg".into(),
                    data: "base64data".into(),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "I need to call a tool.".into(),
                },
                IrContentBlock::Text {
                    text: "Let me search for that.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "search".into(),
                    input: json!({"query": "analysis"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Found results.".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Text {
                text: "Based on the results, here is my analysis.".into(),
            }],
        ),
    ])
}

fn validator() -> DefaultMappingValidator {
    DefaultMappingValidator::new()
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. supported_ir_pairs: Test all 24 supported IR pairs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn supported_ir_pairs_returns_24_pairs() {
    let pairs = supported_ir_pairs();
    assert_eq!(pairs.len(), 24, "Expected 24 supported IR pairs");
}

#[test]
fn supported_ir_pairs_includes_all_identity_pairs() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "Missing identity pair for {:?}", d);
    }
}

#[test]
fn supported_ir_pairs_includes_openai_claude_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
}

#[test]
fn supported_ir_pairs_includes_openai_gemini_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
}

#[test]
fn supported_ir_pairs_includes_claude_gemini_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
}

#[test]
fn supported_ir_pairs_includes_openai_codex_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Codex)));
    assert!(pairs.contains(&(Dialect::Codex, Dialect::OpenAi)));
}

#[test]
fn supported_ir_pairs_includes_openai_kimi_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::OpenAi)));
}

#[test]
fn supported_ir_pairs_includes_claude_kimi_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Claude)));
}

#[test]
fn supported_ir_pairs_includes_openai_copilot_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
    assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
}

#[test]
fn supported_ir_pairs_includes_gemini_kimi_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
}

#[test]
fn supported_ir_pairs_includes_codex_claude_both_directions() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. default_ir_mapper factory: Test all pairs return Some and unsupported None
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_ir_mapper_returns_some_for_all_supported_pairs() {
    for (from, to) in supported_ir_pairs() {
        assert!(
            default_ir_mapper(from, to).is_some(),
            "default_ir_mapper({:?}, {:?}) should return Some",
            from,
            to
        );
    }
}

#[test]
fn default_ir_mapper_identity_for_same_dialect() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d).expect("identity mapper should exist");
        let ir = simple_ir_conversation();
        let result = mapper.map_request(d, d, &ir).unwrap();
        assert_eq!(result.messages.len(), ir.messages.len());
    }
}

#[test]
fn default_ir_mapper_returns_none_for_unsupported_codex_copilot() {
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Copilot).is_none());
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Codex).is_none());
}

#[test]
fn default_ir_mapper_returns_none_for_unsupported_codex_kimi() {
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Kimi).is_none());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Codex).is_none());
}

#[test]
fn default_ir_mapper_returns_none_for_unsupported_codex_gemini() {
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Gemini).is_none());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Codex).is_none());
}

#[test]
fn default_ir_mapper_returns_none_for_unsupported_copilot_claude() {
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Claude).is_none());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Copilot).is_none());
}

#[test]
fn default_ir_mapper_returns_none_for_unsupported_copilot_gemini() {
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Gemini).is_none());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Copilot).is_none());
}

#[test]
fn default_ir_mapper_returns_none_for_unsupported_copilot_kimi() {
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Kimi).is_none());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. IrMapper trait: Test supported_pairs for each concrete mapper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_identity_mapper_supported_pairs_are_all_identity() {
    let mapper = IrIdentityMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), Dialect::all().len());
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)));
    }
}

#[test]
fn openai_claude_ir_mapper_supported_pairs() {
    let mapper = OpenAiClaudeIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
}

#[test]
fn openai_gemini_ir_mapper_supported_pairs() {
    let mapper = OpenAiGeminiIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
}

#[test]
fn claude_gemini_ir_mapper_supported_pairs() {
    let mapper = ClaudeGeminiIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
}

#[test]
fn openai_codex_ir_mapper_supported_pairs() {
    let mapper = OpenAiCodexIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Codex)));
    assert!(pairs.contains(&(Dialect::Codex, Dialect::OpenAi)));
}

#[test]
fn openai_kimi_ir_mapper_supported_pairs() {
    let mapper = OpenAiKimiIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::OpenAi)));
}

#[test]
fn claude_kimi_ir_mapper_supported_pairs() {
    let mapper = ClaudeKimiIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Claude)));
}

#[test]
fn openai_copilot_ir_mapper_supported_pairs() {
    let mapper = OpenAiCopilotIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
    assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
}

#[test]
fn gemini_kimi_ir_mapper_supported_pairs() {
    let mapper = GeminiKimiIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
}

#[test]
fn codex_claude_ir_mapper_supported_pairs() {
    let mapper = CodexClaudeIrMapper;
    let pairs = mapper.supported_pairs();
    assert_eq!(pairs.len(), 2);
    assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. IrMapper unsupported pair errors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_claude_mapper_rejects_unsupported_pair() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn openai_gemini_mapper_rejects_unsupported_pair() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn claude_gemini_mapper_rejects_unsupported_pair() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn openai_codex_mapper_rejects_unsupported_pair() {
    let mapper = OpenAiCodexIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn openai_kimi_mapper_rejects_unsupported_pair() {
    let mapper = OpenAiKimiIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn claude_kimi_mapper_rejects_unsupported_pair() {
    let mapper = ClaudeKimiIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn openai_copilot_mapper_rejects_unsupported_pair() {
    let mapper = OpenAiCopilotIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn gemini_kimi_mapper_rejects_unsupported_pair() {
    let mapper = GeminiKimiIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn codex_claude_mapper_rejects_unsupported_pair() {
    let mapper = CodexClaudeIrMapper;
    let ir = simple_ir_conversation();
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. MapError types and messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_error_unsupported_pair_display() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("Kimi"));
    assert!(msg.contains("Copilot"));
    assert!(msg.contains("unsupported dialect pair"));
}

#[test]
fn map_error_lossy_conversion_display() {
    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "target has no thinking block".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("lossy"));
}

#[test]
fn map_error_unmappable_tool_display() {
    let err = MapError::UnmappableTool {
        name: "computer_use".into(),
        reason: "not supported in target".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("computer_use"));
    assert!(msg.contains("unmappable tool"));
}

#[test]
fn map_error_incompatible_capability_display() {
    let err = MapError::IncompatibleCapability {
        capability: "logprobs".into(),
        reason: "target dialect does not support logprobs".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
    assert!(msg.contains("incompatible capability"));
}

#[test]
fn map_error_unmappable_content_display() {
    let err = MapError::UnmappableContent {
        field: "system".into(),
        reason: "image blocks in system prompt".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("system"));
    assert!(msg.contains("image blocks"));
}

#[test]
fn map_error_serde_roundtrip_unsupported_pair() {
    let err = MapError::UnsupportedPair {
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let json_str = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json_str).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_serde_roundtrip_lossy_conversion() {
    let err = MapError::LossyConversion {
        field: "system_instruction".into(),
        reason: "flattened".into(),
    };
    let json_str = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json_str).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_serde_roundtrip_unmappable_tool() {
    let err = MapError::UnmappableTool {
        name: "bash".into(),
        reason: "restricted".into(),
    };
    let json_str = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json_str).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_serde_roundtrip_incompatible_capability() {
    let err = MapError::IncompatibleCapability {
        capability: "vision".into(),
        reason: "no image support".into(),
    };
    let json_str = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json_str).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_serde_roundtrip_unmappable_content() {
    let err = MapError::UnmappableContent {
        field: "system".into(),
        reason: "image blocks".into(),
    };
    let json_str = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json_str).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_clone_equality() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Gemini,
        to: Dialect::Kimi,
    };
    let cloned = err.clone();
    assert_eq!(err, cloned);
}

#[test]
fn map_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MapError>();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. MappingError types and messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_unsupported_capability() {
    let err = MappingError::UnsupportedCapability {
        capability: "logprobs".into(),
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
    assert!(msg.contains("unsupported capability"));
}

#[test]
fn mapping_error_incompatible_types() {
    let err = MappingError::IncompatibleTypes {
        source_type: "function_call".into(),
        target_type: "tool_use".into(),
        reason: "schema mismatch".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("function_call"));
    assert!(msg.contains("tool_use"));
}

#[test]
fn mapping_error_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        field: "thinking".into(),
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        detail: "no native thinking block".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("fidelity loss"));
    assert!(msg.contains("thinking"));
}

#[test]
fn mapping_error_unmappable_request() {
    let err = MappingError::UnmappableRequest {
        reason: "empty messages array".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("unmappable request"));
    assert!(msg.contains("empty messages array"));
}

#[test]
fn mapping_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MappingError>();
}

#[test]
fn mapping_error_clone() {
    let err = MappingError::UnsupportedCapability {
        capability: "x".into(),
        source_dialect: Dialect::Gemini,
        target_dialect: Dialect::Kimi,
    };
    let cloned = err.clone();
    assert_eq!(err.to_string(), cloned.to_string());
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Validation: pre-mapping for all dialects
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pre_mapping_valid_openai() {
    let v = validator();
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn pre_mapping_valid_claude() {
    let v = validator();
    let req = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1024
    });
    let r = v.validate_pre_mapping(Dialect::Claude, &req);
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn pre_mapping_valid_gemini() {
    let v = validator();
    let req = json!({
        "model": "gemini-pro",
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
    });
    let r = v.validate_pre_mapping(Dialect::Gemini, &req);
    assert!(r.is_valid());
    assert_eq!(r.field_coverage, 100.0);
}

#[test]
fn pre_mapping_valid_codex() {
    let v = validator();
    let req = json!({
        "model": "codex-mini",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let r = v.validate_pre_mapping(Dialect::Codex, &req);
    assert!(r.is_valid());
}

#[test]
fn pre_mapping_valid_kimi() {
    let v = validator();
    let req = json!({
        "model": "moonshot-v1",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let r = v.validate_pre_mapping(Dialect::Kimi, &req);
    assert!(r.is_valid());
}

#[test]
fn pre_mapping_valid_copilot() {
    let v = validator();
    let req = json!({
        "model": "copilot-gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let r = v.validate_pre_mapping(Dialect::Copilot, &req);
    assert!(r.is_valid());
}

#[test]
fn pre_mapping_missing_model_all_dialects() {
    let v = validator();
    for &d in Dialect::all() {
        let msgs_key = if d == Dialect::Gemini {
            "contents"
        } else {
            "messages"
        };
        let mut req = json!({});
        req[msgs_key] = json!([{"role": "user", "content": "hi"}]);
        if d == Dialect::Claude {
            req["max_tokens"] = json!(1024);
        }
        let r = v.validate_pre_mapping(d, &req);
        assert!(!r.is_valid(), "Should fail for {:?} without model", d);
        assert!(r.issues.iter().any(|i| i.field == "model"));
    }
}

#[test]
fn pre_mapping_non_object_returns_invalid() {
    let v = validator();
    let r = v.validate_pre_mapping(Dialect::OpenAi, &json!(42));
    assert!(!r.is_valid());
    assert_eq!(r.field_coverage, 0.0);
    assert!(r.issues[0].code == "invalid_type");
}

#[test]
fn pre_mapping_claude_missing_max_tokens() {
    let v = validator();
    let req = json!({"model": "claude-3", "messages": []});
    let r = v.validate_pre_mapping(Dialect::Claude, &req);
    assert!(!r.is_valid());
    assert!(
        r.issues
            .iter()
            .any(|i| i.field == "max_tokens" && i.code == "missing_required_field")
    );
}

#[test]
fn pre_mapping_empty_messages_warning() {
    let v = validator();
    let req = json!({"model": "gpt-4", "messages": []});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(r.is_valid()); // warnings don't invalidate
    assert!(r.warning_count() > 0);
    assert!(r.issues.iter().any(|i| i.code == "empty_messages"));
}

#[test]
fn pre_mapping_gemini_empty_contents_warning() {
    let v = validator();
    let req = json!({"model": "gemini-pro", "contents": []});
    let r = v.validate_pre_mapping(Dialect::Gemini, &req);
    assert!(r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "empty_messages"));
}

#[test]
fn pre_mapping_partial_coverage() {
    let v = validator();
    // OpenAI requires model + messages = 2 fields; provide only model
    let req = json!({"model": "gpt-4"});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(!r.is_valid());
    assert_eq!(r.field_coverage, 50.0);
}

#[test]
fn pre_mapping_claude_partial_coverage() {
    let v = validator();
    // Claude requires model, messages, max_tokens = 3 fields; provide 1
    let req = json!({"model": "claude-3"});
    let r = v.validate_pre_mapping(Dialect::Claude, &req);
    assert!(!r.is_valid());
    let expected = (1.0 / 3.0) * 100.0;
    assert!((r.field_coverage - expected).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Validation: post-mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn post_mapping_valid_openai() {
    let v = validator();
    let mapped = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate_post_mapping(Dialect::OpenAi, &mapped);
    assert!(r.is_valid());
}

#[test]
fn post_mapping_valid_claude() {
    let v = validator();
    let mapped = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1024
    });
    let r = v.validate_post_mapping(Dialect::Claude, &mapped);
    assert!(r.is_valid());
}

#[test]
fn post_mapping_missing_field_invalid() {
    let v = validator();
    let mapped = json!({"model": "gpt-4"});
    let r = v.validate_post_mapping(Dialect::OpenAi, &mapped);
    assert!(!r.is_valid());
    assert!(r.error_count() > 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Validation: roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_identical_values_are_lossless() {
    let v = validator();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate_roundtrip(&val, &val);
    assert!(r.is_lossless());
    assert!(r.lost_fields.is_empty());
    assert!(r.added_fields.is_empty());
    assert!(r.changed_fields.is_empty());
}

#[test]
fn roundtrip_detects_lost_fields() {
    let v = validator();
    let orig = json!({"model": "gpt-4", "temperature": 0.7, "top_p": 0.9});
    let roundtripped = json!({"model": "gpt-4"});
    let r = v.validate_roundtrip(&orig, &roundtripped);
    assert!(!r.is_lossless());
    assert!(r.lost_fields.contains(&"temperature".to_string()));
    assert!(r.lost_fields.contains(&"top_p".to_string()));
}

#[test]
fn roundtrip_detects_added_fields() {
    let v = validator();
    let orig = json!({"model": "gpt-4"});
    let roundtripped = json!({"model": "gpt-4", "extra": true, "another": 42});
    let r = v.validate_roundtrip(&orig, &roundtripped);
    assert!(!r.is_lossless());
    assert!(r.added_fields.contains(&"extra".to_string()));
    assert!(r.added_fields.contains(&"another".to_string()));
}

#[test]
fn roundtrip_detects_changed_fields() {
    let v = validator();
    let orig = json!({"model": "gpt-4", "temperature": 0.7});
    let roundtripped = json!({"model": "gpt-4", "temperature": 0.5});
    let r = v.validate_roundtrip(&orig, &roundtripped);
    assert!(!r.is_lossless());
    assert!(r.changed_fields.contains(&"temperature".to_string()));
}

#[test]
fn roundtrip_nested_objects() {
    let v = validator();
    let orig = json!({"a": {"b": 1, "c": 2}});
    let roundtripped = json!({"a": {"b": 1, "c": 3}});
    let r = v.validate_roundtrip(&orig, &roundtripped);
    assert!(!r.is_lossless());
    assert!(r.changed_fields.contains(&"a.c".to_string()));
}

#[test]
fn roundtrip_array_elements() {
    let v = validator();
    let orig = json!({"items": [1, 2, 3]});
    let roundtripped = json!({"items": [1, 2, 4]});
    let r = v.validate_roundtrip(&orig, &roundtripped);
    assert!(!r.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. ValidationSeverity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_severity_display() {
    assert_eq!(ValidationSeverity::Info.to_string(), "info");
    assert_eq!(ValidationSeverity::Warning.to_string(), "warning");
    assert_eq!(ValidationSeverity::Error.to_string(), "error");
}

#[test]
fn validation_severity_serde_roundtrip() {
    for sev in [
        ValidationSeverity::Info,
        ValidationSeverity::Warning,
        ValidationSeverity::Error,
    ] {
        let json_str = serde_json::to_string(&sev).unwrap();
        let back: ValidationSeverity = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, sev);
    }
}

#[test]
fn validation_severity_ordering() {
    assert!(ValidationSeverity::Info < ValidationSeverity::Warning);
    assert!(ValidationSeverity::Warning < ValidationSeverity::Error);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. ValidationIssue
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_issue_display_format() {
    let issue = ValidationIssue {
        severity: ValidationSeverity::Error,
        field: "model".into(),
        message: "missing required field".into(),
        code: "missing_required_field".into(),
    };
    let s = format!("{issue}");
    assert!(s.contains("[error]"));
    assert!(s.contains("model"));
    assert!(s.contains("missing required field"));
    assert!(s.contains("missing_required_field"));
}

#[test]
fn validation_issue_serde_roundtrip() {
    let issue = ValidationIssue {
        severity: ValidationSeverity::Warning,
        field: "messages".into(),
        message: "empty array".into(),
        code: "empty_messages".into(),
    };
    let json_str = serde_json::to_string(&issue).unwrap();
    let back: ValidationIssue = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, issue);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. ValidationPipeline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_full_pass_openai_to_openai() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::OpenAi,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = pipe.run(&req, |v| Ok(v.clone()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    assert!(result.post.as_ref().unwrap().is_valid());
}

#[test]
fn pipeline_pre_fail_skips_mapping() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::OpenAi,
    );
    let req = json!({"not_model": true});
    let result = pipe.run(&req, |_| panic!("should not be called"));
    assert!(!result.pre.is_valid());
    assert!(result.mapped.is_none());
    assert!(result.post.is_none());
}

#[test]
fn pipeline_map_error_produces_post_error() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::Claude,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = pipe.run(&req, |_| Err("mapping failed".into()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_none());
    let post = result.post.unwrap();
    assert!(!post.is_valid());
    assert_eq!(post.issues[0].code, "mapping_failed");
}

#[test]
fn pipeline_accessor_returns_correct_dialects() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::Claude,
    );
    let _v = pipe.validator();
    assert_eq!(pipe.source, Dialect::OpenAi);
    assert_eq!(pipe.target, Dialect::Claude);
}

#[test]
fn pipeline_openai_to_claude_with_real_mapper() {
    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::Claude,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = pipe.run(&req, |v| {
        let mapper = OpenAiToClaudeMapper;
        let dialect_req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: v.clone(),
        };
        mapper.map_request(&dialect_req).map_err(|e| e.to_string())
    });
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    let post = result.post.unwrap();
    assert!(post.is_valid());
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. IR mapping: simple conversations through all supported pairs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapping_simple_conversation_all_supported_pairs() {
    let ir = simple_ir_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &ir);
        assert!(
            result.is_ok(),
            "map_request failed for {:?} -> {:?}: {:?}",
            from,
            to,
            result.err()
        );
    }
}

#[test]
fn ir_mapping_simple_response_all_supported_pairs() {
    let ir = simple_ir_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_response(from, to, &ir);
        assert!(
            result.is_ok(),
            "map_response failed for {:?} -> {:?}: {:?}",
            from,
            to,
            result.err()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. IR mapping: tool calls
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapping_tool_calls_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = tool_call_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // Tool-role message should become user-role
    let tool_msg = result
        .messages
        .iter()
        .find(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .unwrap();
    assert_eq!(tool_msg.role, IrRole::User);
}

#[test]
fn ir_mapping_tool_calls_claude_to_openai() {
    let mapper = OpenAiClaudeIrMapper;
    // Claude-style: user message with ToolResult block
    let ir = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "query".into(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        ),
    ]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    // User messages with only ToolResult should become Tool-role
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert!(!tool_msgs.is_empty());
}

#[test]
fn ir_mapping_tool_calls_openai_to_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = tool_call_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    // Tool-role should become User-role for Gemini
    let has_tool_role = result.messages.iter().any(|m| m.role == IrRole::Tool);
    assert!(!has_tool_role);
}

#[test]
fn ir_mapping_tool_calls_preserved_in_kimi() {
    let mapper = OpenAiKimiIrMapper;
    let ir = tool_call_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &ir)
        .unwrap();
    // Tool calls should be preserved (near-identity)
    assert!(result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { .. }))
    }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. IR mapping: thinking blocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_blocks_dropped_claude_to_openai() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(
        !has_thinking,
        "Thinking blocks should be dropped for OpenAI"
    );
}

#[test]
fn thinking_blocks_preserved_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    // Claude supports thinking blocks, so they should survive
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(
        has_thinking,
        "Thinking blocks should be preserved for Claude"
    );
}

#[test]
fn thinking_blocks_dropped_openai_to_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking);
}

#[test]
fn thinking_blocks_dropped_claude_to_gemini() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking);
}

#[test]
fn thinking_blocks_dropped_openai_to_kimi() {
    let mapper = OpenAiKimiIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking);
}

#[test]
fn thinking_blocks_dropped_claude_to_kimi() {
    let mapper = ClaudeKimiIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking);
}

#[test]
fn thinking_blocks_dropped_openai_to_copilot() {
    let mapper = OpenAiCopilotIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Copilot, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking);
}

#[test]
fn thinking_blocks_dropped_gemini_to_kimi() {
    let mapper = GeminiKimiIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. IR mapping: Codex lossy conversions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_drops_system_messages() {
    let mapper = OpenAiCodexIrMapper;
    let ir = simple_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    let has_system = result.messages.iter().any(|m| m.role == IrRole::System);
    assert!(!has_system, "Codex should drop system messages");
}

#[test]
fn codex_drops_tool_messages() {
    let mapper = OpenAiCodexIrMapper;
    let ir = tool_call_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    let has_tool = result.messages.iter().any(|m| m.role == IrRole::Tool);
    assert!(!has_tool, "Codex should drop tool-role messages");
}

#[test]
fn codex_drops_tool_use_blocks() {
    let mapper = OpenAiCodexIrMapper;
    let ir = tool_call_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    let has_tool_use = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { .. }))
    });
    assert!(!has_tool_use, "Codex should drop ToolUse blocks");
}

#[test]
fn codex_drops_thinking_blocks() {
    let mapper = OpenAiCodexIrMapper;
    let ir = thinking_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking, "Codex should drop thinking blocks");
}

#[test]
fn codex_drops_image_blocks() {
    let mapper = OpenAiCodexIrMapper;
    let ir = image_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    let has_image = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    });
    assert!(!has_image, "Codex should drop image blocks");
}

#[test]
fn codex_preserves_text_blocks() {
    let mapper = OpenAiCodexIrMapper;
    let ir = simple_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    // Should have user and assistant text messages
    assert!(result.messages.iter().any(|m| m.role == IrRole::User));
    assert!(result.messages.iter().any(|m| m.role == IrRole::Assistant));
}

#[test]
fn codex_to_openai_is_lossless() {
    let mapper = OpenAiCodexIrMapper;
    let ir = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Text { text: "Hi".into() }],
        ),
    ]);
    let result = mapper
        .map_request(Dialect::Codex, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.messages.len(), ir.messages.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. IR mapping: Codex-Claude lossy conversions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_claude_drops_system_for_codex() {
    let mapper = CodexClaudeIrMapper;
    let ir = simple_ir_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Codex, &ir)
        .unwrap();
    let has_system = result.messages.iter().any(|m| m.role == IrRole::System);
    assert!(!has_system);
}

#[test]
fn codex_claude_rejects_unmappable_tools() {
    let mapper = CodexClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "apply_patch".into(),
            input: json!({"patch": "..."}),
        }],
    )]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { .. }));
    assert!(err.to_string().contains("apply_patch"));
}

#[test]
fn codex_claude_rejects_apply_diff_tool() {
    let mapper = CodexClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "apply_diff".into(),
            input: json!({"diff": "..."}),
        }],
    )]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { .. }));
}

#[test]
fn codex_claude_allows_safe_tools() {
    let mapper = CodexClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "src/main.rs"}),
        }],
    )]);
    let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &ir);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. IR mapping: Claude-Gemini system prompt with image error
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_gemini_rejects_system_with_image() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "You are helpful.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    )]);
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableContent { .. }));
    assert!(err.to_string().contains("system"));
}

#[test]
fn claude_gemini_allows_system_text_only() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "You are helpful.".into(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text { text: "Hi".into() }],
        ),
    ]);
    let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &ir);
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Complex IR content mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complex_ir_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = complex_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert!(!result.messages.is_empty());
    // Claude should preserve thinking blocks
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(has_thinking);
}

#[test]
fn complex_ir_openai_to_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let ir = complex_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &ir)
        .unwrap();
    assert!(!result.messages.is_empty());
    // Gemini should drop thinking blocks
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking);
}

#[test]
fn complex_ir_openai_to_codex_drops_everything_except_text() {
    let mapper = OpenAiCodexIrMapper;
    let ir = complex_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    // Should only have text blocks
    for msg in &result.messages {
        for block in &msg.content {
            assert!(matches!(block, IrContentBlock::Text { .. }));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Mapping determinism (same input → same output)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapping_deterministic_openai_to_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = tool_call_ir_conversation();
    let r1 = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let r2 = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(r1.messages.len(), r2.messages.len());
    for (m1, m2) in r1.messages.iter().zip(r2.messages.iter()) {
        assert_eq!(m1.role, m2.role);
        assert_eq!(m1.content.len(), m2.content.len());
    }
}

#[test]
fn ir_mapping_deterministic_all_supported_pairs() {
    let ir = simple_ir_conversation();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let r1 = mapper.map_request(from, to, &ir).unwrap();
        let r2 = mapper.map_request(from, to, &ir).unwrap();
        assert_eq!(
            r1.messages.len(),
            r2.messages.len(),
            "Non-deterministic for {:?} -> {:?}",
            from,
            to
        );
    }
}

#[test]
fn json_mapper_deterministic_openai_to_claude() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 1024,
            "temperature": 0.7
        }),
    };
    let r1 = mapper.map_request(&req).unwrap();
    let r2 = mapper.map_request(&req).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn json_mapper_deterministic_openai_to_gemini() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 512
        }),
    };
    let r1 = mapper.map_request(&req).unwrap();
    let r2 = mapper.map_request(&req).unwrap();
    assert_eq!(r1, r2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. JSON-level Mapper trait: IdentityMapper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn identity_mapper_passthrough_request() {
    let mapper = IdentityMapper;
    let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: body.clone(),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result, body);
}

#[test]
fn identity_mapper_passthrough_response() {
    let mapper = IdentityMapper;
    let body = json!({"choices": [{"message": {"content": "hello"}}]});
    let resp = mapper.map_response(&body).unwrap();
    assert_eq!(resp.body, body);
    assert_eq!(resp.dialect, Dialect::OpenAi);
}

#[test]
fn identity_mapper_passthrough_event() {
    let mapper = IdentityMapper;
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let result = mapper.map_event(&event).unwrap();
    assert!(result.get("type").is_some());
}

#[test]
fn identity_mapper_works_for_all_dialects() {
    let mapper = IdentityMapper;
    for &d in Dialect::all() {
        let req = DialectRequest {
            dialect: d,
            body: json!({"test": true}),
        };
        let result = mapper.map_request(&req).unwrap();
        assert_eq!(result, json!({"test": true}));
    }
}

#[test]
fn identity_mapper_source_target_dialects() {
    let mapper = IdentityMapper;
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. JSON-level Mapper: OpenAiToClaudeMapper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_basic_mapping() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["model"], "gpt-4");
    assert_eq!(result["max_tokens"], 1024);
}

#[test]
fn openai_to_claude_extracts_system_prompt() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hi"}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["system"], "You are helpful.");
    assert_eq!(result["messages"].as_array().unwrap().len(), 1);
}

#[test]
fn openai_to_claude_default_max_tokens() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["max_tokens"], 4096);
}

#[test]
fn openai_to_claude_wrong_dialect_rejected() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::Claude,
        body: json!({"model": "claude-3"}),
    };
    let err = mapper.map_request(&req).unwrap_err();
    assert!(matches!(err, MappingError::UnmappableRequest { .. }));
}

#[test]
fn openai_to_claude_non_object_rejected() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!("not an object"),
    };
    let err = mapper.map_request(&req).unwrap_err();
    assert!(matches!(err, MappingError::UnmappableRequest { .. }));
}

#[test]
fn openai_to_claude_maps_tools() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {"loc": {"type": "string"}}}
                }
            }]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools[0]["name"], "get_weather");
    assert!(tools[0].get("input_schema").is_some());
}

#[test]
fn openai_to_claude_maps_stop_string() {
    let mapper = OpenAiToClaudeMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": "END"
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["stop_sequences"], json!(["END"]));
}

#[test]
fn openai_to_claude_source_target() {
    let mapper = OpenAiToClaudeMapper;
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. JSON-level Mapper: OpenAiToGeminiMapper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_gemini_basic_mapping() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["contents"][0]["role"], "user");
    assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
}

#[test]
fn openai_to_gemini_system_instruction() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be helpful."},
                {"role": "user", "content": "Hi"}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(
        result["system_instruction"]["parts"][0]["text"],
        "Be helpful."
    );
}

#[test]
fn openai_to_gemini_generation_config() {
    let mapper = OpenAiToGeminiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1024,
            "temperature": 0.7,
            "top_p": 0.9
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["generationConfig"]["maxOutputTokens"], 1024);
    assert_eq!(result["generationConfig"]["temperature"], 0.7);
    assert_eq!(result["generationConfig"]["topP"], 0.9);
}

#[test]
fn openai_to_gemini_source_target() {
    let mapper = OpenAiToGeminiMapper;
    assert_eq!(mapper.source_dialect(), Dialect::OpenAi);
    assert_eq!(mapper.target_dialect(), Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. JSON-level Mapper: GeminiToOpenAiMapper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_to_openai_basic_mapping() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["messages"][0]["content"], "Hello");
}

#[test]
fn gemini_to_openai_model_becomes_assistant() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({
            "model": "gemini-pro",
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]},
                {"role": "model", "parts": [{"text": "Hello!"}]}
            ]
        }),
    };
    let result = mapper.map_request(&req).unwrap();
    assert_eq!(result["messages"][1]["role"], "assistant");
}

#[test]
fn gemini_to_openai_wrong_dialect_rejected() {
    let mapper = GeminiToOpenAiMapper;
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4"}),
    };
    let err = mapper.map_request(&req).unwrap_err();
    assert!(matches!(err, MappingError::UnmappableRequest { .. }));
}

#[test]
fn gemini_to_openai_source_target() {
    let mapper = GeminiToOpenAiMapper;
    assert_eq!(mapper.source_dialect(), Dialect::Gemini);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. JSON-level Mapper: ClaudeToOpenAiMapper (skipped if not public)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_to_openai_source_target() {
    let mapper = ClaudeToOpenAiMapper;
    assert_eq!(mapper.source_dialect(), Dialect::Claude);
    assert_eq!(mapper.target_dialect(), Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════════
// 26. IrIdentityMapper
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_identity_preserves_conversation_exactly() {
    let mapper = IrIdentityMapper;
    let ir = complex_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.messages.len(), ir.messages.len());
    for (orig, mapped) in ir.messages.iter().zip(result.messages.iter()) {
        assert_eq!(orig.role, mapped.role);
        assert_eq!(orig.content.len(), mapped.content.len());
    }
}

#[test]
fn ir_identity_preserves_response_exactly() {
    let mapper = IrIdentityMapper;
    let ir = tool_call_ir_conversation();
    let result = mapper
        .map_response(Dialect::Claude, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(result.messages.len(), ir.messages.len());
}

#[test]
fn ir_identity_works_for_all_dialects() {
    let mapper = IrIdentityMapper;
    let ir = simple_ir_conversation();
    for &d in Dialect::all() {
        let r = mapper.map_request(d, d, &ir).unwrap();
        assert_eq!(r.messages.len(), ir.messages.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 27. Empty conversation edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_conversation_maps_through_all_pairs() {
    let ir = IrConversation::from_messages(vec![]);
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &ir);
        assert!(
            result.is_ok(),
            "Empty conversation failed for {:?} -> {:?}",
            from,
            to
        );
        assert_eq!(result.unwrap().messages.len(), 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 28. Mixed content user messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mixed_tool_result_and_text_claude_to_openai() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Here's context.".into(),
            },
            IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &ir)
        .unwrap();
    // Should split: user text + tool role
    let user_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::User)
        .collect();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert!(!user_msgs.is_empty());
    assert!(!tool_msgs.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 29. Gemini-Kimi tool result role conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_to_kimi_converts_user_tool_results_to_tool_role() {
    let mapper = GeminiKimiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: vec![IrContentBlock::Text { text: "42".into() }],
            is_error: false,
        }],
    )]);
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &ir)
        .unwrap();
    assert!(result.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn kimi_to_gemini_converts_tool_role_to_user() {
    let mapper = GeminiKimiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: vec![IrContentBlock::Text { text: "42".into() }],
            is_error: false,
        }],
    )]);
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::Gemini, &ir)
        .unwrap();
    assert!(!result.messages.iter().any(|m| m.role == IrRole::Tool));
    assert!(result.messages.iter().any(|m| m.role == IrRole::User));
}

// ═══════════════════════════════════════════════════════════════════════════
// 30. Claude-Kimi tool result role conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_to_kimi_converts_user_tool_results_to_tool_role() {
    let mapper = ClaudeKimiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: vec![IrContentBlock::Text { text: "42".into() }],
            is_error: false,
        }],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &ir)
        .unwrap();
    assert!(result.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn kimi_to_claude_converts_tool_role_to_user() {
    let mapper = ClaudeKimiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: vec![IrContentBlock::Text { text: "42".into() }],
            is_error: false,
        }],
    )]);
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::Claude, &ir)
        .unwrap();
    assert!(!result.messages.iter().any(|m| m.role == IrRole::Tool));
    assert!(result.messages.iter().any(|m| m.role == IrRole::User));
}

// ═══════════════════════════════════════════════════════════════════════════
// 31. Gemini-Claude tool role conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_to_claude_converts_tool_role_to_user() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: vec![IrContentBlock::Text { text: "42".into() }],
            is_error: false,
        }],
    )]);
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &ir)
        .unwrap();
    assert!(result.messages.iter().any(|m| m.role == IrRole::User));
    assert!(!result.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn claude_to_gemini_converts_tool_role_to_user() {
    let mapper = ClaudeGeminiIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: vec![IrContentBlock::Text { text: "42".into() }],
            is_error: false,
        }],
    )]);
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &ir)
        .unwrap();
    assert!(result.messages.iter().any(|m| m.role == IrRole::User));
}

// ═══════════════════════════════════════════════════════════════════════════
// 32. DialectRequest / DialectResponse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_request_debug() {
    let req = DialectRequest {
        dialect: Dialect::OpenAi,
        body: json!({"model": "gpt-4"}),
    };
    let dbg = format!("{req:?}");
    assert!(dbg.contains("OpenAi"));
}

#[test]
fn dialect_request_clone() {
    let req = DialectRequest {
        dialect: Dialect::Gemini,
        body: json!({"contents": []}),
    };
    let cloned = req.clone();
    assert_eq!(cloned.body, req.body);
}

#[test]
fn dialect_response_debug() {
    let resp = DialectResponse {
        dialect: Dialect::Claude,
        body: json!({"content": []}),
    };
    let dbg = format!("{resp:?}");
    assert!(dbg.contains("Claude"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 33. ValidationResult helper methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_result_error_count() {
    let v = validator();
    let req = json!({}); // missing everything
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert_eq!(r.error_count(), 2); // model + messages
}

#[test]
fn validation_result_warning_count() {
    let v = validator();
    let req = json!({"model": "gpt-4", "messages": []});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert_eq!(r.warning_count(), 1); // empty messages
}

// ═══════════════════════════════════════════════════════════════════════════
// 34. DefaultMappingValidator new()
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_mapping_validator_new() {
    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(r.is_valid());
}

#[test]
fn default_mapping_validator_default_trait() {
    let v = DefaultMappingValidator::default();
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(r.is_valid());
}

#[test]
fn default_mapping_validator_serde_roundtrip() {
    let v = DefaultMappingValidator::new();
    let json_str = serde_json::to_string(&v).unwrap();
    let _back: DefaultMappingValidator = serde_json::from_str(&json_str).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// 35. map_request vs map_response symmetry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapper_request_response_same_behavior_for_identity() {
    let mapper = IrIdentityMapper;
    let ir = simple_ir_conversation();
    let req_result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    let resp_result = mapper
        .map_response(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(req_result.messages.len(), resp_result.messages.len());
}

#[test]
fn ir_mapper_request_response_symmetry_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = simple_ir_conversation();
    let req_result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let resp_result = mapper
        .map_response(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    assert_eq!(req_result.messages.len(), resp_result.messages.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 36. Comprehensive error message content
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_error_unsupported_pair_contains_dialect_names() {
    let err = MapError::UnsupportedPair {
        from: Dialect::OpenAi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("OpenAI"));
    assert!(msg.contains("Copilot"));
}

#[test]
fn mapping_error_fidelity_loss_contains_all_fields() {
    let err = MappingError::FidelityLoss {
        field: "system_prompt".into(),
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        detail: "flattened to single string".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("system_prompt"));
    assert!(msg.contains("OpenAI"));
    assert!(msg.contains("Gemini"));
    assert!(msg.contains("flattened"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 37. Mapping with error tool results
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapping_error_tool_result_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: vec![IrContentBlock::Text {
                text: "Error: file not found".into(),
            }],
            is_error: true,
        }],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &ir)
        .unwrap();
    let has_error_result = result.messages.iter().any(|m| {
        m.content.iter().any(|b| match b {
            IrContentBlock::ToolResult { is_error, .. } => *is_error,
            _ => false,
        })
    });
    assert!(has_error_result);
}

// ═══════════════════════════════════════════════════════════════════════════
// 38. IrMapper is Send + Sync (object safety)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_mapper_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IrIdentityMapper>();
    assert_send_sync::<OpenAiClaudeIrMapper>();
    assert_send_sync::<OpenAiGeminiIrMapper>();
    assert_send_sync::<ClaudeGeminiIrMapper>();
    assert_send_sync::<OpenAiCodexIrMapper>();
    assert_send_sync::<OpenAiKimiIrMapper>();
    assert_send_sync::<ClaudeKimiIrMapper>();
    assert_send_sync::<OpenAiCopilotIrMapper>();
    assert_send_sync::<GeminiKimiIrMapper>();
    assert_send_sync::<CodexClaudeIrMapper>();
}

#[test]
fn ir_mapper_trait_object_usable() {
    let mapper: Box<dyn IrMapper> = Box::new(IrIdentityMapper);
    let ir = simple_ir_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &ir)
        .unwrap();
    assert_eq!(result.messages.len(), ir.messages.len());
}

// ═══════════════════════════════════════════════════════════════════════════
// 39. Mapper trait is Send + Sync
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapper_trait_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IdentityMapper>();
    assert_send_sync::<OpenAiToClaudeMapper>();
    assert_send_sync::<OpenAiToGeminiMapper>();
    assert_send_sync::<GeminiToOpenAiMapper>();
    assert_send_sync::<ClaudeToOpenAiMapper>();
}

// ═══════════════════════════════════════════════════════════════════════════
// 40. Validation for all dialect required fields
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn validation_gemini_requires_model_and_contents() {
    let v = validator();
    let req = json!({});
    let r = v.validate_pre_mapping(Dialect::Gemini, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "model"));
    assert!(r.issues.iter().any(|i| i.field == "contents"));
}

#[test]
fn validation_kimi_requires_model_and_messages() {
    let v = validator();
    let req = json!({});
    let r = v.validate_pre_mapping(Dialect::Kimi, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "model"));
    assert!(r.issues.iter().any(|i| i.field == "messages"));
}

#[test]
fn validation_copilot_requires_model_and_messages() {
    let v = validator();
    let req = json!({});
    let r = v.validate_pre_mapping(Dialect::Copilot, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "model"));
    assert!(r.issues.iter().any(|i| i.field == "messages"));
}

#[test]
fn validation_codex_requires_model_and_messages() {
    let v = validator();
    let req = json!({});
    let r = v.validate_pre_mapping(Dialect::Codex, &req);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "model"));
    assert!(r.issues.iter().any(|i| i.field == "messages"));
}
