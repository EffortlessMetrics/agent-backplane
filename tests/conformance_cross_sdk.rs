#![allow(clippy::all)]

//! Comprehensive conformance test suite for cross-SDK mapping correctness.
//!
//! Validates that the same logical request produces correct results when mapped
//! through any pair of SDK dialects via the IR-level mapping infrastructure.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_dialect::Dialect;
use abp_mapper::{MapError, default_ir_mapper, supported_ir_pairs};
use serde_json::json;
use std::collections::BTreeMap;

// ── Scenario builders ───────────────────────────────────────────────────

fn simple_chat() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello, how are you?")])
}

fn chat_with_system() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello!"),
    ])
}

fn tool_use_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather in London?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "get_weather".into(),
                input: json!({"city": "London"}),
            }],
        ),
    ])
}

fn tool_result_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather in London?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "get_weather".into(),
                input: json!({"city": "London"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Sunny, 22°C".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The weather in London is sunny at 22°C."),
    ])
}

fn multi_turn_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a coding assistant."),
        IrMessage::text(IrRole::User, "Write a hello world in Python."),
        IrMessage::text(IrRole::Assistant, "print('Hello, World!')"),
        IrMessage::text(IrRole::User, "Now make it a function."),
        IrMessage::text(
            IrRole::Assistant,
            "def hello():\n    print('Hello, World!')",
        ),
    ])
}

fn thinking_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Explain quantum computing."),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think about how to explain this simply...".into(),
                },
                IrContentBlock::Text {
                    text: "Quantum computing uses qubits instead of classical bits.".into(),
                },
            ],
        ),
    ])
}

fn image_scenario() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is in this image?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ],
    )])
}

fn unicode_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "こんにちは世界！🌍🎉 Ñoño café résumé naïve"),
        IrMessage::text(IrRole::Assistant, "你好！这是Unicode测试 🚀✨"),
    ])
}

fn long_content_scenario() -> IrConversation {
    let long_text = "A".repeat(10_000);
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, &long_text)])
}

fn empty_scenario() -> IrConversation {
    IrConversation::new()
}

fn multi_tool_scenario() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Compare weather in London and Paris"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "London"}),
                },
                IrContentBlock::ToolUse {
                    id: "call_2".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "Paris"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "London: Sunny 22°C".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_2".into(),
                content: vec![IrContentBlock::Text {
                    text: "Paris: Cloudy 18°C".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(
            IrRole::Assistant,
            "London is sunny at 22°C while Paris is cloudy at 18°C.",
        ),
    ])
}

fn metadata_scenario() -> IrConversation {
    let mut metadata = BTreeMap::new();
    metadata.insert("temperature".into(), json!(0.7));
    metadata.insert("model".into(), json!("gpt-4"));
    IrConversation::from_messages(vec![IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "Hello with metadata".into(),
        }],
        metadata,
    }])
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn run_mapping(ir: IrConversation, from: Dialect, to: Dialect) -> IrConversation {
    let mapper =
        default_ir_mapper(from, to).unwrap_or_else(|| panic!("no mapper for {from:?} -> {to:?}"));
    mapper
        .map_request(from, to, &ir)
        .unwrap_or_else(|e| panic!("mapping {from:?} -> {to:?} failed: {e}"))
}

fn run_roundtrip(ir: &IrConversation, from: Dialect, to: Dialect) -> IrConversation {
    let fwd = default_ir_mapper(from, to).expect("forward mapper");
    let rev = default_ir_mapper(to, from).expect("reverse mapper");
    let intermediate = fwd.map_request(from, to, ir).expect("forward map");
    rev.map_request(to, from, &intermediate)
        .expect("reverse map")
}

fn collect_all_text(conv: &IrConversation) -> String {
    conv.messages.iter().map(|m| m.text_content()).collect()
}

// ── Macro for generating all 24 supported-pair tests per scenario ───────

