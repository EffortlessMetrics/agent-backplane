// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive mapper validation and error handling tests.
//!
//! Covers:
//! 1. Mapper factory: registration, lookup, all pairs, thread safety
//! 2. Input validation: edge cases in IR conversations
//! 3. Output validation: mapped output well-formed, tool IDs preserved, usage
//! 4. Error cases: MapError variants, ErrorCode mapping
//! 5. Fidelity reporting: lossy detection, thinking block loss, roundtrip

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_dialect::Dialect;
use abp_error::ErrorCode;
use abp_mapper::{
    ClaudeGeminiIrMapper, IrIdentityMapper, IrMapper, MapError, OpenAiClaudeIrMapper,
    OpenAiGeminiIrMapper, default_ir_mapper, supported_ir_pairs,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn tool_call_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Checking.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc_1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "NYC"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 72°F."),
    ])
}

fn thinking_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Step by step...".into(),
                },
                IrContentBlock::Text {
                    text: "Answer: 42".into(),
                },
            ],
        ),
    ])
}

fn multi_tool_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Do two things"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "read_file".into(),
                    input: json!({"path": "main.rs"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "found it".into(),
                    }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main(){}".into(),
                    }],
                    is_error: false,
                },
            ],
        ),
    ])
}

fn image_conv() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Describe this".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ],
    )])
}

// ═══════════════════════════════════════════════════════════════════════
// Module 1: Mapper factory (10 tests)
// ═══════════════════════════════════════════════════════════════════════

mod factory_tests {
    use super::*;

    #[test]
    fn factory_returns_identity_for_same_dialect() {
        for &d in Dialect::all() {
            let mapper = default_ir_mapper(d, d);
            assert!(mapper.is_some(), "no identity mapper for {d}");
        }
    }

    #[test]
    fn factory_openai_claude_both_directions() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
        assert!(default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
    }

