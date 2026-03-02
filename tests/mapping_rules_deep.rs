// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for mapping/transformation rules between SDK dialects.
//!
//! Categories:
//! 1. MappingRule construction
//! 2. Rule application to IR messages (via validate_mapping)
//! 3. Cross-dialect mapping (openai→anthropic, anthropic→gemini, etc.)
//! 4. Mapping error handling (unmappable features)
//! 5. Mapping registry/lookup
//! 6. Composition of mapping rules
//! 7. Edge cases: identity mapping, lossy mapping, round-trip fidelity

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_dialect::Dialect;
use abp_mapping::{
    features, known_rules, validate_mapping, Fidelity, MappingError, MappingMatrix,
    MappingRegistry, MappingRule, MappingValidation,
};

// ═══════════════════════════════════════════════════════════════════════════
// 1. MappingRule construction
// ═══════════════════════════════════════════════════════════════════════════

mod rule_construction {
    use super::*;

    #[test]
    fn lossless_rule_fields() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        };
        assert_eq!(rule.source_dialect, Dialect::OpenAi);
        assert_eq!(rule.target_dialect, Dialect::Claude);
        assert_eq!(rule.feature, "tool_use");
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn lossy_rule_fields() {
        let rule = MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "thinking".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "mapped to system msg".into(),
            },
        };
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn unsupported_rule_fields() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Codex,
            feature: "image_input".into(),
            fidelity: Fidelity::Unsupported {
                reason: "Codex no images".into(),
            },
        };
        assert!(rule.fidelity.is_unsupported());
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn rule_clone_eq() {
        let rule = MappingRule {
            source_dialect: Dialect::Gemini,
            target_dialect: Dialect::Claude,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        };
        let cloned = rule.clone();
        assert_eq!(rule, cloned);
    }

    #[test]
    fn rule_with_empty_feature() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: String::new(),
            fidelity: Fidelity::Lossless,
        };
        assert!(rule.feature.is_empty());
    }

    #[test]
    fn rule_serde_roundtrip_lossless() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Gemini,
            feature: features::STREAMING.into(),
            fidelity: Fidelity::Lossless,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let deserialized: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, deserialized);
    }

    #[test]
    fn rule_serde_roundtrip_lossy() {
        let rule = MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Codex,
            feature: features::THINKING.into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "different budget model".into(),
            },
        };
        let json = serde_json::to_string(&rule).unwrap();
        let deserialized: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, deserialized);
    }

    #[test]
    fn rule_serde_roundtrip_unsupported() {
        let rule = MappingRule {
            source_dialect: Dialect::Gemini,
            target_dialect: Dialect::Codex,
            feature: features::IMAGE_INPUT.into(),
            fidelity: Fidelity::Unsupported {
                reason: "no images".into(),
            },
        };
        let json = serde_json::to_string(&rule).unwrap();
        let deserialized: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, deserialized);
    }

    #[test]
    fn rule_with_all_dialects_as_source() {
        for &d in Dialect::all() {
            let rule = MappingRule {
                source_dialect: d,
                target_dialect: Dialect::OpenAi,
                feature: "test".into(),
                fidelity: Fidelity::Lossless,
            };
            assert_eq!(rule.source_dialect, d);
        }
    }

    #[test]
    fn rule_with_all_dialects_as_target() {
        for &d in Dialect::all() {
            let rule = MappingRule {
                source_dialect: Dialect::Claude,
                target_dialect: d,
                feature: "test".into(),
                fidelity: Fidelity::Lossless,
            };
            assert_eq!(rule.target_dialect, d);
        }
    }

    #[test]
    fn fidelity_lossless_helpers() {
        let f = Fidelity::Lossless;
        assert!(f.is_lossless());
        assert!(!f.is_unsupported());
    }

    #[test]
    fn fidelity_lossy_helpers() {
        let f = Fidelity::LossyLabeled {
            warning: "some loss".into(),
        };
        assert!(!f.is_lossless());
        assert!(!f.is_unsupported());
    }

    #[test]
    fn fidelity_unsupported_helpers() {
        let f = Fidelity::Unsupported {
            reason: "nope".into(),
        };
        assert!(!f.is_lossless());
        assert!(f.is_unsupported());
    }

    #[test]
    fn fidelity_serde_all_variants() {
        let variants: Vec<Fidelity> = vec![
            Fidelity::Lossless,
            Fidelity::LossyLabeled {
                warning: "w".into(),
            },
            Fidelity::Unsupported {
                reason: "r".into(),
            },
        ];
        for v in &variants {
            let json = serde_json::to_string(v).unwrap();
            let parsed: Fidelity = serde_json::from_str(&json).unwrap();
            assert_eq!(v, &parsed);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Rule application to IR messages (via validate_mapping)
// ═══════════════════════════════════════════════════════════════════════════

mod rule_application_ir {
    use super::*;

    #[test]
    fn validate_text_message_features() {
        let reg = known_rules();
        let _msg = IrMessage::text(IrRole::User, "Hello");
        // Text content relies on tool_use and streaming being mapped
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::STREAMING.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn validate_tool_use_features_for_tool_message() {
        let reg = known_rules();
        let _msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: serde_json::json!({"q": "test"}),
            }],
        );
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::TOOL_USE.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn validate_image_features_for_image_block() {
        let reg = known_rules();
        let _msg = IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            }],
        );
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn validate_image_to_codex_unsupported_for_image_block() {
        let reg = known_rules();
        let _msg = IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            }],
        );
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn validate_thinking_features_for_thinking_block() {
        let reg = known_rules();
        let _msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "Let me think...".into(),
            }],
        );
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::THINKING.into()],
        );
        assert!(!results[0].fidelity.is_lossless());
        assert!(!results[0].fidelity.is_unsupported());
    }

    #[test]
    fn validate_conversation_all_features() {
        let reg = known_rules();
        let _conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are a helper"))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: serde_json::json!({}),
                }],
            ));
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[
                features::TOOL_USE.into(),
                features::STREAMING.into(),
                features::THINKING.into(),
                features::IMAGE_INPUT.into(),
            ],
        );
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn validate_tool_result_block_features() {
        let reg = known_rules();
        let _msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        );
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::Gemini,
            &[features::TOOL_USE.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn validate_error_tool_result_features() {
        let reg = known_rules();
        let _msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "error occurred".into(),
                }],
                is_error: true,
            }],
        );
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Gemini,
            &[features::TOOL_USE.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn ir_message_text_content_helper() {
        let msg = IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "hello ".into(),
                },
                IrContentBlock::Text {
                    text: "world".into(),
                },
            ],
        );
        assert_eq!(msg.text_content(), "hello world");
    }

    #[test]
    fn ir_message_is_text_only() {
        let text_msg = IrMessage::text(IrRole::User, "text");
        assert!(text_msg.is_text_only());

        let mixed = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "hello".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t".into(),
                    name: "n".into(),
                    input: serde_json::json!({}),
                },
            ],
        );
        assert!(!mixed.is_text_only());
    }

    #[test]
    fn ir_message_tool_use_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me help".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: serde_json::json!({}),
                },
                IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                },
            ],
        );
        assert_eq!(msg.tool_use_blocks().len(), 2);
    }

    #[test]
    fn ir_conversation_accessors() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "System"))
            .push(IrMessage::text(IrRole::User, "User"))
            .push(IrMessage::text(IrRole::Assistant, "Reply"));
        assert_eq!(conv.len(), 3);
        assert!(!conv.is_empty());
        assert_eq!(conv.system_message().unwrap().text_content(), "System");
        assert_eq!(conv.last_assistant().unwrap().text_content(), "Reply");
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
    }

    #[test]
    fn ir_conversation_empty() {
        let conv = IrConversation::new();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
        assert!(conv.tool_calls().is_empty());
    }

    #[test]
    fn ir_tool_definition_serde() {
        let tool = IrToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, parsed);
    }

    #[test]
    fn ir_usage_from_io() {
        let u = IrUsage::from_io(100, 50);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 0);
    }

    #[test]
    fn ir_usage_merge() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::from_io(200, 100);
        let merged = a.merge(b);
        assert_eq!(merged.total_tokens, 450);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Cross-dialect mapping