macro_rules! all_pair_tests {
    ($mod_name:ident, $scenario_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn identity_openai() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::OpenAi);
            }
            #[test]
            fn identity_claude() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Claude);
            }
            #[test]
            fn identity_gemini() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::Gemini);
            }
            #[test]
            fn identity_codex() {
                run_mapping($scenario_fn(), Dialect::Codex, Dialect::Codex);
            }
            #[test]
            fn identity_kimi() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::Kimi);
            }
            #[test]
            fn identity_copilot() {
                run_mapping($scenario_fn(), Dialect::Copilot, Dialect::Copilot);
            }
            #[test]
            fn openai_to_claude() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Claude);
            }
            #[test]
            fn claude_to_openai() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::OpenAi);
            }
            #[test]
            fn openai_to_gemini() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Gemini);
            }
            #[test]
            fn gemini_to_openai() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::OpenAi);
            }
            #[test]
            fn claude_to_gemini() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Gemini);
            }
            #[test]
            fn gemini_to_claude() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::Claude);
            }
            #[test]
            fn openai_to_codex() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Codex);
            }
            #[test]
            fn codex_to_openai() {
                run_mapping($scenario_fn(), Dialect::Codex, Dialect::OpenAi);
            }
            #[test]
            fn openai_to_kimi() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Kimi);
            }
            #[test]
            fn kimi_to_openai() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::OpenAi);
            }
            #[test]
            fn claude_to_kimi() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Kimi);
            }
            #[test]
            fn kimi_to_claude() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::Claude);
            }
            #[test]
            fn openai_to_copilot() {
                run_mapping($scenario_fn(), Dialect::OpenAi, Dialect::Copilot);
            }
            #[test]
            fn copilot_to_openai() {
                run_mapping($scenario_fn(), Dialect::Copilot, Dialect::OpenAi);
            }
            #[test]
            fn gemini_to_kimi() {
                run_mapping($scenario_fn(), Dialect::Gemini, Dialect::Kimi);
            }
            #[test]
            fn kimi_to_gemini() {
                run_mapping($scenario_fn(), Dialect::Kimi, Dialect::Gemini);
            }
            #[test]
            fn codex_to_claude() {
                run_mapping($scenario_fn(), Dialect::Codex, Dialect::Claude);
            }
            #[test]
            fn claude_to_codex() {
                run_mapping($scenario_fn(), Dialect::Claude, Dialect::Codex);
            }
        }
    };
}

// ── Per-scenario pair tests (12 × 24 = 288 tests) ──────────────────────

all_pair_tests!(simple_chat_pairs, simple_chat);
all_pair_tests!(system_prompt_pairs, chat_with_system);
all_pair_tests!(tool_use_pairs, tool_use_scenario);
all_pair_tests!(tool_result_pairs, tool_result_scenario);
all_pair_tests!(multi_turn_pairs, multi_turn_scenario);
all_pair_tests!(thinking_pairs, thinking_scenario);
all_pair_tests!(image_pairs, image_scenario);
all_pair_tests!(unicode_pairs, unicode_scenario);
all_pair_tests!(long_content_pairs, long_content_scenario);
all_pair_tests!(empty_pairs, empty_scenario);
all_pair_tests!(multi_tool_pairs, multi_tool_scenario);
all_pair_tests!(metadata_pairs, metadata_scenario);

// ── Passthrough identity: A → A preserves exact content ─────────────────

mod passthrough_identity {
    use super::*;

    macro_rules! identity_exact {
        ($name:ident, $dialect:expr, $scenario_fn:ident) => {
            #[test]
            fn $name() {
                let original = $scenario_fn();
                let mapped = run_mapping(original.clone(), $dialect, $dialect);
                assert_eq!(original, mapped, "identity mapping must be exact");
            }
        };
    }

    identity_exact!(simple_openai, Dialect::OpenAi, simple_chat);
    identity_exact!(simple_claude, Dialect::Claude, simple_chat);
    identity_exact!(simple_gemini, Dialect::Gemini, simple_chat);
    identity_exact!(simple_codex, Dialect::Codex, simple_chat);
    identity_exact!(simple_kimi, Dialect::Kimi, simple_chat);
    identity_exact!(simple_copilot, Dialect::Copilot, simple_chat);

