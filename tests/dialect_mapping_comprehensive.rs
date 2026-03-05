#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

//! Comprehensive dialect x engine mapping tests for the Agent Backplane.
//!
//! Covers all 6 dialects (OpenAI, Claude, Gemini, Codex, Kimi, Copilot),
//! cross-dialect IR mapping, passthrough mode, mapped mode, capability negotiation,
//! emulation labeling, early failure for unmappable requests, fidelity levels,
//! model name mapping, and tool definition mapping.

use abp_capability::{
    CapabilityRegistry, NegotiationResult, SupportLevel, check_capability,
    claude_35_sonnet_manifest, codex_manifest, copilot_manifest, gemini_15_pro_manifest,
    kimi_manifest, negotiate, negotiate_capabilities, openai_gpt4o_manifest,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    Capability, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel, WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy,
    FidelityLabel, apply_emulation, can_emulate, compute_fidelity, default_strategy,
};
use abp_ir::lower::{ir_role_to_dialect, lower_to_claude, lower_to_gemini, lower_to_openai};
use abp_ir::normalize;
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, DialectRequest, DialectResponse,
    GeminiKimiIrMapper, IdentityMapper, IrIdentityMapper, IrMapper, MapError, Mapper, MappingError,
    OpenAiClaudeIrMapper, OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper,
    OpenAiKimiIrMapper, default_ir_mapper, supported_ir_pairs,
};
use abp_mapping::{
    Fidelity, MappingMatrix, MappingRegistry, MappingRule, features, known_rules, validate_mapping,
};
use abp_projection::{
    BackendEntry, CompatibilityScore, DialectPair, ProjectionEntry, ProjectionError,
    ProjectionMatrix, ProjectionMode, ProjectionScore, RoutingHop, RoutingPath,
};
use serde_json::json;
use std::collections::BTreeMap;

// ============================================================================
// Helper functions
// ============================================================================

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

fn simple_conversation() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Hello, world!"))
}

fn conversation_with_tool_use() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Read the file."))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ))
}

fn sample_tool_definitions() -> Vec<IrToolDefinition> {
    vec![
        IrToolDefinition {
            name: "read_file".into(),
            description: "Read the contents of a file".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"}
                },
                "required": ["path"]
            }),
        },
        IrToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        },
    ]
}

fn manifest_for_dialect(dialect: Dialect) -> BTreeMap<Capability, CoreSupportLevel> {
    match dialect {
        Dialect::OpenAi => openai_gpt4o_manifest(),
        Dialect::Claude => claude_35_sonnet_manifest(),
        Dialect::Gemini => gemini_15_pro_manifest(),
        Dialect::Kimi => kimi_manifest(),
        Dialect::Codex => codex_manifest(),
        Dialect::Copilot => copilot_manifest(),
    }
}

// ============================================================================
// Module 1: Dialect enum basics
// ============================================================================
mod dialect_enum {
    use super::*;

    #[test]
    fn all_dialects_returns_six() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn dialect_labels_are_non_empty() {
        for &d in Dialect::all() {
            assert!(!d.label().is_empty());
        }
    }

    #[test]
    fn dialect_display_matches_label() {
        for &d in Dialect::all() {
            assert_eq!(format!("{d}"), d.label());
        }
    }

    #[test]
    fn dialect_equality() {
        assert_eq!(Dialect::OpenAi, Dialect::OpenAi);
        assert_ne!(Dialect::OpenAi, Dialect::Claude);
    }

    #[test]
    fn dialect_clone() {
        let d = Dialect::Gemini;
        let cloned = d;
        assert_eq!(d, cloned);
    }

    #[test]
    fn dialect_serde_roundtrip() {
        for &d in Dialect::all() {
            let json = serde_json::to_string(&d).unwrap();
            let back: Dialect = serde_json::from_str(&json).unwrap();
            assert_eq!(d, back);
        }
    }
}

// ============================================================================
// Module 2: IR mapper factory - all supported pairs
// ============================================================================
mod ir_mapper_factory {
    use super::*;

    #[test]
    fn identity_mappers_for_all_six_dialects() {
        for &d in all_dialects() {
            let mapper = default_ir_mapper(d, d);
            assert!(mapper.is_some(), "Identity mapper missing for {d}");
        }
    }

    #[test]
    fn supported_pairs_includes_all_identity() {
        let pairs = supported_ir_pairs();
        for &d in all_dialects() {
            assert!(pairs.contains(&(d, d)), "Missing identity pair for {d}");
        }
    }

    #[test]
    fn openai_claude_mapper_exists() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
        assert!(default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
    }

    #[test]
    fn openai_gemini_mapper_exists() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
        assert!(default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).is_some());
    }

    #[test]
    fn claude_gemini_mapper_exists() {
        assert!(default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
        assert!(default_ir_mapper(Dialect::Gemini, Dialect::Claude).is_some());
    }

    #[test]
    fn openai_codex_mapper_exists() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Codex).is_some());
        assert!(default_ir_mapper(Dialect::Codex, Dialect::OpenAi).is_some());
    }

    #[test]
    fn openai_kimi_mapper_exists() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).is_some());
        assert!(default_ir_mapper(Dialect::Kimi, Dialect::OpenAi).is_some());
    }

    #[test]
    fn claude_kimi_mapper_exists() {
        assert!(default_ir_mapper(Dialect::Claude, Dialect::Kimi).is_some());
        assert!(default_ir_mapper(Dialect::Kimi, Dialect::Claude).is_some());
    }

    #[test]
    fn openai_copilot_mapper_exists() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).is_some());
        assert!(default_ir_mapper(Dialect::Copilot, Dialect::OpenAi).is_some());
    }

    #[test]
    fn gemini_kimi_mapper_exists() {
        assert!(default_ir_mapper(Dialect::Gemini, Dialect::Kimi).is_some());
        assert!(default_ir_mapper(Dialect::Kimi, Dialect::Gemini).is_some());
    }

    #[test]
    fn codex_claude_mapper_exists() {
        assert!(default_ir_mapper(Dialect::Codex, Dialect::Claude).is_some());
        assert!(default_ir_mapper(Dialect::Claude, Dialect::Codex).is_some());
    }

    #[test]
    fn unsupported_pair_returns_none() {
        assert!(default_ir_mapper(Dialect::Copilot, Dialect::Codex).is_none());
        assert!(default_ir_mapper(Dialect::Codex, Dialect::Copilot).is_none());
    }

    #[test]
    fn copilot_kimi_returns_none() {
        assert!(default_ir_mapper(Dialect::Copilot, Dialect::Kimi).is_none());
        assert!(default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
    }

    #[test]
    fn copilot_gemini_returns_none() {
        assert!(default_ir_mapper(Dialect::Copilot, Dialect::Gemini).is_none());
        assert!(default_ir_mapper(Dialect::Gemini, Dialect::Copilot).is_none());
    }

    #[test]
    fn copilot_claude_returns_none() {
        assert!(default_ir_mapper(Dialect::Copilot, Dialect::Claude).is_none());
        assert!(default_ir_mapper(Dialect::Claude, Dialect::Copilot).is_none());
    }

    #[test]
    fn codex_kimi_returns_none() {
        assert!(default_ir_mapper(Dialect::Codex, Dialect::Kimi).is_none());
        assert!(default_ir_mapper(Dialect::Kimi, Dialect::Codex).is_none());
    }

    #[test]
    fn codex_gemini_returns_none() {
        assert!(default_ir_mapper(Dialect::Codex, Dialect::Gemini).is_none());
        assert!(default_ir_mapper(Dialect::Gemini, Dialect::Codex).is_none());
    }
}

// ============================================================================
// Module 3: Passthrough mode - same dialect preserves all fields
// ============================================================================
mod passthrough {
    use super::*;