// ═══════════════════════════════════════════════════════════════════════════

mod cross_dialect {
    use super::*;

    #[test]
    fn openai_to_claude_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn claude_to_openai_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn openai_to_gemini_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn gemini_to_openai_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Gemini, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn claude_to_gemini_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn openai_to_codex_tool_use_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn codex_to_openai_tool_use_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Codex, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn claude_to_codex_tool_use_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn gemini_to_codex_tool_use_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Gemini, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn streaming_all_pairs_lossless() {
        let reg = known_rules();
        let core = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ];
        for &a in &core {
            for &b in &core {
                let rule = reg.lookup(a, b, features::STREAMING).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "streaming {} -> {} should be lossless",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn thinking_claude_to_openai_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn thinking_claude_to_gemini_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::Gemini, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn thinking_openai_to_claude_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn image_openai_to_claude_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn image_openai_to_gemini_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Gemini, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn image_claude_to_gemini_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::Gemini, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn image_to_codex_unsupported() {
        let reg = known_rules();
        for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg.lookup(src, Dialect::Codex, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image {} -> Codex should be unsupported",
                src
            );
        }
    }

    #[test]
    fn kimi_to_openai_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn copilot_to_openai_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Copilot, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn kimi_to_copilot_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn kimi_image_input_unsupported() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(Dialect::Kimi, tgt, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image Kimi -> {} should be unsupported",
                tgt
            );
        }
    }

    #[test]
    fn copilot_image_unsupported() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg
                .lookup(Dialect::Copilot, tgt, features::IMAGE_INPUT)
                .unwrap();
            assert!(rule.fidelity.is_unsupported());
        }
    }

    #[test]
    fn kimi_to_codex_tool_use_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn copilot_to_codex_tool_use_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Copilot, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn kimi_thinking_cross_dialect_lossy() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(Dialect::Kimi, tgt, features::THINKING).unwrap();
            assert!(
                !rule.fidelity.is_lossless(),
                "thinking Kimi -> {} should be lossy",
                tgt
            );
        }
    }

    #[test]
    fn copilot_thinking_cross_dialect_lossy() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg
                .lookup(Dialect::Copilot, tgt, features::THINKING)
                .unwrap();
            assert!(!rule.fidelity.is_lossless());
        }
    }

    #[test]
    fn kimi_streaming_lossless_all() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ] {
            let rule = reg.lookup(Dialect::Kimi, tgt, features::STREAMING).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming Kimi -> {} should be lossless",
                tgt
            );
        }
    }

    #[test]
    fn copilot_streaming_lossless_all() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Kimi,
        ] {
            let rule = reg
                .lookup(Dialect::Copilot, tgt, features::STREAMING)
                .unwrap();
            assert!(rule.fidelity.is_lossless());
        }
    }

    #[test]
    fn code_exec_cross_dialect_lossy() {
        let reg = known_rules();
        let code_capable = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ];
        for &a in &code_capable {
            for &b in &code_capable {
                if a == b {
                    continue;
                }
                let rule = reg.lookup(a, b, features::CODE_EXEC).unwrap();
                assert!(
                    !rule.fidelity.is_lossless(),
                    "code_exec {} -> {} should be lossy",
                    a,
                    b
                );
                assert!(
                    !rule.fidelity.is_unsupported(),
                    "code_exec {} -> {} should not be unsupported",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn kimi_code_exec_unsupported() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ] {
            let rule = reg.lookup(Dialect::Kimi, tgt, features::CODE_EXEC).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "code_exec Kimi -> {} should be unsupported",
                tgt
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Mapping error handling
// ═══════════════════════════════════════════════════════════════════════════

mod error_handling {
    use super::*;

    #[test]
    fn feature_unsupported_error_display() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("Claude"));
        assert!(msg.contains("Gemini"));
    }

    #[test]
    fn fidelity_loss_error_display() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "different budget model".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("thinking"));
        assert!(msg.contains("different budget model"));
    }

    #[test]
    fn dialect_mismatch_error_display() {
        let err = MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        let msg = err.to_string();
        assert!(msg.contains("OpenAI"));
        assert!(msg.contains("Codex"));
    }

    #[test]
    fn invalid_input_error_display() {
        let err = MappingError::InvalidInput {
            reason: "empty feature name".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("empty feature name"));
    }

    #[test]
    fn error_serde_feature_unsupported() {
        let err = MappingError::FeatureUnsupported {
            feature: "img".into(),
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, parsed);
    }

    #[test]
    fn error_serde_fidelity_loss() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "lossy".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, parsed);
    }

    #[test]
    fn error_serde_dialect_mismatch() {
        let err = MappingError::DialectMismatch {
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, parsed);
    }

    #[test]
    fn error_serde_invalid_input() {
        let err = MappingError::InvalidInput {
            reason: "bad input".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, parsed);
    }

    #[test]
    fn validate_empty_feature_gives_invalid_input_error() {
        let reg = known_rules();
        let results =
            validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::InvalidInput { .. }
        ));
    }

    #[test]
    fn validate_unknown_feature_gives_feature_unsupported_error() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["teleportation".into()],
        );
        assert_eq!(results[0].errors.len(), 1);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { .. }
        ));
    }

    #[test]
    fn validate_unsupported_feature_gives_feature_unsupported_error() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { .. }
        ));
    }

    #[test]
    fn validate_lossy_feature_gives_fidelity_loss_error() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::THINKING.into()],
        );
        assert!(!results[0].fidelity.is_lossless());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FidelityLoss { .. }
        ));
    }

    #[test]
    fn validate_lossless_feature_has_no_errors() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::TOOL_USE.into()],
        );
        assert!(results[0].errors.is_empty());
    }

    #[test]
    fn validate_empty_feature_list() {
        let reg = known_rules();
        let results =
            validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn validate_mixed_feature_results() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[
                features::STREAMING.into(),
                features::IMAGE_INPUT.into(),
                features::TOOL_USE.into(),
            ],
        );
        assert_eq!(results.len(), 3);
        // streaming: lossless
        assert!(results[0].errors.is_empty());
        // image_input: unsupported
        assert!(!results[1].errors.is_empty());
        // tool_use: lossy
        assert!(!results[2].errors.is_empty());
    }

    #[test]
    fn error_clone_eq() {
        let err = MappingError::FeatureUnsupported {
            feature: "x".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        assert_eq!(err, err.clone());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Mapping registry/lookup
// ═══════════════════════════════════════════════════════════════════════════

mod registry_lookup {
    use super::*;

    #[test]
    fn empty_registry_is_empty() {
        let reg = MappingRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn insert_single_rule() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
    }

    #[test]
    fn lookup_existing_rule() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn lookup_missing_rule() {
        let reg = MappingRegistry::new();
        assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use").is_none());
    }

    #[test]
    fn lookup_wrong_source() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        assert!(reg.lookup(Dialect::Claude, Dialect::Claude, "tool_use").is_none());
    }

    #[test]
    fn lookup_wrong_target() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        assert!(reg.lookup(Dialect::OpenAi, Dialect::Gemini, "tool_use").is_none());
    }

    #[test]
    fn lookup_wrong_feature() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, "streaming").is_none());
    }

    #[test]
    fn insert_replaces_same_key() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "changed".into(),
            },
        });
        assert_eq!(reg.len(), 1);
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn insert_different_features_same_pair() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn insert_different_pairs_same_feature() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Gemini,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn registry_iter_count() {
        let mut reg = MappingRegistry::new();
        for i in 0..5 {
            reg.insert(MappingRule {
                source_dialect: Dialect::OpenAi,
                target_dialect: Dialect::Claude,
                feature: format!("feat_{i}"),
                fidelity: Fidelity::Lossless,
            });
        }
        assert_eq!(reg.iter().count(), 5);
    }

    #[test]
    fn known_rules_nonempty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
        assert!(reg.len() > 50); // many rules for 6 dialects × 5 features
    }

    #[test]
    fn known_rules_self_mapping_all_lossless() {
        let reg = known_rules();
        let all_features = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        for &d in Dialect::all() {
            for &f in &all_features {
                let rule = reg.lookup(d, d, f).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "{d} -> {d} {f} should be lossless"
                );
            }
        }
    }

    #[test]
    fn rank_targets_basic() {
        let reg = known_rules();
        let ranked =
            reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
        assert!(!ranked.is_empty());
        // Claude should be in the results since both are lossless
        assert!(ranked.iter().any(|(d, _)| *d == Dialect::Claude));
    }

    #[test]
    fn rank_targets_excludes_source() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
        assert!(!ranked.iter().any(|(d, _)| *d == Dialect::OpenAi));
    }

    #[test]
    fn rank_targets_sorted_descending() {
        let reg = known_rules();
        let ranked = reg.rank_targets(
            Dialect::OpenAi,
            &[features::TOOL_USE, features::STREAMING, features::IMAGE_INPUT],
        );
        for w in ranked.windows(2) {
            assert!(w[0].1 >= w[1].1, "should be sorted by lossless count desc");
        }
    }

    #[test]
    fn rank_targets_empty_features() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_targets_unknown_feature() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &["nonexistent"]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn registry_clone() {
        let reg = known_rules();
        let cloned = reg.clone();
        assert_eq!(reg.len(), cloned.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Composition of mapping rules
// ═══════════════════════════════════════════════════════════════════════════

mod composition {
    use super::*;

    #[test]
    fn matrix_from_registry_marks_supported_pairs() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        // OpenAI->Claude has lossless tool_use, so supported
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn matrix_from_registry_unsupported_only_pair_not_supported() {
        // A registry with only unsupported rules should not mark pair as supported
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Gemini,
            target_dialect: Dialect::Codex,
            feature: "image_input".into(),
            fidelity: Fidelity::Unsupported {
                reason: "nope".into(),
            },
        });
        let matrix = MappingMatrix::from_registry(&reg);
        assert!(!matrix.is_supported(Dialect::Gemini, Dialect::Codex));
    }

    #[test]
    fn matrix_empty_default() {
        let matrix = MappingMatrix::new();
        assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert_eq!(matrix.get(Dialect::OpenAi, Dialect::Claude), None);
    }

    #[test]
    fn matrix_set_and_get() {
        let mut matrix = MappingMatrix::new();
        matrix.set(Dialect::OpenAi, Dialect::Claude, true);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert_eq!(matrix.get(Dialect::OpenAi, Dialect::Claude), Some(true));
    }

    #[test]
    fn matrix_not_symmetric() {
        let mut matrix = MappingMatrix::new();
        matrix.set(Dialect::OpenAi, Dialect::Claude, true);
        assert!(!matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
    }

    #[test]
    fn matrix_overwrite() {
        let mut matrix = MappingMatrix::new();
        matrix.set(Dialect::OpenAi, Dialect::Claude, true);
        matrix.set(Dialect::OpenAi, Dialect::Claude, false);
        assert!(!matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn matrix_from_known_rules_comprehensive() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        // All core pairs should be supported (at least streaming is lossless)
        let core = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ];
        for &a in &core {
            for &b in &core {
                if a != b {
                    assert!(
                        matrix.is_supported(a, b),
                        "{a} -> {b} should be supported"
                    );
                }
            }
        }
    }

    #[test]
    fn validate_then_matrix_consistency() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        // If matrix says supported, at least one feature must validate without unsupported
        let features_list: Vec<String> = vec![
            features::TOOL_USE.into(),
            features::STREAMING.into(),
        ];
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                if src == tgt {
                    continue;
                }
                if matrix.is_supported(src, tgt) {
                    let results = validate_mapping(&reg, src, tgt, &features_list);
                    let any_not_unsupported = results
                        .iter()
                        .any(|r| !r.fidelity.is_unsupported());
                    assert!(
                        any_not_unsupported,
                        "matrix says {src}->{tgt} supported but all features unsupported"
                    );
                }
            }
        }
    }

    #[test]
    fn compose_custom_registry_with_known() {
        let mut reg = known_rules();
        let initial_len = reg.len();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "custom_feature".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), initial_len + 1);
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "custom_feature")
                .is_some()
        );
    }

    #[test]
    fn validate_multi_feature_mixed_fidelity() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::Codex,
            &[
                features::STREAMING.into(),    // lossless
                features::THINKING.into(),     // lossy
                features::IMAGE_INPUT.into(),  // unsupported
            ],
        );
        assert!(results[0].fidelity.is_lossless());
        assert!(!results[1].fidelity.is_lossless());
        assert!(!results[1].fidelity.is_unsupported());
        assert!(results[2].fidelity.is_unsupported());
    }

    #[test]
    fn rank_targets_prefers_more_lossless() {
        let reg = known_rules();
        // OpenAI source: Claude/Gemini have lossless tool_use+streaming+image
        // Codex has lossy tool_use, lossless streaming, unsupported image
        let ranked = reg.rank_targets(
            Dialect::OpenAi,
            &[
                features::TOOL_USE,
                features::STREAMING,
                features::IMAGE_INPUT,
            ],
        );
        if ranked.len() >= 2 {
            // The first should have more lossless features than the last
            assert!(ranked[0].1 >= ranked[ranked.len() - 1].1);
        }
    }

    #[test]
    fn matrix_clone_equality() {
        let mut matrix = MappingMatrix::new();
        matrix.set(Dialect::OpenAi, Dialect::Claude, true);
        matrix.set(Dialect::Claude, Dialect::Gemini, false);
        let cloned = matrix.clone();
        assert_eq!(
            cloned.is_supported(Dialect::OpenAi, Dialect::Claude),
            matrix.is_supported(Dialect::OpenAi, Dialect::Claude)
        );
        assert_eq!(
            cloned.is_supported(Dialect::Claude, Dialect::Gemini),
            matrix.is_supported(Dialect::Claude, Dialect::Gemini)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Edge cases: identity, lossy, round-trip fidelity
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn identity_mapping_all_dialects_all_features() {
        let reg = known_rules();
        let all_features = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        for &d in Dialect::all() {
            for &f in &all_features {
                let rule = reg.lookup(d, d, f).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "identity mapping {d}->{d} for {f} should be lossless"
                );
            }
        }
    }

    #[test]
    fn identity_validation_no_errors() {
        let reg = known_rules();
        for &d in Dialect::all() {
            let results = validate_mapping(
                &reg,
                d,
                d,
                &[
                    features::TOOL_USE.into(),
                    features::STREAMING.into(),
                    features::THINKING.into(),
                    features::IMAGE_INPUT.into(),
                    features::CODE_EXEC.into(),
                ],
            );
            for r in &results {
                assert!(
                    r.errors.is_empty(),
                    "identity validation for {d} feature {} should have no errors",
                    r.feature
                );
            }
        }
    }

    #[test]
    fn roundtrip_fidelity_openai_claude() {
        let reg = known_rules();
        let features_list = [features::TOOL_USE, features::STREAMING];
        for &f in &features_list {
            let forward = reg.lookup(Dialect::OpenAi, Dialect::Claude, f).unwrap();
            let backward = reg.lookup(Dialect::Claude, Dialect::OpenAi, f).unwrap();
            // If forward is lossless, backward should also be lossless for symmetric pairs
            if forward.fidelity.is_lossless() {
                assert!(
                    backward.fidelity.is_lossless(),
                    "round-trip for {f} OpenAI<->Claude should be symmetric lossless"
                );
            }
        }
    }

    #[test]
    fn roundtrip_fidelity_openai_gemini() {
        let reg = known_rules();
        for &f in &[features::TOOL_USE, features::STREAMING, features::IMAGE_INPUT] {
            let forward = reg.lookup(Dialect::OpenAi, Dialect::Gemini, f).unwrap();
            let backward = reg.lookup(Dialect::Gemini, Dialect::OpenAi, f).unwrap();
            if forward.fidelity.is_lossless() {
                assert!(backward.fidelity.is_lossless());
            }
        }
    }

    #[test]
    fn roundtrip_fidelity_claude_gemini() {
        let reg = known_rules();
        for &f in &[features::TOOL_USE, features::STREAMING, features::IMAGE_INPUT] {
            let forward = reg.lookup(Dialect::Claude, Dialect::Gemini, f).unwrap();
            let backward = reg.lookup(Dialect::Gemini, Dialect::Claude, f).unwrap();
            if forward.fidelity.is_lossless() {
                assert!(backward.fidelity.is_lossless());
            }
        }
    }

    #[test]
    fn lossy_roundtrip_thinking() {
        let reg = known_rules();
        // Thinking Claude->OpenAI is lossy
        let fwd = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        assert!(!fwd.fidelity.is_lossless());
        let bwd = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING)
            .unwrap();
        assert!(!bwd.fidelity.is_lossless());
    }

    #[test]
    fn unsupported_roundtrip_image_codex() {
        let reg = known_rules();
        let fwd = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        assert!(fwd.fidelity.is_unsupported());
        let bwd = reg
            .lookup(Dialect::Codex, Dialect::OpenAi, features::IMAGE_INPUT)
            .unwrap();
        assert!(bwd.fidelity.is_unsupported());
    }

    #[test]
    fn validation_serde_roundtrip() {
        let v = MappingValidation {
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let json = serde_json::to_string(&v).unwrap();
        let parsed: MappingValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(v, parsed);
    }

    #[test]
    fn validation_serde_with_errors() {
        let v = MappingValidation {
            feature: "image_input".into(),
            fidelity: Fidelity::Unsupported {
                reason: "nope".into(),
            },
            errors: vec![MappingError::FeatureUnsupported {
                feature: "image_input".into(),
                from: Dialect::OpenAi,
                to: Dialect::Codex,
            }],
        };
        let json = serde_json::to_string(&v).unwrap();
        let parsed: MappingValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(v, parsed);
    }

    #[test]
    fn validate_many_unknown_features() {
        let reg = known_rules();
        let features: Vec<String> = (0..20).map(|i| format!("unknown_{i}")).collect();
        let results =
            validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features);
        assert_eq!(results.len(), 20);
        for r in &results {
            assert!(r.fidelity.is_unsupported());
            assert_eq!(r.errors.len(), 1);
        }
    }

    #[test]
    fn validate_duplicate_features() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[
                features::TOOL_USE.into(),
                features::TOOL_USE.into(),
                features::TOOL_USE.into(),
            ],
        );
        assert_eq!(results.len(), 3);
        // All should give same result
        for r in &results {
            assert!(r.fidelity.is_lossless());
        }
    }

    #[test]
    fn feature_constants_are_non_empty() {
        assert!(!features::TOOL_USE.is_empty());
        assert!(!features::STREAMING.is_empty());
        assert!(!features::THINKING.is_empty());
        assert!(!features::IMAGE_INPUT.is_empty());
        assert!(!features::CODE_EXEC.is_empty());
    }

    #[test]
    fn feature_constants_are_distinct() {
        let feats = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j], "features should be distinct");
            }
        }
    }

    #[test]
    fn dialect_all_has_six_entries() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn dialect_labels_non_empty() {
        for &d in Dialect::all() {
            assert!(!d.label().is_empty());
        }
    }

    #[test]
    fn dialect_display_matches_label() {
        for &d in Dialect::all() {
            assert_eq!(d.to_string(), d.label());
        }
    }

    #[test]
    fn mapping_rule_debug_impl() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        };
        let debug = format!("{rule:?}");
        assert!(debug.contains("MappingRule"));
        assert!(debug.contains("OpenAi"));
    }

    #[test]
    fn mapping_error_debug_impl() {
        let err = MappingError::FeatureUnsupported {
            feature: "x".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        let debug = format!("{err:?}");
        assert!(debug.contains("FeatureUnsupported"));
    }

    #[test]
    fn fidelity_debug_impl() {
        let f = Fidelity::LossyLabeled {
            warning: "test".into(),
        };
        let debug = format!("{f:?}");
        assert!(debug.contains("LossyLabeled"));
    }

    #[test]
    fn matrix_debug_impl() {
        let m = MappingMatrix::new();
        let debug = format!("{m:?}");
        assert!(debug.contains("MappingMatrix"));
    }

    #[test]
    fn registry_debug_impl() {
        let r = MappingRegistry::new();
        let debug = format!("{r:?}");
        assert!(debug.contains("MappingRegistry"));
    }

    #[test]
    fn ir_conversation_from_messages() {
        let msgs = vec![
            IrMessage::text(IrRole::User, "hi"),
            IrMessage::text(IrRole::Assistant, "hello"),
        ];
        let conv = IrConversation::from_messages(msgs);
        assert_eq!(conv.len(), 2);
    }

    #[test]
    fn ir_conversation_last_message() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "first"))
            .push(IrMessage::text(IrRole::Assistant, "last"));
        assert_eq!(conv.last_message().unwrap().text_content(), "last");
    }

    #[test]
    fn ir_conversation_tool_calls() {
        let conv = IrConversation::new()
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::ToolUse {
                        id: "a".into(),
                        name: "search".into(),
                        input: serde_json::json!({}),
                    },
                    IrContentBlock::ToolUse {
                        id: "b".into(),
                        name: "read".into(),
                        input: serde_json::json!({}),
                    },
                ],
            ))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c".into(),
                    name: "write".into(),
                    input: serde_json::json!({}),
                }],
            ));
        assert_eq!(conv.tool_calls().len(), 3);
    }

    #[test]
    fn ir_usage_with_cache() {
        let u = IrUsage::with_cache(100, 50, 30, 10);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 30);
        assert_eq!(u.cache_write_tokens, 10);
    }

    #[test]
    fn ir_usage_default() {
        let u = IrUsage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total_tokens, 0);
    }

    #[test]
    fn ir_message_metadata() {
        let mut msg = IrMessage::text(IrRole::User, "hi");
        msg.metadata
            .insert("key".into(), serde_json::json!("value"));
        assert_eq!(msg.metadata.len(), 1);
    }

    #[test]
    fn ir_message_new_has_empty_metadata() {
        let msg = IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "test".into(),
            }],
        );
        assert!(msg.metadata.is_empty());
    }

    #[test]
    fn comprehensive_all_dialect_pairs_have_streaming() {
        let reg = known_rules();
        for &a in Dialect::all() {
            for &b in Dialect::all() {
                let rule = reg.lookup(a, b, features::STREAMING);
                assert!(
                    rule.is_some(),
                    "streaming rule should exist for {a} -> {b}"
                );
            }
        }
    }

    #[test]
    fn comprehensive_all_self_pairs_have_all_features() {
        let reg = known_rules();
        let all_features = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        for &d in Dialect::all() {
            for &f in &all_features {
                assert!(
                    reg.lookup(d, d, f).is_some(),
                    "self-mapping rule for {d} feature {f} should exist"
                );
            }
        }
    }

    #[test]
    fn kimi_copilot_image_unsupported_between_each_other() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }

    #[test]
    fn kimi_copilot_thinking_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn kimi_copilot_streaming_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn kimi_copilot_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn rank_targets_includes_kimi_copilot() {
        let reg = known_rules();
        let ranked = reg.rank_targets(
            Dialect::OpenAi,
            &[features::TOOL_USE, features::STREAMING],
        );
        // Kimi and Copilot should appear since they have lossless tool_use+streaming to OpenAI
        let dialects: Vec<Dialect> = ranked.iter().map(|(d, _)| *d).collect();
        assert!(dialects.contains(&Dialect::Kimi));
        assert!(dialects.contains(&Dialect::Copilot));
    }
}