    identity_exact!(multi_turn_openai, Dialect::OpenAi, multi_turn_scenario);
    identity_exact!(multi_turn_claude, Dialect::Claude, multi_turn_scenario);
    identity_exact!(multi_turn_gemini, Dialect::Gemini, multi_turn_scenario);
    identity_exact!(multi_turn_codex, Dialect::Codex, multi_turn_scenario);
    identity_exact!(multi_turn_kimi, Dialect::Kimi, multi_turn_scenario);
    identity_exact!(multi_turn_copilot, Dialect::Copilot, multi_turn_scenario);

    identity_exact!(tool_result_openai, Dialect::OpenAi, tool_result_scenario);
    identity_exact!(tool_result_claude, Dialect::Claude, tool_result_scenario);
    identity_exact!(tool_result_gemini, Dialect::Gemini, tool_result_scenario);
    identity_exact!(tool_result_codex, Dialect::Codex, tool_result_scenario);
    identity_exact!(tool_result_kimi, Dialect::Kimi, tool_result_scenario);
    identity_exact!(tool_result_copilot, Dialect::Copilot, tool_result_scenario);
}

// ── Round-trip tests: A → B → A preserves semantics ─────────────────────

mod roundtrip_text_preservation {
    use super::*;

    #[test]
    fn simple_chat_openai_claude() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_openai_gemini() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Gemini);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_openai_kimi() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Kimi);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_openai_copilot() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Copilot);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_claude_gemini() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::Claude, Dialect::Gemini);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_claude_kimi() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::Claude, Dialect::Kimi);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn simple_chat_gemini_kimi() {
        let orig = simple_chat();
        let rt = run_roundtrip(&orig, Dialect::Gemini, Dialect::Kimi);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn unicode_openai_claude() {
        let orig = unicode_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn unicode_claude_gemini() {
        let orig = unicode_scenario();
        let rt = run_roundtrip(&orig, Dialect::Claude, Dialect::Gemini);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn multi_turn_openai_claude() {
        let orig = multi_turn_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn multi_turn_openai_gemini() {
        let orig = multi_turn_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Gemini);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn multi_turn_claude_kimi() {
        let orig = multi_turn_scenario();
        let rt = run_roundtrip(&orig, Dialect::Claude, Dialect::Kimi);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn long_content_openai_claude() {
        let orig = long_content_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }

    #[test]
    fn long_content_openai_kimi() {
        let orig = long_content_scenario();
        let rt = run_roundtrip(&orig, Dialect::OpenAi, Dialect::Kimi);
        assert_eq!(collect_all_text(&orig), collect_all_text(&rt));
    }
}

// ── Round-trip stability: applying the round-trip twice converges ────────

mod roundtrip_stability {
    use super::*;

    fn assert_roundtrip_stable(scenario: IrConversation, a: Dialect, b: Dialect) {
        let first = run_roundtrip(&scenario, a, b);
        let second = run_roundtrip(&first, a, b);
        assert_eq!(
            first, second,
            "round-trip {a:?}<->{b:?} should stabilize after one pass"
        );
    }

    #[test]
    fn stable_openai_claude_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Claude);
    }

    #[test]
    fn stable_openai_gemini_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Gemini);
    }

    #[test]
    fn stable_claude_gemini_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::Claude, Dialect::Gemini);
    }

    #[test]
    fn stable_openai_kimi_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Kimi);
    }

    #[test]
    fn stable_openai_copilot_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Copilot);
    }

    #[test]
    fn stable_claude_kimi_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::Claude, Dialect::Kimi);
    }

    #[test]
    fn stable_gemini_kimi_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::Gemini, Dialect::Kimi);
    }

    #[test]
    fn stable_openai_codex_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::OpenAi, Dialect::Codex);
    }

    #[test]
    fn stable_codex_claude_simple() {
        assert_roundtrip_stable(simple_chat(), Dialect::Codex, Dialect::Claude);
    }

    #[test]
    fn stable_openai_claude_thinking() {
        assert_roundtrip_stable(thinking_scenario(), Dialect::OpenAi, Dialect::Claude);
    }

    #[test]
    fn stable_openai_gemini_tool_result() {
        assert_roundtrip_stable(tool_result_scenario(), Dialect::OpenAi, Dialect::Gemini);
    }

    #[test]
    fn stable_claude_kimi_tool_result() {
        assert_roundtrip_stable(tool_result_scenario(), Dialect::Claude, Dialect::Kimi);
    }
}