    #[test]
    fn identity_ir_mapper_preserves_simple_conversation() {
        let conv = simple_conversation();
        let mapper = IrIdentityMapper;
        for &d in all_dialects() {
            let result = mapper.map_request(d, d, &conv).unwrap();
            assert_eq!(result, conv);
        }
    }

    #[test]
    fn identity_ir_mapper_preserves_tool_conversation() {
        let conv = conversation_with_tool_use();
        let mapper = IrIdentityMapper;
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn identity_mapper_preserves_request_body() {
        let mapper = IdentityMapper;
        let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        for &d in all_dialects() {
            let req = DialectRequest {
                dialect: d,
                body: body.clone(),
            };
            let result = mapper.map_request(&req).unwrap();
            assert_eq!(result, body);
        }
    }

    #[test]
    fn identity_mapper_preserves_response_body() {
        let mapper = IdentityMapper;
        let body = json!({"id": "resp-1", "choices": [{"message": {"content": "hello"}}]});
        let resp = mapper.map_response(&body).unwrap();
        assert_eq!(resp.body, body);
    }

    #[test]
    fn identity_ir_mapper_response_preserves_all_fields() {
        let conv = simple_conversation();
        let mapper = IrIdentityMapper;
        for &d in all_dialects() {
            let result = mapper.map_response(d, d, &conv).unwrap();
            assert_eq!(result.messages.len(), conv.messages.len());
            for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
                assert_eq!(orig.role, mapped.role);
                assert_eq!(orig.text_content(), mapped.text_content());
            }
        }
    }

    #[test]
    fn identity_ir_mapper_supported_pairs_covers_all_identity() {
        let mapper = IrIdentityMapper;
        let pairs = mapper.supported_pairs();
        for &d in all_dialects() {
            assert!(pairs.contains(&(d, d)));
        }
    }
}

// ============================================================================
// Module 4: Cross-dialect IR mapping (all supported pairs)
// ============================================================================
mod cross_dialect_ir {
    use super::*;

    fn assert_ir_mapping_works(from: Dialect, to: Dialect) {
        let mapper =
            default_ir_mapper(from, to).unwrap_or_else(|| panic!("No mapper for {from} -> {to}"));
        let conv = simple_conversation();
        let result = mapper.map_request(from, to, &conv);
        assert!(
            result.is_ok(),
            "map_request failed for {from} -> {to}: {:?}",
            result.err()
        );
        let mapped = result.unwrap();
        assert!(
            !mapped.messages.is_empty(),
            "Empty result for {from} -> {to}"
        );
    }

    #[test]
    fn openai_to_claude_ir() {
        assert_ir_mapping_works(Dialect::OpenAi, Dialect::Claude);
    }
    #[test]
    fn claude_to_openai_ir() {
        assert_ir_mapping_works(Dialect::Claude, Dialect::OpenAi);
    }
    #[test]
    fn openai_to_gemini_ir() {
        assert_ir_mapping_works(Dialect::OpenAi, Dialect::Gemini);
    }
    #[test]
    fn gemini_to_openai_ir() {
        assert_ir_mapping_works(Dialect::Gemini, Dialect::OpenAi);
    }
    #[test]
    fn claude_to_gemini_ir() {
        assert_ir_mapping_works(Dialect::Claude, Dialect::Gemini);
    }
    #[test]
    fn gemini_to_claude_ir() {
        assert_ir_mapping_works(Dialect::Gemini, Dialect::Claude);
    }
    #[test]
    fn openai_to_codex_ir() {
        assert_ir_mapping_works(Dialect::OpenAi, Dialect::Codex);
    }
    #[test]
    fn codex_to_openai_ir() {
        assert_ir_mapping_works(Dialect::Codex, Dialect::OpenAi);
    }
    #[test]
    fn openai_to_kimi_ir() {
        assert_ir_mapping_works(Dialect::OpenAi, Dialect::Kimi);
    }
    #[test]
    fn kimi_to_openai_ir() {
        assert_ir_mapping_works(Dialect::Kimi, Dialect::OpenAi);
    }
    #[test]
    fn claude_to_kimi_ir() {
        assert_ir_mapping_works(Dialect::Claude, Dialect::Kimi);
    }
    #[test]
    fn kimi_to_claude_ir() {
        assert_ir_mapping_works(Dialect::Kimi, Dialect::Claude);
    }
    #[test]
    fn openai_to_copilot_ir() {
        assert_ir_mapping_works(Dialect::OpenAi, Dialect::Copilot);
    }
    #[test]
    fn copilot_to_openai_ir() {
        assert_ir_mapping_works(Dialect::Copilot, Dialect::OpenAi);
    }
    #[test]
    fn gemini_to_kimi_ir() {
        assert_ir_mapping_works(Dialect::Gemini, Dialect::Kimi);
    }
    #[test]
    fn kimi_to_gemini_ir() {
        assert_ir_mapping_works(Dialect::Kimi, Dialect::Gemini);
    }
    #[test]
    fn codex_to_claude_ir() {
        assert_ir_mapping_works(Dialect::Codex, Dialect::Claude);
    }
    #[test]
    fn claude_to_codex_ir() {
        assert_ir_mapping_works(Dialect::Claude, Dialect::Codex);
    }

    #[test]
    fn unsupported_pair_fails_with_error() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_conversation();
        let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &conv);
        assert!(result.is_err());
        match result.unwrap_err() {
            MapError::UnsupportedPair { .. } => {}
            other => panic!("Expected UnsupportedPair, got {other:?}"),
        }
    }
}

// ============================================================================
// Module 5: Cross-dialect IR mapping with tool use
// ============================================================================
mod cross_dialect_ir_with_tools {
    use super::*;

    fn assert_tool_mapping_works(from: Dialect, to: Dialect) {
        if let Some(mapper) = default_ir_mapper(from, to) {
            let conv = conversation_with_tool_use();
            let result = mapper.map_request(from, to, &conv);
            assert!(
                result.is_ok(),
                "Tool mapping failed for {from} -> {to}: {:?}",
                result.err()
            );
            let mapped = result.unwrap();
            assert!(!mapped.messages.is_empty());
        }
    }

    #[test]
    fn openai_to_claude_tools() {
        assert_tool_mapping_works(Dialect::OpenAi, Dialect::Claude);
    }
    #[test]
    fn claude_to_openai_tools() {
        assert_tool_mapping_works(Dialect::Claude, Dialect::OpenAi);
    }
    #[test]
    fn openai_to_gemini_tools() {
        assert_tool_mapping_works(Dialect::OpenAi, Dialect::Gemini);
    }
    #[test]
    fn gemini_to_openai_tools() {
        assert_tool_mapping_works(Dialect::Gemini, Dialect::OpenAi);
    }
    #[test]
    fn claude_to_gemini_tools() {
        assert_tool_mapping_works(Dialect::Claude, Dialect::Gemini);
    }
    #[test]
    fn openai_to_kimi_tools() {
        assert_tool_mapping_works(Dialect::OpenAi, Dialect::Kimi);
    }
    #[test]
    fn openai_to_copilot_tools() {
        assert_tool_mapping_works(Dialect::OpenAi, Dialect::Copilot);
    }
    #[test]
    fn openai_to_codex_tools() {
        assert_tool_mapping_works(Dialect::OpenAi, Dialect::Codex);
    }
    #[test]
    fn codex_to_claude_tools() {
        assert_tool_mapping_works(Dialect::Codex, Dialect::Claude);
    }
    #[test]
    fn gemini_to_kimi_tools() {
        assert_tool_mapping_works(Dialect::Gemini, Dialect::Kimi);
    }
}

// ============================================================================
// Module 6: Lowering to vendor-specific formats
// ============================================================================
mod lowering {
    use super::*;