    #[test]
    fn factory_openai_gemini_both_directions() {
        assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
        assert!(default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).is_some());
    }

    #[test]
    fn factory_claude_gemini_both_directions() {
        assert!(default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
        assert!(default_ir_mapper(Dialect::Gemini, Dialect::Claude).is_some());
    }

    #[test]
    fn factory_unsupported_pair_returns_none() {
        assert!(default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
        assert!(default_ir_mapper(Dialect::Codex, Dialect::Kimi).is_none());
    }

    #[test]
    fn factory_supported_pairs_includes_identity() {
        let pairs = supported_ir_pairs();
        for &d in Dialect::all() {
            assert!(pairs.contains(&(d, d)), "missing identity for {d}");
        }
    }

    #[test]
    fn factory_supported_pairs_includes_cross_dialect() {
        let pairs = supported_ir_pairs();
        let expected = [
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::Gemini, Dialect::OpenAi),
            (Dialect::Claude, Dialect::Gemini),
            (Dialect::Gemini, Dialect::Claude),
        ];
        for pair in &expected {
            assert!(pairs.contains(pair), "missing pair {:?}", pair);
        }
    }

    #[test]
    fn factory_all_supported_pairs_resolve() {
        for (from, to) in supported_ir_pairs() {
            assert!(
                default_ir_mapper(from, to).is_some(),
                "supported pair ({from}, {to}) did not resolve"
            );
        }
    }

    #[test]
    fn factory_mapper_is_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn IrMapper>();
    }

    #[test]
    fn factory_thread_safety_concurrent_lookups() {
        // Verify factory can be called from multiple threads
        let handles: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    let _ = default_ir_mapper(Dialect::OpenAi, Dialect::Claude);
                    let _ = default_ir_mapper(Dialect::Claude, Dialect::Gemini);
                    let _ = supported_ir_pairs();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 2: Input validation (15 tests)
// ═══════════════════════════════════════════════════════════════════════

mod input_validation {
    use super::*;

    #[test]
    fn empty_conversation_accepted_openai_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::new();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &conv);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn empty_conversation_accepted_openai_gemini() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::new();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Gemini, &conv);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn empty_conversation_accepted_claude_gemini() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::new();
        let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &conv);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn single_user_message_accepted() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hi")]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.messages[0].role, IrRole::User);
    }

    #[test]
    fn system_only_message_accepted() {
        let mapper = OpenAiClaudeIrMapper;
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "Be helpful")]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.messages[0].role, IrRole::System);
    }

    #[test]
    fn message_with_empty_text_block() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "")]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].text_content(), "");
    }

    #[test]
    fn message_with_no_content_blocks() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 0);
    }

    #[test]
    fn wrong_dialect_pair_rejected_openai_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_conv();
        let err = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn wrong_dialect_pair_rejected_openai_gemini() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = simple_conv();
        let err = mapper
            .map_request(Dialect::Claude, Dialect::Copilot, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn wrong_dialect_pair_rejected_claude_gemini() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = simple_conv();
        let err = mapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn many_messages_accepted() {
        let mapper = OpenAiClaudeIrMapper;
        let msgs: Vec<IrMessage> = (0..100)
            .map(|i| {
                if i % 2 == 0 {
                    IrMessage::text(IrRole::User, format!("User msg {i}"))
                } else {
                    IrMessage::text(IrRole::Assistant, format!("Asst msg {i}"))
                }
            })
            .collect();
        let conv = IrConversation::from_messages(msgs);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn tool_call_with_empty_input_accepted() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "run"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "noop".into(),
                    input: json!({}),
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn tool_result_with_error_flag_accepted() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "go"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({"cmd": "ls"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "error!".into(),
                    }],
                    is_error: true,
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        // Tool message becomes User in Gemini
        assert_eq!(result.messages[2].role, IrRole::User);
    }

    #[test]
    fn metadata_on_message_accepted() {
        let mapper = OpenAiClaudeIrMapper;
        let mut msg = IrMessage::text(IrRole::User, "hello");
        msg.metadata.insert("key".into(), json!("value"));
        let conv = IrConversation::from_messages(vec![msg]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(
            result.messages[0].metadata.get("key"),
            Some(&json!("value"))
        );
    }

    #[test]
    fn identity_mapper_accepts_any_dialect_pair() {
        let mapper = IrIdentityMapper;
        let conv = simple_conv();
        // Even cross-dialect pairs are accepted by identity mapper
        let result = mapper.map_request(Dialect::Kimi, Dialect::Copilot, &conv);
        assert!(result.is_ok());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 3: Output validation (15 tests)
// ═══════════════════════════════════════════════════════════════════════

mod output_validation {
    use super::*;

    #[test]
    fn mapped_output_preserves_message_count_simple() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), conv.len());
    }

    #[test]
    fn mapped_output_preserves_text_content() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn mapped_output_no_empty_text_for_text_messages() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = simple_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        for msg in &result.messages {
            if msg
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Text { .. }))
            {
                // At least one text block should have content
                let text = msg.text_content();
                assert!(!text.is_empty() || msg.content.is_empty());
            }
        }
    }

    #[test]
    fn tool_use_id_preserved_openai_to_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_call_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        if let IrContentBlock::ToolUse { id, .. } = tool_calls[0] {
            assert_eq!(id, "tc_1");
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn tool_use_id_preserved_openai_to_gemini() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = tool_call_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        if let IrContentBlock::ToolUse { id, .. } = tool_calls[0] {
            assert_eq!(id, "tc_1");
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn tool_name_preserved_across_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_call_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        if let IrContentBlock::ToolUse { name, .. } = tool_calls[0] {
            assert_eq!(name, "get_weather");
        }
    }

    #[test]
    fn tool_input_preserved_across_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_call_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        if let IrContentBlock::ToolUse { input, .. } = tool_calls[0] {
            assert_eq!(input, &json!({"city": "NYC"}));
        }
    }

    #[test]
    fn tool_result_id_preserved_openai_to_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_call_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        // Tool result message (now User role in Claude)
        let tool_result_msg = &result.messages[2];
        if let IrContentBlock::ToolResult { tool_use_id, .. } = &tool_result_msg.content[0] {
            assert_eq!(tool_use_id, "tc_1");
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn multiple_tool_ids_preserved_after_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = multi_tool_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 2);
        let ids: Vec<&str> = tool_calls
            .iter()
            .filter_map(|b| {
                if let IrContentBlock::ToolUse { id, .. } = b {
                    Some(id.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(ids.contains(&"t1"));
        assert!(ids.contains(&"t2"));
    }

    #[test]
    fn image_blocks_preserved_openai_to_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = image_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let has_image = result.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }));
        assert!(has_image);
    }

    #[test]
    fn image_blocks_preserved_openai_to_gemini() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = image_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let has_image = result.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }));
        assert!(has_image);
    }

    #[test]
    fn tool_error_flag_preserved_through_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "go"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "e1".into(),
                    name: "fail_tool".into(),
                    input: json!({}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "e1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "denied".into(),
                    }],
                    is_error: true,
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        if let IrContentBlock::ToolResult { is_error, .. } = &result.messages[2].content[0] {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn metadata_preserved_through_openai_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let mut msg = IrMessage::text(IrRole::User, "hi");
        msg.metadata.insert("trace_id".into(), json!("abc-123"));
        let conv = IrConversation::from_messages(vec![msg]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(
            result.messages[0].metadata.get("trace_id"),
            Some(&json!("abc-123"))
        );
    }

    #[test]
    fn response_mapping_preserves_content() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_conv();
        let result = mapper
            .map_response(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), conv.len());
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn usage_aggregation_via_merge() {
        let u1 = IrUsage::from_io(100, 50);
        let u2 = IrUsage::from_io(200, 75);
        let merged = u1.merge(u2);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 125);
        assert_eq!(merged.total_tokens, 425);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 4: Error cases (15 tests)