// ── Semantic preservation tests ─────────────────────────────────────────

mod semantic_preservation {
    use super::*;

    #[test]
    fn simple_text_preserved_openai_to_claude() {
        let mapped = run_mapping(simple_chat(), Dialect::OpenAi, Dialect::Claude);
        assert!(collect_all_text(&mapped).contains("Hello, how are you?"));
    }

    #[test]
    fn simple_text_preserved_openai_to_gemini() {
        let mapped = run_mapping(simple_chat(), Dialect::OpenAi, Dialect::Gemini);
        assert!(collect_all_text(&mapped).contains("Hello, how are you?"));
    }

    #[test]
    fn simple_text_preserved_claude_to_kimi() {
        let mapped = run_mapping(simple_chat(), Dialect::Claude, Dialect::Kimi);
        assert!(collect_all_text(&mapped).contains("Hello, how are you?"));
    }

    #[test]
    fn system_prompt_preserved_non_codex() {
        for &to in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Kimi,
            Dialect::Copilot,
        ] {
            let mapped = run_mapping(chat_with_system(), Dialect::OpenAi, to);
            let has_system = mapped.system_message().is_some();
            assert!(has_system, "system message lost mapping to {to:?}");
        }
    }

    #[test]
    fn system_prompt_dropped_by_codex() {
        let mapped = run_mapping(chat_with_system(), Dialect::OpenAi, Dialect::Codex);
        assert!(
            mapped.system_message().is_none(),
            "Codex should drop system messages"
        );
    }

    #[test]
    fn thinking_preserved_to_claude() {
        let mapped = run_mapping(thinking_scenario(), Dialect::OpenAi, Dialect::Claude);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(
            has_thinking,
            "thinking blocks should be preserved for Claude"
        );
    }

    #[test]
    fn thinking_dropped_to_openai() {
        let mapped = run_mapping(thinking_scenario(), Dialect::Claude, Dialect::OpenAi);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(
            !has_thinking,
            "thinking blocks should be dropped for OpenAI"
        );
    }

    #[test]
    fn thinking_dropped_to_gemini() {
        let mapped = run_mapping(thinking_scenario(), Dialect::Claude, Dialect::Gemini);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(
            !has_thinking,
            "thinking blocks should be dropped for Gemini"
        );
    }

    #[test]
    fn thinking_dropped_to_kimi() {
        let mapped = run_mapping(thinking_scenario(), Dialect::Claude, Dialect::Kimi);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(!has_thinking, "thinking blocks should be dropped for Kimi");
    }

    #[test]
    fn thinking_dropped_to_copilot() {
        let mapped = run_mapping(thinking_scenario(), Dialect::OpenAi, Dialect::Copilot);
        let has_thinking = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
        assert!(
            !has_thinking,
            "thinking blocks should be dropped for Copilot"
        );
    }

    #[test]
    fn thinking_text_content_still_present_after_drop() {
        let mapped = run_mapping(thinking_scenario(), Dialect::Claude, Dialect::OpenAi);
        let all_text = collect_all_text(&mapped);
        assert!(all_text.contains("Quantum computing uses qubits"));
    }

    #[test]
    fn tool_use_preserved_openai_to_claude() {
        let mapped = run_mapping(tool_use_scenario(), Dialect::OpenAi, Dialect::Claude);
        let tool_calls = mapped.tool_calls();
        assert_eq!(tool_calls.len(), 1, "should have one tool call");
        if let IrContentBlock::ToolUse { name, .. } = tool_calls[0] {
            assert_eq!(name, "get_weather");
        }
    }

    #[test]
    fn tool_use_preserved_openai_to_gemini() {
        let mapped = run_mapping(tool_use_scenario(), Dialect::OpenAi, Dialect::Gemini);
        let tool_calls = mapped.tool_calls();
        assert_eq!(tool_calls.len(), 1);
    }

    #[test]
    fn tool_use_dropped_to_codex() {
        let mapped = run_mapping(tool_use_scenario(), Dialect::OpenAi, Dialect::Codex);
        let tool_calls = mapped.tool_calls();
        assert!(tool_calls.is_empty(), "Codex should drop tool calls");
    }

    #[test]
    fn tool_role_becomes_user_for_claude() {
        let mapped = run_mapping(tool_result_scenario(), Dialect::OpenAi, Dialect::Claude);
        let tool_msgs = mapped.messages_by_role(IrRole::Tool);
        assert!(
            tool_msgs.is_empty(),
            "Claude should not have Tool-role messages"
        );
    }

    #[test]
    fn tool_role_becomes_user_for_gemini() {
        let mapped = run_mapping(tool_result_scenario(), Dialect::OpenAi, Dialect::Gemini);
        let tool_msgs = mapped.messages_by_role(IrRole::Tool);
        assert!(
            tool_msgs.is_empty(),
            "Gemini should not have Tool-role messages"
        );
    }

    #[test]
    fn user_tool_result_becomes_tool_role_for_openai() {
        // Claude-style: User message with ToolResult → OpenAI Tool role
        let claude_ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "search"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({}),
                }],
            ),
            IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "found it".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let mapped = run_mapping(claude_ir, Dialect::Claude, Dialect::OpenAi);
        let tool_msgs = mapped.messages_by_role(IrRole::Tool);
        assert!(
            !tool_msgs.is_empty(),
            "OpenAI should have Tool-role messages from Claude ToolResult"
        );
    }

    #[test]
    fn multi_tool_calls_preserved() {
        let mapped = run_mapping(multi_tool_scenario(), Dialect::OpenAi, Dialect::Claude);
        let tool_calls = mapped.tool_calls();
        assert_eq!(tool_calls.len(), 2, "both tool calls should be preserved");
    }

    #[test]
    fn unicode_exact_preservation() {
        let orig = unicode_scenario();
        let mapped = run_mapping(orig.clone(), Dialect::OpenAi, Dialect::Claude);
        assert_eq!(
            collect_all_text(&orig),
            collect_all_text(&mapped),
            "unicode must be preserved exactly"
        );
    }

    #[test]
    fn long_content_length_preserved() {
        let orig = long_content_scenario();
        let mapped = run_mapping(orig.clone(), Dialect::OpenAi, Dialect::Claude);
        assert_eq!(
            collect_all_text(&orig).len(),
            collect_all_text(&mapped).len(),
            "long content length must be preserved"
        );
    }

    #[test]
    fn metadata_preserved_through_mapping() {
        let orig = metadata_scenario();
        let mapped = run_mapping(orig.clone(), Dialect::OpenAi, Dialect::Claude);
        let orig_meta = &orig.messages[0].metadata;
        let mapped_meta = &mapped.messages[0].metadata;
        assert_eq!(orig_meta, mapped_meta, "metadata should be preserved");
    }

    #[test]
    fn empty_conversation_stays_empty() {
        for &to in Dialect::all() {
            let mapped = run_mapping(empty_scenario(), Dialect::OpenAi, to);
            assert!(mapped.is_empty(), "empty should stay empty for {to:?}");
        }
    }

    #[test]
    fn codex_drops_images() {
        let mapped = run_mapping(image_scenario(), Dialect::OpenAi, Dialect::Codex);
        let has_image = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::Image { .. }));
        assert!(!has_image, "Codex should drop image blocks");
    }

    #[test]
    fn codex_drops_tool_results() {
        let mapped = run_mapping(tool_result_scenario(), Dialect::OpenAi, Dialect::Codex);
        let has_tool_result = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::ToolResult { .. }));
        assert!(!has_tool_result, "Codex should drop tool result blocks");
    }
}