    #[test]
    fn lower_to_openai_has_messages_key() {
        let conv = simple_conversation();
        let result = lower_to_openai(&conv, &[]);
        assert!(result.get("messages").is_some());
        assert!(result["messages"].is_array());
    }

    #[test]
    fn lower_to_openai_with_tools() {
        let conv = simple_conversation();
        let tools = sample_tool_definitions();
        let result = lower_to_openai(&conv, &tools);
        assert!(result.get("tools").is_some());
        assert_eq!(result["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn lower_to_claude_extracts_system() {
        let conv = simple_conversation();
        let result = lower_to_claude(&conv, &[]);
        assert!(result.get("system").is_some());
        assert!(result["system"].as_str().unwrap().contains("helpful"));
    }

    #[test]
    fn lower_to_claude_messages_exclude_system() {
        let conv = simple_conversation();
        let result = lower_to_claude(&conv, &[]);
        let messages = result["messages"].as_array().unwrap();
        for msg in messages {
            assert_ne!(msg["role"].as_str().unwrap(), "system");
        }
    }

    #[test]
    fn lower_to_claude_with_tools() {
        let conv = simple_conversation();
        let tools = sample_tool_definitions();
        let result = lower_to_claude(&conv, &tools);
        assert!(result.get("tools").is_some());
        let tool_arr = result["tools"].as_array().unwrap();
        assert_eq!(tool_arr.len(), 2);
        assert!(tool_arr[0].get("input_schema").is_some());
    }

    #[test]
    fn lower_to_gemini_has_contents_key() {
        let conv = simple_conversation();
        let result = lower_to_gemini(&conv, &[]);
        assert!(result.get("contents").is_some());
    }

    #[test]
    fn lower_to_gemini_extracts_system_instruction() {
        let conv = simple_conversation();
        let result = lower_to_gemini(&conv, &[]);
        assert!(result.get("system_instruction").is_some());
    }

    #[test]
    fn lower_to_gemini_with_tools() {
        let conv = simple_conversation();
        let tools = sample_tool_definitions();
        let result = lower_to_gemini(&conv, &tools);
        assert!(result.get("tools").is_some());
        let tools_arr = result["tools"].as_array().unwrap();
        assert!(tools_arr[0].get("function_declarations").is_some());
    }

    #[test]
    fn lower_to_openai_tool_use_conversation() {
        let conv = conversation_with_tool_use();
        let result = lower_to_openai(&conv, &[]);
        let messages = result["messages"].as_array().unwrap();
        assert!(messages.len() >= 3);
    }

    #[test]
    fn lower_to_claude_tool_use_conversation() {
        let conv = conversation_with_tool_use();
        let result = lower_to_claude(&conv, &[]);
        let messages = result["messages"].as_array().unwrap();
        assert!(!messages.is_empty());
    }
}

// ============================================================================
// Module 7: Role mapping across dialects
// ============================================================================
mod role_mapping {
    use super::*;
    use abp_sdk_types::Dialect as SdkDialect;

    #[test]
    fn system_role_is_system_for_all_sdk_dialects() {
        let dialects = [
            SdkDialect::OpenAi,
            SdkDialect::Claude,
            SdkDialect::Gemini,
            SdkDialect::Codex,
            SdkDialect::Kimi,
            SdkDialect::Copilot,
        ];
        for d in dialects {
            assert_eq!(ir_role_to_dialect(IrRole::System, d), "system");
        }
    }

    #[test]
    fn user_role_is_user_for_all_sdk_dialects() {
        let dialects = [
            SdkDialect::OpenAi,
            SdkDialect::Claude,
            SdkDialect::Gemini,
            SdkDialect::Codex,
            SdkDialect::Kimi,
            SdkDialect::Copilot,
        ];
        for d in dialects {
            assert_eq!(ir_role_to_dialect(IrRole::User, d), "user");
        }
    }

    #[test]
    fn assistant_role_varies_by_dialect() {
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, SdkDialect::OpenAi),
            "assistant"
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, SdkDialect::Claude),
            "assistant"
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, SdkDialect::Gemini),
            "model"
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, SdkDialect::Codex),
            "assistant"
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, SdkDialect::Kimi),
            "assistant"
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, SdkDialect::Copilot),
            "assistant"
        );
    }

    #[test]
    fn tool_role_maps_to_tool_for_openai_like() {
        assert_eq!(ir_role_to_dialect(IrRole::Tool, SdkDialect::OpenAi), "tool");
        assert_eq!(ir_role_to_dialect(IrRole::Tool, SdkDialect::Codex), "tool");
        assert_eq!(ir_role_to_dialect(IrRole::Tool, SdkDialect::Kimi), "tool");
        assert_eq!(
            ir_role_to_dialect(IrRole::Tool, SdkDialect::Copilot),
            "tool"
        );
    }

    #[test]
    fn tool_role_maps_to_user_for_claude_and_gemini() {
        assert_eq!(ir_role_to_dialect(IrRole::Tool, SdkDialect::Claude), "user");
        assert_eq!(ir_role_to_dialect(IrRole::Tool, SdkDialect::Gemini), "user");
    }
}

// ============================================================================
// Module 8: Mapping fidelity levels
// ============================================================================
mod fidelity_levels {
    use super::*;

    #[test]
    fn lossless_is_lossless() {
        assert!(Fidelity::Lossless.is_lossless());
        assert!(!Fidelity::Lossless.is_unsupported());
    }

    #[test]
    fn lossy_labeled_is_not_lossless() {
        let f = Fidelity::LossyLabeled {
            warning: "some loss".into(),
        };
        assert!(!f.is_lossless());
        assert!(!f.is_unsupported());
    }

    #[test]
    fn unsupported_is_unsupported() {
        let f = Fidelity::Unsupported {
            reason: "not available".into(),
        };
        assert!(f.is_unsupported());
        assert!(!f.is_lossless());
    }

    #[test]
    fn fidelity_serde_roundtrip() {
        let variants = vec![
            Fidelity::Lossless,
            Fidelity::LossyLabeled {
                warning: "test".into(),
            },
            Fidelity::Unsupported {
                reason: "nope".into(),
            },
        ];
        for f in &variants {
            let json = serde_json::to_string(f).unwrap();
            let back: Fidelity = serde_json::from_str(&json).unwrap();
            assert_eq!(*f, back);
        }
    }

    #[test]
    fn same_dialect_tool_use_is_lossless() {
        let reg = known_rules();
        for &d in all_dialects() {
            let rule = reg.lookup(d, d, features::TOOL_USE);
            assert!(rule.is_some(), "Missing tool_use rule for {d} -> {d}");
            assert!(rule.unwrap().fidelity.is_lossless());
        }
    }

    #[test]
    fn same_dialect_streaming_is_lossless() {
        let reg = known_rules();
        for &d in all_dialects() {
            let rule = reg.lookup(d, d, features::STREAMING);
            assert!(rule.is_some());
            assert!(rule.unwrap().fidelity.is_lossless());
        }
    }

    #[test]
    fn same_dialect_all_features_are_lossless() {
        let reg = known_rules();
        let feats = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        for &d in all_dialects() {
            for &f in &feats {
                let rule = reg.lookup(d, d, f);
                assert!(rule.is_some(), "Missing {f} rule for {d} -> {d}");
                assert!(
                    rule.unwrap().fidelity.is_lossless(),
                    "{f} not lossless for {d} -> {d}"
                );
            }
        }
    }

    #[test]
    fn openai_to_claude_tool_use_is_lossless() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn openai_to_codex_tool_use_is_lossy() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE);
        assert!(rule.is_some());
        assert!(!rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn image_input_to_codex_is_unsupported() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT);
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_unsupported());
    }

    #[test]
    fn thinking_cross_dialect_is_lossy() {
        let reg = known_rules();
        let rule = reg.lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING);
        assert!(rule.is_some());
        assert!(!rule.unwrap().fidelity.is_lossless());
        assert!(!rule.unwrap().fidelity.is_unsupported());
    }
}