// ═══════════════════════════════════════════════════════════════════════

mod error_cases {
    use super::*;

    #[test]
    fn unsupported_pair_error_captures_dialects() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = simple_conv();
        let err = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap_err();
        if let MapError::UnsupportedPair { from, to } = err {
            assert_eq!(from, Dialect::Gemini);
            assert_eq!(to, Dialect::Kimi);
        } else {
            panic!("expected UnsupportedPair");
        }
    }

    #[test]
    fn unsupported_pair_display_mentions_dialects() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        };
        let msg = err.to_string();
        assert!(msg.contains("Kimi"), "display should mention source");
        assert!(msg.contains("Copilot"), "display should mention target");
    }

    #[test]
    fn lossy_conversion_error_display() {
        let err = MapError::LossyConversion {
            field: "thinking".into(),
            reason: "no equivalent in target".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("thinking"));
        assert!(msg.contains("lossy"));
    }

    #[test]
    fn unmappable_tool_error_display() {
        let err = MapError::UnmappableTool {
            name: "computer_use".into(),
            reason: "not representable".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("computer_use"));
    }

    #[test]
    fn incompatible_capability_error_display() {
        let err = MapError::IncompatibleCapability {
            capability: "logprobs".into(),
            reason: "unsupported by target".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
        assert!(msg.contains("incompatible"));
    }

    #[test]
    fn map_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MapError>();
    }

    #[test]
    fn map_error_serialize_roundtrip_unsupported() {
        let err = MapError::UnsupportedPair {
            from: Dialect::OpenAi,
            to: Dialect::Kimi,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn map_error_serialize_roundtrip_lossy() {
        let err = MapError::LossyConversion {
            field: "audio".into(),
            reason: "dropped".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn map_error_serialize_roundtrip_unmappable_tool() {
        let err = MapError::UnmappableTool {
            name: "bash".into(),
            reason: "restricted".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn map_error_serialize_roundtrip_incompatible() {
        let err = MapError::IncompatibleCapability {
            capability: "vision".into(),
            reason: "no support".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn error_code_lossy_conversion_exists() {
        let code = ErrorCode::MappingLossyConversion;
        assert_eq!(code.as_str(), "mapping_lossy_conversion");
    }

    #[test]
    fn error_code_lossy_display_uses_message() {
        let code = ErrorCode::MappingLossyConversion;
        // Display uses .message(), which is human-readable
        let display = format!("{code}");
        assert_eq!(display, code.message());
        assert_ne!(display, code.as_str());
    }

    #[test]
    fn error_code_unmappable_tool_exists() {
        let code = ErrorCode::MappingUnmappableTool;
        assert_eq!(code.as_str(), "mapping_unmappable_tool");
    }

    #[test]
    fn error_code_dialect_mismatch_exists() {
        let code = ErrorCode::MappingDialectMismatch;
        assert_eq!(code.as_str(), "mapping_dialect_mismatch");
    }

    #[test]
    fn error_code_mapping_category() {
        use abp_error::ErrorCategory;
        assert_eq!(
            ErrorCode::MappingLossyConversion.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingUnmappableTool.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingDialectMismatch.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::MappingUnsupportedCapability.category(),
            ErrorCategory::Mapping
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module 5: Fidelity reporting (10 tests)
// ═══════════════════════════════════════════════════════════════════════

mod fidelity_reporting {
    use super::*;

    #[test]
    fn thinking_blocks_dropped_claude_to_openai() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = thinking_conv();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let has_thinking = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        });
        assert!(
            !has_thinking,
            "thinking blocks should be dropped for OpenAI"
        );
    }

    #[test]
    fn thinking_blocks_dropped_claude_to_gemini() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = thinking_conv();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let has_thinking = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        });
        assert!(
            !has_thinking,
            "thinking blocks should be dropped for Gemini"
        );
    }

    #[test]
    fn thinking_text_content_survives_lossy_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = thinking_conv();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        // The non-thinking text should still be present
        assert_eq!(result.messages[1].text_content(), "Answer: 42");
    }

    #[test]
    fn roundtrip_simple_is_lossless_openai_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let orig = simple_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &mapped)
            .unwrap();
        assert_eq!(orig.len(), back.len());
        for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
            assert_eq!(o.role, b.role);
            assert_eq!(o.text_content(), b.text_content());
        }
    }

    #[test]
    fn roundtrip_simple_is_lossless_openai_gemini() {
        let mapper = OpenAiGeminiIrMapper;
        let orig = simple_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::Gemini, Dialect::OpenAi, &mapped)
            .unwrap();
        assert_eq!(orig.len(), back.len());
        for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
            assert_eq!(o.role, b.role);
            assert_eq!(o.text_content(), b.text_content());
        }
    }

    #[test]
    fn roundtrip_thinking_is_lossy() {
        let mapper = OpenAiClaudeIrMapper;
        let orig = thinking_conv();
        let openai = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
            .unwrap();
        // Thinking block is gone after roundtrip
        let orig_has_thinking = orig.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        });
        let back_has_thinking = back.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        });
        assert!(orig_has_thinking);
        assert!(!back_has_thinking, "thinking lost in roundtrip");
    }

    #[test]
    fn roundtrip_tool_calls_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let orig = tool_call_conv();
        let mapped = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &mapped)
            .unwrap();
        let orig_tools = orig.tool_calls();
        let back_tools = back.tool_calls();
        assert_eq!(orig_tools.len(), back_tools.len());
        for (ot, bt) in orig_tools.iter().zip(back_tools.iter()) {
            if let (
                IrContentBlock::ToolUse {
                    name: on,
                    input: oi,
                    ..
                },
                IrContentBlock::ToolUse {
                    name: bn,
                    input: bi,
                    ..
                },
            ) = (ot, bt)
            {
                assert_eq!(on, bn);
                assert_eq!(oi, bi);
            }
        }
    }

    #[test]
    fn lossy_error_code_snake_case_serialization() {
        let code = ErrorCode::MappingLossyConversion;
        let json = serde_json::to_value(code).unwrap();
        assert_eq!(json, json!("mapping_lossy_conversion"));
    }

    #[test]
    fn fidelity_score_identity_mapper_is_perfect() {
        let mapper = IrIdentityMapper;
        let conv = thinking_conv();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        // Identity mapper preserves everything including thinking blocks
        assert_eq!(conv, result);
    }

    #[test]
    fn fidelity_tool_count_stable_across_triple_mapping() {
        // OpenAI → Claude → Gemini should preserve tool call count
        let oc_mapper = OpenAiClaudeIrMapper;
        let cg_mapper = ClaudeGeminiIrMapper;
        let orig = tool_call_conv();
        let orig_tool_count = orig.tool_calls().len();

        let claude = oc_mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
            .unwrap();
        let gemini = cg_mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &claude)
            .unwrap();
        assert_eq!(
            gemini.tool_calls().len(),
            orig_tool_count,
            "tool count should be stable across chained mappings"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional edge-case and cross-cutting tests
// ═══════════════════════════════════════════════════════════════════════

mod cross_cutting {
    use super::*;

    #[test]
    fn openai_to_claude_tool_role_becomes_user() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_call_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[2].role, IrRole::User);
    }

    #[test]
    fn claude_to_openai_user_tool_results_become_tool_role() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = multi_tool_conv();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let tool_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Tool)
            .collect();
        assert_eq!(tool_msgs.len(), 2);
    }

    #[test]
    fn openai_to_gemini_tool_role_becomes_user() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = tool_call_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.messages[2].role, IrRole::User);
    }

    #[test]
    fn gemini_to_openai_user_tool_results_become_tool_role() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = multi_tool_conv();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
            .unwrap();
        let tool_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Tool)
            .collect();
        assert_eq!(tool_msgs.len(), 2);
    }

    #[test]
    fn claude_to_gemini_tool_role_becomes_user() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "go"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({"cmd": "ls"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "files".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.messages[2].role, IrRole::User);
    }

    #[test]
    fn gemini_to_claude_tool_role_becomes_user() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "go"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({"cmd": "ls"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "files".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[2].role, IrRole::User);
    }

    #[test]
    fn supported_pairs_list_is_deterministic() {
        let pairs1 = supported_ir_pairs();
        let pairs2 = supported_ir_pairs();
        assert_eq!(pairs1, pairs2);
    }

    #[test]
    fn mapper_clone_preserves_behavior() {
        let conv = simple_conv();
        let result1 = OpenAiClaudeIrMapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let result2 = OpenAiClaudeIrMapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result1, result2);
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
    fn thinking_dropped_openai_gemini_direction() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = thinking_conv();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let has_thinking = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        });
        assert!(!has_thinking);
    }
}