// ── Unsupported pair tests ──────────────────────────────────────────────

mod unsupported_pairs {
    use super::*;

    macro_rules! unsupported {
        ($name:ident, $from:expr, $to:expr) => {
            #[test]
            fn $name() {
                assert!(
                    default_ir_mapper($from, $to).is_none(),
                    "pair {:?}->{:?} should be unsupported",
                    $from,
                    $to
                );
            }
        };
    }

    unsupported!(codex_gemini, Dialect::Codex, Dialect::Gemini);
    unsupported!(gemini_codex, Dialect::Gemini, Dialect::Codex);
    unsupported!(codex_kimi, Dialect::Codex, Dialect::Kimi);
    unsupported!(kimi_codex, Dialect::Kimi, Dialect::Codex);
    unsupported!(codex_copilot, Dialect::Codex, Dialect::Copilot);
    unsupported!(copilot_codex, Dialect::Copilot, Dialect::Codex);
    unsupported!(copilot_claude, Dialect::Copilot, Dialect::Claude);
    unsupported!(claude_copilot, Dialect::Claude, Dialect::Copilot);
    unsupported!(copilot_gemini, Dialect::Copilot, Dialect::Gemini);
    unsupported!(gemini_copilot, Dialect::Gemini, Dialect::Copilot);
    unsupported!(copilot_kimi, Dialect::Copilot, Dialect::Kimi);
    unsupported!(kimi_copilot, Dialect::Kimi, Dialect::Copilot);
}