// ============================================================================
// Module 9: Mapping validation
// ============================================================================
mod mapping_validation {
    use super::*;

    #[test]
    fn validate_existing_features_returns_results() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["tool_use".into(), "streaming".into()],
        );
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn validate_unknown_feature_returns_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["nonexistent_feature".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn validate_empty_feature_name_is_invalid() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].errors.is_empty());
    }

    #[test]
    fn validate_all_known_features_openai_to_claude() {
        let reg = known_rules();
        let feats: Vec<String> = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
        assert_eq!(results.len(), 5);
        assert!(results[0].fidelity.is_lossless()); // tool_use
        assert!(results[1].fidelity.is_lossless()); // streaming
    }
}

// ============================================================================
// Module 10: MappingRegistry
// ============================================================================
mod mapping_registry {
    use super::*;

    #[test]
    fn known_rules_is_non_empty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn known_rules_has_many_rules() {
        let reg = known_rules();
        assert!(reg.len() > 50, "Expected >50 rules, got {}", reg.len());
    }

    #[test]
    fn custom_registry_insert_and_lookup() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "custom_feat".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 1);
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "custom_feat");
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn rank_targets_excludes_self() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
        for (d, _) in &ranked {
            assert_ne!(*d, Dialect::OpenAi);
        }
    }

    #[test]
    fn rank_targets_sorted_descending() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
        for window in ranked.windows(2) {
            assert!(window[0].1 >= window[1].1);
        }
    }
}

// ============================================================================
// Module 11: MappingMatrix
// ============================================================================
mod mapping_matrix {
    use super::*;

    #[test]
    fn empty_matrix_returns_false() {
        let matrix = MappingMatrix::new();
        assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn set_and_get() {
        let mut matrix = MappingMatrix::new();
        matrix.set(Dialect::OpenAi, Dialect::Claude, true);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(!matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
    }

    #[test]
    fn from_registry_populates_supported_pairs() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        for &d in all_dialects() {
            assert!(
                matrix.is_supported(d, d),
                "Same-dialect {d} should be supported"
            );
        }
    }

    #[test]
    fn from_registry_has_cross_dialect_support() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
    }
}

// ============================================================================
// Module 12: Projection matrix - dialect pair registration
// ============================================================================
mod projection_dialect_pairs {
    use super::*;

    #[test]
    fn default_projection_has_all_identity_pairs() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in all_dialects() {
            let entry = pm.lookup(d, d);
            assert!(entry.is_some(), "Missing identity entry for {d}");
            assert_eq!(entry.unwrap().mode, ProjectionMode::Passthrough);
        }
    }

    #[test]
    fn default_projection_has_openai_claude_mapped() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::OpenAi, Dialect::Claude);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
    }

    #[test]
    fn default_projection_has_claude_openai_mapped() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::Claude, Dialect::OpenAi);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
    }

    #[test]
    fn default_projection_has_openai_gemini_mapped() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::OpenAi, Dialect::Gemini);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
    }

    #[test]
    fn default_projection_has_codex_openai_mapped() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::Codex, Dialect::OpenAi);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
    }

    #[test]
    fn default_projection_unsupported_pairs_exist() {
        let pm = ProjectionMatrix::with_defaults();
        let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Unsupported);
    }

    #[test]
    fn register_custom_pair() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::Kimi, Dialect::Copilot, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::Kimi, Dialect::Copilot);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Mapped);
    }

    #[test]
    fn same_dialect_registration_forces_passthrough() {
        let mut pm = ProjectionMatrix::new();
        pm.register(Dialect::OpenAi, Dialect::OpenAi, ProjectionMode::Mapped);
        let entry = pm.lookup(Dialect::OpenAi, Dialect::OpenAi);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().mode, ProjectionMode::Passthrough);
    }

    #[test]
    fn dialect_entry_count() {
        let pm = ProjectionMatrix::with_defaults();
        assert_eq!(pm.dialect_entry_count(), 36);
    }
}

// ============================================================================
// Module 13: Projection matrix - routing
// ============================================================================
mod projection_routing {
    use super::*;

    #[test]
    fn same_dialect_route_has_zero_cost() {
        let pm = ProjectionMatrix::with_defaults();
        for &d in all_dialects() {
            let route = pm.find_route(d, d);
            assert!(route.is_some());
            let path = route.unwrap();
            assert_eq!(path.cost, 0);
            assert!(path.hops.is_empty());
            assert_eq!(path.fidelity, 1.0);
        }
    }

    #[test]
    fn direct_mapped_route_has_cost_one() {
        let pm = ProjectionMatrix::with_defaults();
        let route = pm.find_route(Dialect::OpenAi, Dialect::Claude);
        assert!(route.is_some());
        let path = route.unwrap();
        assert_eq!(path.cost, 1);
        assert!(path.is_direct());
        assert!(!path.is_multi_hop());
    }

    #[test]
    fn routing_path_display() {
        let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
        let s = format!("{pair}");
        assert!(s.contains("OpenAI"));
        assert!(s.contains("Claude"));
    }
}

// ============================================================================
// Module 14: Projection matrix - resolve mapper
// ============================================================================
mod projection_mapper_resolve {
    use super::*;

    #[test]
    fn resolve_identity_mapper() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi);
        assert!(mapper.is_some());
    }

    #[test]
    fn resolve_openai_to_claude_mapper() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(mapper.is_some());
    }

    #[test]
    fn resolve_unsupported_pair_returns_none() {
        let pm = ProjectionMatrix::with_defaults();
        let mapper = pm.resolve_mapper(Dialect::Kimi, Dialect::Copilot);
        assert!(mapper.is_none());
    }
}

// ============================================================================
// Module 15: Projection matrix - compatibility scores
// ============================================================================
mod projection_compatibility {
    use super::*;

    #[test]
    fn same_dialect_compatibility_is_perfect() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);
        for &d in all_dialects() {
            let score = pm.compatibility_score(d, d);
            assert_eq!(score.fidelity, 1.0);
            assert_eq!(score.lossless_features, 2);
            assert_eq!(score.lossy_features, 0);
            assert_eq!(score.unsupported_features, 0);
        }
    }

    #[test]
    fn cross_dialect_compatibility_computed() {
        let reg = known_rules();
        let mut pm = ProjectionMatrix::with_mapping_registry(reg);
        pm.register_defaults();
        pm.set_mapping_features(vec![features::TOOL_USE.into(), features::STREAMING.into()]);
        let score = pm.compatibility_score(Dialect::OpenAi, Dialect::Claude);
        assert!(score.fidelity > 0.0);
        assert_eq!(score.lossless_features, 2);
    }

    #[test]
    fn compatibility_score_struct_fields() {
        let score = CompatibilityScore {
            source: Dialect::OpenAi,
            target: Dialect::Claude,
            fidelity: 0.9,
            lossless_features: 3,
            lossy_features: 1,
            unsupported_features: 0,
        };
        assert_eq!(score.source, Dialect::OpenAi);
        assert_eq!(score.target, Dialect::Claude);
    }
}

// ============================================================================
// Module 16: Capability negotiation per dialect
// ============================================================================
mod capability_negotiation {
    use super::*;

    #[test]
    fn openai_manifest_has_streaming() {
        let m = openai_gpt4o_manifest();
        assert!(m.contains_key(&Capability::Streaming));
    }

    #[test]
    fn claude_manifest_has_tool_use() {
        let m = claude_35_sonnet_manifest();
        assert!(m.contains_key(&Capability::ToolUse));
    }

    #[test]
    fn gemini_manifest_has_vision() {
        let m = gemini_15_pro_manifest();
        assert!(m.contains_key(&Capability::Vision));
    }

    #[test]
    fn kimi_manifest_has_streaming() {
        let m = kimi_manifest();
        assert!(m.contains_key(&Capability::Streaming));
    }

    #[test]
    fn codex_manifest_has_tool_read() {
        let m = codex_manifest();
        assert!(m.contains_key(&Capability::ToolRead));
    }

    #[test]
    fn copilot_manifest_has_tool_use() {
        let m = copilot_manifest();
        assert!(m.contains_key(&Capability::ToolUse));
    }

    #[test]
    fn negotiate_streaming_against_all_manifests() {
        for &d in all_dialects() {
            let m = manifest_for_dialect(d);
            let result = negotiate_capabilities(&[Capability::Streaming], &m);
            assert!(
                result.is_compatible(),
                "{d} manifest should support Streaming"
            );
        }
    }

    #[test]
    fn negotiate_tool_use_against_all_manifests() {
        for &d in all_dialects() {
            let m = manifest_for_dialect(d);
            let result = negotiate_capabilities(&[Capability::ToolUse], &m);
            assert!(
                result.is_compatible(),
                "{d} manifest should support ToolUse"
            );
        }
    }

    #[test]
    fn negotiate_unsupported_capability() {
        let m: BTreeMap<Capability, CoreSupportLevel> = BTreeMap::new();
        let result = negotiate_capabilities(&[Capability::Streaming], &m);
        assert!(!result.is_compatible());
        assert_eq!(result.unsupported.len(), 1);
    }

    #[test]
    fn check_capability_native_vs_emulated() {
        let m = claude_35_sonnet_manifest();
        let streaming_level = check_capability(&m, &Capability::Streaming);
        assert!(matches!(streaming_level, SupportLevel::Native));

        let fc_level = check_capability(&m, &Capability::FunctionCalling);
        assert!(matches!(fc_level, SupportLevel::Emulated { .. }));
    }
}

// ============================================================================
// Module 17: Emulation labeling
// ============================================================================
mod emulation_labeling {
    use super::*;

    #[test]
    fn can_emulate_extended_thinking() {
        assert!(can_emulate(&Capability::ExtendedThinking));
    }

    #[test]
    fn can_emulate_structured_output() {
        assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    }

    #[test]
    fn cannot_emulate_code_execution() {
        assert!(!can_emulate(&Capability::CodeExecution));
    }

    #[test]
    fn cannot_emulate_streaming() {
        assert!(!can_emulate(&Capability::Streaming));
    }

    #[test]
    fn cannot_emulate_tool_use() {
        assert!(!can_emulate(&Capability::ToolUse));
    }

    #[test]
    fn default_strategy_extended_thinking_is_prompt_injection() {
        let s = default_strategy(&Capability::ExtendedThinking);
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn default_strategy_structured_output_is_post_processing() {
        let s = default_strategy(&Capability::StructuredOutputJsonSchema);
        assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn default_strategy_code_execution_is_disabled() {
        let s = default_strategy(&Capability::CodeExecution);
        assert!(matches!(s, EmulationStrategy::Disabled { .. }));
    }

    #[test]
    fn emulation_engine_apply_records_actions() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "base"))
            .push(IrMessage::text(IrRole::User, "hi"));
        let report = engine.apply(
            &[
                Capability::ExtendedThinking,
                Capability::StructuredOutputJsonSchema,
                Capability::CodeExecution,
            ],
            &mut conv,
        );
        assert_eq!(report.applied.len(), 2);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn emulation_check_missing_does_not_mutate() {
        let engine = EmulationEngine::with_defaults();
        let report =
            engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);
        assert_eq!(report.applied.len(), 1);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn compute_fidelity_labels() {
        let native_caps = vec![Capability::Streaming, Capability::ToolUse];
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "think".into(),
                },
            }],
            warnings: vec![],
        };
        let labels = compute_fidelity(&native_caps, &report);
        assert!(matches!(
            labels.get(&Capability::Streaming),
            Some(FidelityLabel::Native)
        ));
        assert!(matches!(
            labels.get(&Capability::ToolUse),
            Some(FidelityLabel::Native)
        ));
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn emulation_config_override() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::CodeExecution,
            EmulationStrategy::SystemPromptInjection {
                prompt: "simulate code".into(),
            },
        );
        let engine = EmulationEngine::new(config);
        let strategy = engine.resolve_strategy(&Capability::CodeExecution);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn emulation_report_is_empty_when_no_caps() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = IrConversation::new();
        let report = engine.apply(&[], &mut conv);
        assert!(report.is_empty());
    }

    #[test]
    fn emulation_report_has_unemulatable() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);
        assert!(report.has_unemulatable());
    }
}

// ============================================================================
// Module 18: Early failure for unmappable requests
// ============================================================================
mod early_failure {
    use super::*;

    #[test]
    fn unsupported_pair_error() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Copilot,
            to: Dialect::Codex,
        };
        let msg = err.to_string();
        assert!(msg.contains("Copilot"));
        assert!(msg.contains("Codex"));
    }

    #[test]
    fn lossy_conversion_error() {
        let err = MapError::LossyConversion {
            field: "thinking".into(),
            reason: "no thinking block".into(),
        };
        assert!(err.to_string().contains("thinking"));
    }

    #[test]
    fn unmappable_tool_error() {
        let err = MapError::UnmappableTool {
            name: "computer_use".into(),
            reason: "not supported".into(),
        };
        assert!(err.to_string().contains("computer_use"));
    }

    #[test]
    fn incompatible_capability_error() {
        let err = MapError::IncompatibleCapability {
            capability: "logprobs".into(),
            reason: "target lacks logprobs".into(),
        };
        assert!(err.to_string().contains("logprobs"));
    }

    #[test]
    fn unmappable_content_error() {
        let err = MapError::UnmappableContent {
            field: "system".into(),
            reason: "image in system prompt".into(),
        };
        assert!(err.to_string().contains("system"));
    }

    #[test]
    fn map_error_serde_roundtrip() {
        let errors = vec![
            MapError::UnsupportedPair {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MapError::LossyConversion {
                field: "f".into(),
                reason: "r".into(),
            },
            MapError::UnmappableTool {
                name: "t".into(),
                reason: "r".into(),
            },
            MapError::IncompatibleCapability {
                capability: "c".into(),
                reason: "r".into(),
            },
            MapError::UnmappableContent {
                field: "f".into(),
                reason: "r".into(),
            },
        ];
        for e in &errors {
            let json = serde_json::to_string(e).unwrap();
            let back: MapError = serde_json::from_str(&json).unwrap();
            assert_eq!(*e, back);
        }
    }

    #[test]
    fn mapper_error_unsupported_capability() {
        let err = MappingError::UnsupportedCapability {
            capability: "vision".into(),
            source_dialect: Dialect::Gemini,
            target_dialect: Dialect::Codex,
        };
        assert!(err.to_string().contains("vision"));
    }

    #[test]
    fn mapper_error_unmappable_request() {
        let err = MappingError::UnmappableRequest {
            reason: "empty body".into(),
        };
        assert!(err.to_string().contains("empty body"));
    }

    #[test]
    fn projection_error_empty_matrix() {
        let err = ProjectionError::EmptyMatrix;
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn projection_error_no_suitable_backend() {
        let err = ProjectionError::NoSuitableBackend {
            reason: "nothing fits".into(),
        };
        assert!(err.to_string().contains("nothing fits"));
    }
}

// ============================================================================
// Module 19: IR normalization
// ============================================================================
mod ir_normalization {
    use super::*;

    #[test]
    fn dedup_system_merges_multiple_system_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "First."))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::System, "Second."));

        let result = normalize::dedup_system(&conv);
        let sys_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::System)
            .collect();
        assert_eq!(sys_msgs.len(), 1);
        let text = sys_msgs[0].text_content();
        assert!(text.contains("First."));
        assert!(text.contains("Second."));
    }

    #[test]
    fn trim_text_removes_whitespace() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello  "));
        let result = normalize::trim_text(&conv);
        assert_eq!(result.messages[0].text_content(), "hello");
    }
}