// ── Edge case tests ─────────────────────────────────────────────────────

mod edge_cases {
    use super::*;

    #[test]
    fn single_empty_message() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let _ = mapper.map_request(from, to, &ir);
        }
    }

    #[test]
    fn message_with_only_whitespace() {
        let ir = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "   \n\t  ")]);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        assert!(!mapped.is_empty());
    }

    #[test]
    fn tool_error_result() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "run this"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "execute".into(),
                    input: json!({"cmd": "fail"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "Error: command failed".into(),
                    }],
                    is_error: true,
                }],
            ),
        ]);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        let has_error_result = mapped
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .any(|b| matches!(b, IrContentBlock::ToolResult { is_error: true, .. }));
        assert!(has_error_result, "error flag should be preserved");
    }

    #[test]
    fn nested_tool_result_content() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![
                    IrContentBlock::Text {
                        text: "result line 1".into(),
                    },
                    IrContentBlock::Text {
                        text: "result line 2".into(),
                    },
                ],
                is_error: false,
            }],
        )]);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        assert!(!mapped.is_empty());
    }

    #[test]
    fn many_messages() {
        let messages: Vec<IrMessage> = (0..100)
            .map(|i| {
                if i % 2 == 0 {
                    IrMessage::text(IrRole::User, format!("Turn {i}"))
                } else {
                    IrMessage::text(IrRole::Assistant, format!("Reply {i}"))
                }
            })
            .collect();
        let ir = IrConversation::from_messages(messages);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(mapped.len(), 100);
    }

    #[test]
    fn special_characters_in_tool_name() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "my-tool_v2.0".into(),
                input: json!({}),
            }],
        )]);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        let calls = mapped.tool_calls();
        assert_eq!(calls.len(), 1);
        if let IrContentBlock::ToolUse { name, .. } = calls[0] {
            assert_eq!(name, "my-tool_v2.0");
        }
    }

    #[test]
    fn empty_tool_input() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "list_files".into(),
                input: json!({}),
            }],
        )]);
        for &to in &[Dialect::Claude, Dialect::Gemini, Dialect::Kimi] {
            let mapped = run_mapping(ir.clone(), Dialect::OpenAi, to);
            let calls = mapped.tool_calls();
            assert_eq!(calls.len(), 1, "tool call should be preserved for {to:?}");
        }
    }

    #[test]
    fn complex_tool_input() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "query".into(),
                input: json!({
                    "filters": [{"field": "name", "op": "eq", "value": "test"}],
                    "nested": {"deep": {"key": [1, 2, 3]}},
                    "unicode": "日本語",
                    "empty_array": [],
                    "null_val": null
                }),
            }],
        )]);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        if let IrContentBlock::ToolUse { input, .. } = &mapped.tool_calls()[0] {
            assert!(input.get("nested").is_some(), "nested input preserved");
            assert_eq!(input["unicode"], "日本語");
        }
    }

    #[test]
    fn codex_to_claude_rejects_apply_patch() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "apply_patch".into(),
                input: json!({}),
            }],
        )]);
        let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
        let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &ir);
        assert!(matches!(result, Err(MapError::UnmappableTool { .. })));
    }

    #[test]
    fn codex_to_claude_rejects_apply_diff() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "apply_diff".into(),
                input: json!({}),
            }],
        )]);
        let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
        let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &ir);
        assert!(matches!(result, Err(MapError::UnmappableTool { .. })));
    }

    #[test]
    fn claude_to_gemini_rejects_system_image() {
        let ir = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
            }],
        )]);
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
        let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &ir);
        assert!(matches!(result, Err(MapError::UnmappableContent { .. })));
    }

    #[test]
    fn unsupported_pair_returns_error() {
        let ir = simple_chat();
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        // Pass wrong dialect pair to mapper
        let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &ir);
        assert!(result.is_err(), "wrong pair should error");
    }
}