// ============================================================================
// Module 20: Tool definition mapping across dialects
// ============================================================================
mod tool_definition_mapping {
    use super::*;

    #[test]
    fn openai_tool_format_has_function_type() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let tools = sample_tool_definitions();
        let result = lower_to_openai(&conv, &tools);
        let tool_arr = result["tools"].as_array().unwrap();
        for t in tool_arr {
            assert_eq!(t["type"], "function");
            assert!(t["function"]["name"].is_string());
            assert!(t["function"]["description"].is_string());
            assert!(t["function"]["parameters"].is_object());
        }
    }

    #[test]
    fn claude_tool_format_has_input_schema() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let tools = sample_tool_definitions();
        let result = lower_to_claude(&conv, &tools);
        let tool_arr = result["tools"].as_array().unwrap();
        for t in tool_arr {
            assert!(t["name"].is_string());
            assert!(t["description"].is_string());
            assert!(t["input_schema"].is_object());
        }
    }

    #[test]
    fn gemini_tool_format_has_function_declarations() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let tools = sample_tool_definitions();
        let result = lower_to_gemini(&conv, &tools);
        let tools_arr = result["tools"].as_array().unwrap();
        let decls = tools_arr[0]["function_declarations"].as_array().unwrap();
        for d in decls {
            assert!(d["name"].is_string());
            assert!(d["description"].is_string());
        }
    }

    #[test]
    fn no_tools_omits_tools_key_openai() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let result = lower_to_openai(&conv, &[]);
        assert!(result.get("tools").is_none());
    }

    #[test]
    fn no_tools_omits_tools_key_claude() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let result = lower_to_claude(&conv, &[]);
        assert!(result.get("tools").is_none());
    }

    #[test]
    fn no_tools_omits_tools_key_gemini() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let result = lower_to_gemini(&conv, &[]);
        assert!(result.get("tools").is_none());
    }
}

// ============================================================================
// Module 21: Dialect detection
// ============================================================================
mod dialect_detection_tests {
    use super::*;
    use abp_dialect::DialectDetector;

    #[test]
    fn detect_openai_request() {
        let detector = DialectDetector::new();
        let body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let result = detector.detect(&body);
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::OpenAi);
    }

    #[test]
    fn detect_claude_request() {
        let detector = DialectDetector::new();
        let body = json!({
            "model": "claude-3-sonnet",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
            "type": "message"
        });
        let result = detector.detect(&body);
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::Claude);
    }

    #[test]
    fn detect_gemini_request() {
        let detector = DialectDetector::new();
        let body = json!({
            "model": "gemini-1.5-pro",
            "contents": [{"parts": [{"text": "hi"}]}]
        });
        let result = detector.detect(&body);
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::Gemini);
    }

    #[test]
    fn detect_codex_request() {
        let detector = DialectDetector::new();
        let body = json!({
            "model": "codex-mini",
            "items": [{"type": "message"}],
            "status": "completed",
            "object": "response"
        });
        let result = detector.detect(&body);
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::Codex);
    }

    #[test]
    fn detect_kimi_request() {
        let detector = DialectDetector::new();
        let body = json!({
            "model": "moonshot-v1",
            "messages": [{"role": "user", "content": "hi"}],
            "refs": [],
            "search_plus": true
        });
        let result = detector.detect(&body);
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::Kimi);
    }

    #[test]
    fn detect_empty_object_returns_none() {
        let detector = DialectDetector::new();
        let result = detector.detect(&json!({}));
        assert!(result.is_none());
    }

    #[test]
    fn detect_non_object_returns_none() {
        let detector = DialectDetector::new();
        assert!(detector.detect(&json!("string")).is_none());
        assert!(detector.detect(&json!(42)).is_none());
        assert!(detector.detect(&json!(null)).is_none());
    }

    #[test]
    fn detect_all_returns_sorted_results() {
        let detector = DialectDetector::new();
        let body = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let results = detector.detect_all(&body);
        assert!(!results.is_empty());
        for window in results.windows(2) {
            assert!(window[0].confidence >= window[1].confidence);
        }
    }

    #[test]
    fn detect_from_headers_claude() {
        let detector = DialectDetector::new();
        let mut headers = BTreeMap::new();
        headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
        headers.insert("x-api-key".to_string(), "sk-ant-xxx".to_string());
        let result = detector.detect_from_headers(&headers);
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::Claude);
    }

    #[test]
    fn detect_from_endpoint_openai() {
        let detector = DialectDetector::new();
        let result = detector.detect_from_endpoint("https://api.openai.com/v1/chat/completions");
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::OpenAi);
    }

    #[test]
    fn detect_from_endpoint_gemini() {
        let detector = DialectDetector::new();
        let result = detector.detect_from_endpoint(
            "https://generativelanguage.googleapis.com/v1/models/gemini-pro:generateContent",
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::Gemini);
    }

    #[test]
    fn detect_from_endpoint_kimi() {
        let detector = DialectDetector::new();
        let result = detector.detect_from_endpoint("https://api.moonshot.cn/v1/chat/completions");
        assert!(result.is_some());
        assert_eq!(result.unwrap().dialect, Dialect::Kimi);
    }
}

// ============================================================================
// Module 22: Projection matrix - backend projection
// ============================================================================
mod projection_backend {
    use super::*;

    #[test]
    fn project_empty_matrix_returns_error() {
        let pm = ProjectionMatrix::new();
        let wo = WorkOrderBuilder::new("test task").build();
        let result = pm.project(&wo);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProjectionError::EmptyMatrix));
    }

    #[test]
    fn project_with_single_backend() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai", openai_gpt4o_manifest(), Dialect::OpenAi, 50);
        let wo = WorkOrderBuilder::new("test").build();
        let result = pm.project(&wo);
        assert!(result.is_ok());
        let pr = result.unwrap();
        assert_eq!(pr.selected_backend, "openai");
    }

    #[test]
    fn project_with_multiple_backends() {
        let mut pm = ProjectionMatrix::with_defaults();
        pm.register_backend("openai", openai_gpt4o_manifest(), Dialect::OpenAi, 50);
        pm.register_backend("claude", claude_35_sonnet_manifest(), Dialect::Claude, 60);
        let wo = WorkOrderBuilder::new("test").build();
        let result = pm.project(&wo);
        assert!(result.is_ok());
        let pr = result.unwrap();
        assert!(!pr.fallback_chain.is_empty());
    }

    #[test]
    fn backend_count() {
        let mut pm = ProjectionMatrix::new();
        assert_eq!(pm.backend_count(), 0);
        pm.register_backend("a", openai_gpt4o_manifest(), Dialect::OpenAi, 50);
        assert_eq!(pm.backend_count(), 1);
        pm.register_backend("b", claude_35_sonnet_manifest(), Dialect::Claude, 60);
        assert_eq!(pm.backend_count(), 2);
    }

    #[test]
    fn remove_backend() {
        let mut pm = ProjectionMatrix::new();
        pm.register_backend("a", openai_gpt4o_manifest(), Dialect::OpenAi, 50);
        assert!(pm.remove_backend("a"));
        assert_eq!(pm.backend_count(), 0);
        assert!(!pm.remove_backend("nonexistent"));
    }

    #[test]
    fn remove_dialect_entry() {
        let mut pm = ProjectionMatrix::with_defaults();
        let removed = pm.remove(Dialect::OpenAi, Dialect::Claude);
        assert!(removed.is_some());
        assert!(pm.lookup(Dialect::OpenAi, Dialect::Claude).is_none());
    }
}

// ============================================================================
// Module 23: Projection mode enum
// ============================================================================
mod projection_mode_tests {
    use super::*;

    #[test]
    fn projection_mode_serde_roundtrip() {
        let modes = [
            ProjectionMode::Passthrough,
            ProjectionMode::Mapped,
            ProjectionMode::Unsupported,
        ];
        for m in &modes {
            let json = serde_json::to_string(m).unwrap();
            let back: ProjectionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(*m, back);
        }
    }

    #[test]
    fn projection_mode_equality() {
        assert_eq!(ProjectionMode::Passthrough, ProjectionMode::Passthrough);
        assert_ne!(ProjectionMode::Passthrough, ProjectionMode::Mapped);
    }
}

// ============================================================================
// Module 24: DialectPair
// ============================================================================
mod dialect_pair_tests {
    use super::*;

    #[test]
    fn dialect_pair_new_and_display() {
        let pair = DialectPair::new(Dialect::OpenAi, Dialect::Claude);
        assert_eq!(pair.source, Dialect::OpenAi);
        assert_eq!(pair.target, Dialect::Claude);
        let s = format!("{pair}");
        assert!(s.contains("\u{2192}")); // arrow
    }

    #[test]
    fn dialect_pair_serde_roundtrip() {
        let pair = DialectPair::new(Dialect::Gemini, Dialect::Kimi);
        let json = serde_json::to_string(&pair).unwrap();
        let back: DialectPair = serde_json::from_str(&json).unwrap();
        assert_eq!(pair, back);
    }

    #[test]
    fn dialect_pair_ordering() {
        let a = DialectPair::new(Dialect::Claude, Dialect::OpenAi);
        let b = DialectPair::new(Dialect::Gemini, Dialect::OpenAi);
        let _ = a < b || a >= b;
    }

    #[test]
    fn dialect_pair_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DialectPair::new(Dialect::OpenAi, Dialect::Claude));
        set.insert(DialectPair::new(Dialect::OpenAi, Dialect::Claude));
        assert_eq!(set.len(), 1);
    }
}

// ============================================================================
// Module 25: CapabilityRegistry
// ============================================================================
mod capability_registry {
    use super::*;

    #[test]
    fn registry_with_defaults_has_all_six() {
        let reg = CapabilityRegistry::with_defaults();
        assert!(reg.contains("openai/gpt-4o"));
        assert!(reg.contains("anthropic/claude-3.5-sonnet"));
        assert!(reg.contains("google/gemini-1.5-pro"));
        assert!(reg.contains("openai/codex"));
        assert!(reg.contains("moonshot/kimi"));
        assert!(reg.contains("github/copilot"));
    }

    #[test]
    fn registry_get_openai() {
        let reg = CapabilityRegistry::with_defaults();
        let m = reg.get("openai/gpt-4o");
        assert!(m.is_some());
        assert!(m.unwrap().contains_key(&Capability::Streaming));
    }

    #[test]
    fn registry_unregister() {
        let mut reg = CapabilityRegistry::with_defaults();
        assert!(reg.unregister("openai/gpt-4o"));
        assert!(!reg.contains("openai/gpt-4o"));
    }

    #[test]
    fn registry_custom_registration() {
        let mut reg = CapabilityRegistry::new();
        let m: BTreeMap<Capability, CoreSupportLevel> =
            [(Capability::Streaming, CoreSupportLevel::Native)]
                .into_iter()
                .collect();
        reg.register("custom", m);
        assert!(reg.contains("custom"));
    }
}

// ============================================================================
// Module 26: MappingError (from abp-mapping crate)
// ============================================================================
mod mapping_error_tests {
    use super::*;
    use abp_mapping::MappingError as MappingCrateError;

    #[test]
    fn feature_unsupported_display() {
        let err = MappingCrateError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        assert!(err.to_string().contains("logprobs"));
    }

    #[test]
    fn fidelity_loss_display() {
        let err = MappingCrateError::FidelityLoss {
            feature: "thinking".into(),
            warning: "degraded".into(),
        };
        assert!(err.to_string().contains("thinking"));
    }

    #[test]
    fn dialect_mismatch_display() {
        let err = MappingCrateError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn invalid_input_display() {
        let err = MappingCrateError::InvalidInput {
            reason: "empty".into(),
        };
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn mapping_error_serde_roundtrip() {
        let err = MappingCrateError::FeatureUnsupported {
            feature: "x".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MappingCrateError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }
}

// ============================================================================
// Module 27: IR content blocks and messages
// ============================================================================
mod ir_content_tests {
    use super::*;

    #[test]
    fn ir_message_text_factory() {
        let msg = IrMessage::text(IrRole::User, "hello");
        assert_eq!(msg.role, IrRole::User);
        assert_eq!(msg.text_content(), "hello");
        assert!(msg.is_text_only());
    }

    #[test]
    fn ir_message_with_tool_use() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me read that.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "foo.rs"}),
                },
            ],
        );
        assert!(!msg.is_text_only());
        assert_eq!(msg.tool_use_blocks().len(), 1);
    }

    #[test]
    fn ir_conversation_system_message() {
        let conv = simple_conversation();
        assert!(conv.system_message().is_some());
        assert_eq!(conv.system_message().unwrap().role, IrRole::System);
    }

    #[test]
    fn ir_conversation_empty() {
        let conv = IrConversation::new();
        assert!(conv.messages.is_empty());
        assert!(conv.system_message().is_none());
    }

    #[test]
    fn ir_content_block_serde_roundtrip() {
        let blocks = vec![
            IrContentBlock::Text { text: "hi".into() },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
            IrContentBlock::ToolUse {
                id: "1".into(),
                name: "t".into(),
                input: json!({}),
            },
            IrContentBlock::ToolResult {
                tool_use_id: "1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            },
            IrContentBlock::Thinking { text: "hmm".into() },
        ];
        for b in &blocks {
            let json = serde_json::to_string(b).unwrap();
            let back: IrContentBlock = serde_json::from_str(&json).unwrap();
            assert_eq!(*b, back);
        }
    }

    #[test]
    fn ir_tool_definition_serde_roundtrip() {
        let td = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object"}),
        };
        let json = serde_json::to_string(&td).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(td, back);
    }
}

// ============================================================================
// Module 28: Emulation strategy serde
// ============================================================================
mod emulation_serde {
    use super::*;

    #[test]
    fn strategy_system_prompt_injection_roundtrip() {
        let s = EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step.".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn strategy_post_processing_roundtrip() {
        let s = EmulationStrategy::PostProcessing {
            detail: "validate JSON".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn strategy_disabled_roundtrip() {
        let s = EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn emulation_config_roundtrip() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::SystemPromptInjection {
                prompt: "think".into(),
            },
        );
        let json = serde_json::to_string(&config).unwrap();
        let back: EmulationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn emulation_report_roundtrip() {
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "think".into(),
                },
            }],
            warnings: vec!["warn".into()],
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: EmulationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
    }
}

// ============================================================================
// Module 29: Specific IR mapper implementations
// ============================================================================
mod specific_ir_mappers {
    use super::*;

    #[test]
    fn openai_claude_ir_mapper_supported_pairs() {
        let mapper = OpenAiClaudeIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    }

    #[test]
    fn openai_gemini_ir_mapper_supported_pairs() {
        let mapper = OpenAiGeminiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
    }

    #[test]
    fn claude_gemini_ir_mapper_supported_pairs() {
        let mapper = ClaudeGeminiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
    }