// ── Factory & meta tests ────────────────────────────────────────────────

mod factory_meta {
    use super::*;

    #[test]
    fn supported_pairs_includes_all_identity() {
        let pairs = supported_ir_pairs();
        for &d in Dialect::all() {
            assert!(pairs.contains(&(d, d)), "identity pair missing for {d:?}");
        }
    }

    #[test]
    fn supported_pairs_count() {
        let pairs = supported_ir_pairs();
        // 6 identity + 18 cross-dialect = 24
        assert_eq!(pairs.len(), 24);
    }

    #[test]
    fn all_supported_pairs_have_mappers() {
        for (from, to) in supported_ir_pairs() {
            assert!(
                default_ir_mapper(from, to).is_some(),
                "no mapper for {from:?} -> {to:?}"
            );
        }
    }

    #[test]
    fn mapper_supported_pairs_match_factory() {
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            let pairs = mapper.supported_pairs();
            assert!(
                pairs.contains(&(from, to)),
                "mapper does not list {from:?}->{to:?} in supported_pairs()"
            );
        }
    }

    #[test]
    fn both_map_request_and_map_response_succeed() {
        let ir = simple_chat();
        for (from, to) in supported_ir_pairs() {
            let mapper = default_ir_mapper(from, to).unwrap();
            mapper
                .map_request(from, to, &ir)
                .unwrap_or_else(|e| panic!("map_request {from:?}->{to:?}: {e}"));
            mapper
                .map_response(from, to, &ir)
                .unwrap_or_else(|e| panic!("map_response {from:?}->{to:?}: {e}"));
        }
    }

    #[test]
    fn ir_tool_definition_roundtrip() {
        let tool = IrToolDefinition {
            name: "get_weather".into(),
            description: "Get weather for a city".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        let back: IrToolDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn ir_conversation_serde_roundtrip() {
        let conv = tool_result_scenario();
        let json = serde_json::to_value(&conv).unwrap();
        let back: IrConversation = serde_json::from_value(json).unwrap();
        assert_eq!(conv, back);
    }
}

// ── Model selection & parameter tests ───────────────────────────────────

mod model_and_params {
    use super::*;

    fn conversation_with_model_metadata(model: &str) -> IrConversation {
        let mut metadata = BTreeMap::new();
        metadata.insert("model".into(), json!(model));
        IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
            metadata,
        }])
    }

    fn conversation_with_params(temp: f64, max_tokens: u64) -> IrConversation {
        let mut metadata = BTreeMap::new();
        metadata.insert("temperature".into(), json!(temp));
        metadata.insert("max_tokens".into(), json!(max_tokens));
        IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
            metadata,
        }])
    }

    #[test]
    fn model_metadata_preserved_openai_to_claude() {
        let ir = conversation_with_model_metadata("gpt-4");
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(mapped.messages[0].metadata["model"], json!("gpt-4"));
    }

    #[test]
    fn model_metadata_preserved_openai_to_gemini() {
        let ir = conversation_with_model_metadata("gpt-4o");
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Gemini);
        assert_eq!(mapped.messages[0].metadata["model"], json!("gpt-4o"));
    }

    #[test]
    fn model_metadata_preserved_claude_to_kimi() {
        let ir = conversation_with_model_metadata("claude-3-opus");
        let mapped = run_mapping(ir, Dialect::Claude, Dialect::Kimi);
        assert_eq!(mapped.messages[0].metadata["model"], json!("claude-3-opus"));
    }

    #[test]
    fn temperature_preserved_openai_to_claude() {
        let ir = conversation_with_params(0.7, 1024);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(mapped.messages[0].metadata["temperature"], json!(0.7));
        assert_eq!(mapped.messages[0].metadata["max_tokens"], json!(1024));
    }

    #[test]
    fn temperature_preserved_openai_to_gemini() {
        let ir = conversation_with_params(0.5, 2048);
        let mapped = run_mapping(ir, Dialect::OpenAi, Dialect::Gemini);
        assert_eq!(mapped.messages[0].metadata["temperature"], json!(0.5));
    }

    #[test]
    fn params_preserved_claude_to_openai() {
        let ir = conversation_with_params(0.9, 4096);
        let mapped = run_mapping(ir, Dialect::Claude, Dialect::OpenAi);
        assert_eq!(mapped.messages[0].metadata["temperature"], json!(0.9));
    }

    #[test]
    fn params_preserved_through_roundtrip() {
        let ir = conversation_with_params(0.3, 512);
        let rt = run_roundtrip(&ir, Dialect::OpenAi, Dialect::Claude);
        assert_eq!(rt.messages[0].metadata["temperature"], json!(0.3));
        assert_eq!(rt.messages[0].metadata["max_tokens"], json!(512));
    }

    #[test]
    fn model_metadata_survives_all_non_lossy_pairs() {
        let non_lossy_pairs: &[(Dialect, Dialect)] = &[
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::Gemini, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Kimi),
            (Dialect::Kimi, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Copilot),
            (Dialect::Copilot, Dialect::OpenAi),
        ];
        for &(from, to) in non_lossy_pairs {
            let ir = conversation_with_model_metadata("test-model");
            let mapped = run_mapping(ir, from, to);
            assert!(
                !mapped.is_empty(),
                "mapped should not be empty for {from:?}->{to:?}"
            );
            assert_eq!(
                mapped.messages[0].metadata.get("model"),
                Some(&json!("test-model")),
                "model metadata lost for {from:?}->{to:?}"
            );
        }
    }
}