    #[test]
    fn openai_codex_ir_mapper_supported_pairs() {
        let mapper = OpenAiCodexIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Codex)));
        assert!(pairs.contains(&(Dialect::Codex, Dialect::OpenAi)));
    }

    #[test]
    fn openai_kimi_ir_mapper_supported_pairs() {
        let mapper = OpenAiKimiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Kimi)));
        assert!(pairs.contains(&(Dialect::Kimi, Dialect::OpenAi)));
    }

    #[test]
    fn claude_kimi_ir_mapper_supported_pairs() {
        let mapper = ClaudeKimiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Kimi)));
        assert!(pairs.contains(&(Dialect::Kimi, Dialect::Claude)));
    }

    #[test]
    fn openai_copilot_ir_mapper_supported_pairs() {
        let mapper = OpenAiCopilotIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
        assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
    }

    #[test]
    fn gemini_kimi_ir_mapper_supported_pairs() {
        let mapper = GeminiKimiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
        assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
    }

    #[test]
    fn codex_claude_ir_mapper_supported_pairs() {
        let mapper = CodexClaudeIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
    }
}

// ============================================================================
// Module 30: All 30 permutations across 6 dialects (6*5=30 cross pairs)
// ============================================================================
mod all_permutations {
    use super::*;

    fn check_cross_dialect(from: Dialect, to: Dialect) {
        let reg = known_rules();
        let rule = reg.lookup(from, to, features::STREAMING);
        assert!(rule.is_some(), "Missing streaming rule for {from} -> {to}");
    }

    #[test]
    fn perm_openai_to_claude() {
        check_cross_dialect(Dialect::OpenAi, Dialect::Claude);
    }
    #[test]
    fn perm_openai_to_gemini() {
        check_cross_dialect(Dialect::OpenAi, Dialect::Gemini);
    }
    #[test]
    fn perm_openai_to_codex() {
        check_cross_dialect(Dialect::OpenAi, Dialect::Codex);
    }
    #[test]
    fn perm_openai_to_kimi() {
        check_cross_dialect(Dialect::OpenAi, Dialect::Kimi);
    }
    #[test]
    fn perm_openai_to_copilot() {
        check_cross_dialect(Dialect::OpenAi, Dialect::Copilot);
    }

    #[test]
    fn perm_claude_to_openai() {
        check_cross_dialect(Dialect::Claude, Dialect::OpenAi);
    }
    #[test]
    fn perm_claude_to_gemini() {
        check_cross_dialect(Dialect::Claude, Dialect::Gemini);
    }
    #[test]
    fn perm_claude_to_codex() {
        check_cross_dialect(Dialect::Claude, Dialect::Codex);
    }
    #[test]
    fn perm_claude_to_kimi() {
        check_cross_dialect(Dialect::Claude, Dialect::Kimi);
    }
    #[test]
    fn perm_claude_to_copilot() {
        check_cross_dialect(Dialect::Claude, Dialect::Copilot);
    }

    #[test]
    fn perm_gemini_to_openai() {
        check_cross_dialect(Dialect::Gemini, Dialect::OpenAi);
    }
    #[test]
    fn perm_gemini_to_claude() {
        check_cross_dialect(Dialect::Gemini, Dialect::Claude);
    }
    #[test]
    fn perm_gemini_to_codex() {
        check_cross_dialect(Dialect::Gemini, Dialect::Codex);
    }
    #[test]
    fn perm_gemini_to_kimi() {
        check_cross_dialect(Dialect::Gemini, Dialect::Kimi);
    }
    #[test]
    fn perm_gemini_to_copilot() {
        check_cross_dialect(Dialect::Gemini, Dialect::Copilot);
    }

    #[test]
    fn perm_codex_to_openai() {
        check_cross_dialect(Dialect::Codex, Dialect::OpenAi);
    }
    #[test]
    fn perm_codex_to_claude() {
        check_cross_dialect(Dialect::Codex, Dialect::Claude);
    }
    #[test]
    fn perm_codex_to_gemini() {
        check_cross_dialect(Dialect::Codex, Dialect::Gemini);
    }
    #[test]
    fn perm_codex_to_kimi() {
        check_cross_dialect(Dialect::Codex, Dialect::Kimi);
    }
    #[test]
    fn perm_codex_to_copilot() {
        check_cross_dialect(Dialect::Codex, Dialect::Copilot);
    }

    #[test]
    fn perm_kimi_to_openai() {
        check_cross_dialect(Dialect::Kimi, Dialect::OpenAi);
    }
    #[test]
    fn perm_kimi_to_claude() {
        check_cross_dialect(Dialect::Kimi, Dialect::Claude);
    }
    #[test]
    fn perm_kimi_to_gemini() {
        check_cross_dialect(Dialect::Kimi, Dialect::Gemini);
    }
    #[test]
    fn perm_kimi_to_codex() {
        check_cross_dialect(Dialect::Kimi, Dialect::Codex);
    }
    #[test]
    fn perm_kimi_to_copilot() {
        check_cross_dialect(Dialect::Kimi, Dialect::Copilot);
    }

    #[test]
    fn perm_copilot_to_openai() {
        check_cross_dialect(Dialect::Copilot, Dialect::OpenAi);
    }
    #[test]
    fn perm_copilot_to_claude() {
        check_cross_dialect(Dialect::Copilot, Dialect::Claude);
    }
    #[test]
    fn perm_copilot_to_gemini() {
        check_cross_dialect(Dialect::Copilot, Dialect::Gemini);
    }
    #[test]
    fn perm_copilot_to_codex() {
        check_cross_dialect(Dialect::Copilot, Dialect::Codex);
    }
    #[test]
    fn perm_copilot_to_kimi() {
        check_cross_dialect(Dialect::Copilot, Dialect::Kimi);
    }
}

// ============================================================================
// Module 31: Projection score
// ============================================================================
mod projection_score_tests {
    use super::*;

    #[test]
    fn projection_score_serde_roundtrip() {
        let score = ProjectionScore {
            capability_coverage: 0.9,
            mapping_fidelity: 0.8,
            priority: 0.7,
            total: 0.85,
        };
        let json = serde_json::to_string(&score).unwrap();
        let back: ProjectionScore = serde_json::from_str(&json).unwrap();
        assert_eq!(score, back);
    }
}

// ============================================================================
// Module 32: RoutingHop / RoutingPath
// ============================================================================
mod routing_tests {
    use super::*;

    #[test]
    fn routing_hop_serde_roundtrip() {
        let hop = RoutingHop {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
            mapper_hint: Some("openai_to_claude".into()),
        };
        let json = serde_json::to_string(&hop).unwrap();
        let back: RoutingHop = serde_json::from_str(&json).unwrap();
        assert_eq!(hop, back);
    }

    #[test]
    fn routing_path_direct() {
        let path = RoutingPath {
            hops: vec![RoutingHop {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
                mapper_hint: None,
            }],
            cost: 1,
            fidelity: 0.9,
        };
        assert!(path.is_direct());
        assert!(!path.is_multi_hop());
    }

    #[test]
    fn routing_path_multi_hop() {
        let path = RoutingPath {
            hops: vec![
                RoutingHop {
                    from: Dialect::Kimi,
                    to: Dialect::OpenAi,
                    mapper_hint: None,
                },
                RoutingHop {
                    from: Dialect::OpenAi,
                    to: Dialect::Copilot,
                    mapper_hint: None,
                },
            ],
            cost: 2,
            fidelity: 0.7,
        };
        assert!(path.is_multi_hop());
        assert!(!path.is_direct());
    }

    #[test]
    fn routing_path_serde_roundtrip() {
        let path = RoutingPath {
            hops: vec![RoutingHop {
                from: Dialect::OpenAi,
                to: Dialect::Gemini,
                mapper_hint: Some("openai_to_gemini".into()),
            }],
            cost: 1,
            fidelity: 0.95,
        };
        let json = serde_json::to_string(&path).unwrap();
        let back: RoutingPath = serde_json::from_str(&json).unwrap();
        assert_eq!(path, back);
    }
}