// ── Streaming event simulation tests ────────────────────────────────────

mod streaming_events {
    use super::*;

    fn incremental_assistant_chunks() -> Vec<IrConversation> {
        vec![
            IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hello")]),
            IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hello, ")]),
            IrConversation::from_messages(vec![IrMessage::text(
                IrRole::Assistant,
                "Hello, world!",
            )]),
        ]
    }

    #[test]
    fn streaming_chunks_map_consistently_openai_claude() {
        for chunk in incremental_assistant_chunks() {
            let mapped = run_mapping(chunk.clone(), Dialect::OpenAi, Dialect::Claude);
            assert_eq!(
                collect_all_text(&chunk),
                collect_all_text(&mapped),
                "streaming chunk text must be preserved"
            );
        }
    }

    #[test]
    fn streaming_chunks_map_consistently_openai_gemini() {
        for chunk in incremental_assistant_chunks() {
            let mapped = run_mapping(chunk.clone(), Dialect::OpenAi, Dialect::Gemini);
            assert_eq!(collect_all_text(&chunk), collect_all_text(&mapped));
        }
    }

    #[test]
    fn streaming_chunks_map_consistently_openai_kimi() {
        for chunk in incremental_assistant_chunks() {
            let mapped = run_mapping(chunk.clone(), Dialect::OpenAi, Dialect::Kimi);
            assert_eq!(collect_all_text(&chunk), collect_all_text(&mapped));
        }
    }

    #[test]
    fn streaming_chunks_map_consistently_openai_copilot() {
        for chunk in incremental_assistant_chunks() {
            let mapped = run_mapping(chunk.clone(), Dialect::OpenAi, Dialect::Copilot);
            assert_eq!(collect_all_text(&chunk), collect_all_text(&mapped));
        }
    }

    #[test]
    fn streaming_chunks_map_consistently_claude_gemini() {
        for chunk in incremental_assistant_chunks() {
            let mapped = run_mapping(chunk.clone(), Dialect::Claude, Dialect::Gemini);
            assert_eq!(collect_all_text(&chunk), collect_all_text(&mapped));
        }
    }

    #[test]
    fn streaming_tool_call_chunk() {
        let chunk = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            }],
        )]);
        for &to in &[Dialect::Claude, Dialect::Gemini, Dialect::Kimi] {
            let mapped = run_mapping(chunk.clone(), Dialect::OpenAi, to);
            assert_eq!(
                mapped.tool_calls().len(),
                1,
                "streaming tool call should be preserved for {to:?}"
            );
        }
    }
}
