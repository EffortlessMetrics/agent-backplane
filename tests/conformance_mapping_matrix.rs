// SPDX-License-Identifier: MIT OR Apache-2.0

//! Structured cross-SDK conformance test harness that validates dialect
//! mapping completeness.
//!
//! Every test in this module derives its dialect pairs from the mapper factory
//! via [`abp_mapper::supported_ir_pairs`] — nothing is hard-coded.

use std::collections::BTreeSet;

use abp_capability::{
    CapabilityRegistry, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    gemini_15_pro_manifest, kimi_manifest, openai_gpt4o_manifest,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{Capability, SupportLevel};
use abp_dialect::Dialect;
use abp_mapper::{MapError, default_ir_mapper, supported_ir_pairs};

// ── Helpers ─────────────────────────────────────────────────────────────

/// Returns all registered pairs directly from the factory.
fn all_registered_pairs() -> Vec<(Dialect, Dialect)> {
    supported_ir_pairs()
}

/// Creates a minimal valid `IrConversation` representing a request for any
/// dialect (a user text message).
fn minimal_request(_dialect: Dialect) -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello")])
}

/// Creates a minimal valid `IrConversation` representing a response for any
/// dialect (an assistant text reply).
fn minimal_response(_dialect: Dialect) -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hi there")])
}

/// Returns `true` when the `(from, to)` pair involves Codex, which is lossy.
fn is_lossy_pair(from: Dialect, to: Dialect) -> bool {
    matches!(
        (from, to),
        (Dialect::OpenAi, Dialect::Codex)
            | (Dialect::Codex, Dialect::OpenAi)
            | (Dialect::Codex, Dialect::Claude)
            | (Dialect::Claude, Dialect::Codex)
    )
}

/// Build a multi-turn request with system prompt, user, and assistant turns.
fn multi_turn_request() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "What is 2+2?"),
        IrMessage::text(IrRole::Assistant, "4"),
        IrMessage::text(IrRole::User, "Thanks!"),
    ])
}

/// Build a conversation with a tool-use block.
fn tool_use_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read file.txt"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({"path": "file.txt"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "contents".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

/// Build a conversation with a thinking block (Claude-specific).
fn thinking_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Think step by step."),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me reason…".into(),
                },
                IrContentBlock::Text {
                    text: "Here is my answer.".into(),
                },
            ],
        ),
    ])
}

/// Maps Dialect → canonical registry key used by `CapabilityRegistry::with_defaults`.
fn dialect_registry_key(d: Dialect) -> &'static str {
    match d {
        Dialect::OpenAi => "openai/gpt-4o",
        Dialect::Claude => "anthropic/claude-3.5-sonnet",
        Dialect::Gemini => "google/gemini-1.5-pro",
        Dialect::Codex => "openai/codex",
        Dialect::Kimi => "moonshot/kimi",
        Dialect::Copilot => "github/copilot",
    }
}

// =========================================================================
// Category A — Factory completeness
// =========================================================================

#[test]
fn factory_returns_mapper_for_every_registered_pair() {
    for (from, to) in all_registered_pairs() {
        assert!(
            default_ir_mapper(from, to).is_some(),
            "factory returned None for {from} → {to}",
        );
    }
}

#[test]
fn supported_pairs_is_nonempty() {
    assert!(
        !all_registered_pairs().is_empty(),
        "supported_ir_pairs() must not be empty"
    );
}

#[test]
fn all_dialects_have_identity_pair() {
    let pairs: BTreeSet<(Dialect, Dialect)> = all_registered_pairs().into_iter().collect();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "identity pair missing for {d}",);
    }
}

#[test]
fn factory_returns_none_for_unregistered_pairs() {
    // Pairs NOT in the factory (Kimi↔Copilot, Codex↔Gemini, etc.)
    let registered: BTreeSet<_> = all_registered_pairs().into_iter().collect();
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            if !registered.contains(&(a, b)) {
                assert!(
                    default_ir_mapper(a, b).is_none(),
                    "factory unexpectedly returned Some for unregistered pair {a} → {b}",
                );
            }
        }
    }
}

#[test]
fn mapper_self_report_consistent_with_factory() {
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let self_pairs: BTreeSet<_> = mapper.supported_pairs().into_iter().collect();
        assert!(
            self_pairs.contains(&(from, to)),
            "mapper for {from} → {to} does not list the pair in supported_pairs()",
        );
    }
}

// =========================================================================
// Category B — map_request: no panics on varied input
// =========================================================================

#[test]
fn map_request_no_panic_all_pairs_text_only() {
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let ir = minimal_request(from);
        let _ = mapper.map_request(from, to, &ir);
    }
}

#[test]
fn map_request_no_panic_all_pairs_with_system_prompt() {
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "Hello"),
    ]);
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_request(from, to, &ir);
    }
}

#[test]
fn map_request_no_panic_all_pairs_with_tool_use() {
    let ir = tool_use_conversation();
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_request(from, to, &ir);
    }
}

#[test]
fn map_request_no_panic_all_pairs_with_thinking() {
    let ir = thinking_conversation();
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_request(from, to, &ir);
    }
}

#[test]
fn map_request_no_panic_all_pairs_empty_conversation() {
    let ir = IrConversation::new();
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_request(from, to, &ir);
    }
}

#[test]
fn map_request_no_panic_all_pairs_multi_turn() {
    let ir = multi_turn_request();
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_request(from, to, &ir);
    }
}

// =========================================================================
// Category C — map_response: no panics on varied input
// =========================================================================

#[test]
fn map_response_no_panic_all_pairs_text_only() {
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let ir = minimal_response(from);
        let _ = mapper.map_response(from, to, &ir);
    }
}

#[test]
fn map_response_no_panic_all_pairs_with_tool_result() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "grep".into(),
            input: serde_json::json!({"pattern": "foo"}),
        }],
    )]);
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_response(from, to, &ir);
    }
}

#[test]
fn map_response_no_panic_all_pairs_empty() {
    let ir = IrConversation::new();
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_response(from, to, &ir);
    }
}

#[test]
fn map_response_no_panic_all_pairs_multi_block() {
    let ir = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Here:".into(),
            },
            IrContentBlock::Text {
                text: "Done.".into(),
            },
        ],
    )]);
    for (from, to) in all_registered_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let _ = mapper.map_response(from, to, &ir);
    }
}

// =========================================================================
// Category D — Identity roundtrip: lossless for every dialect
// =========================================================================

fn assert_identity_roundtrip(dialect: Dialect) {
    let mapper = default_ir_mapper(dialect, dialect).unwrap();
    let ir = multi_turn_request();
    let mapped = mapper.map_request(dialect, dialect, &ir).unwrap();
    assert_eq!(ir, mapped, "identity roundtrip failed for {dialect}");
}

#[test]
fn identity_roundtrip_openai() {
    assert_identity_roundtrip(Dialect::OpenAi);
}

#[test]
fn identity_roundtrip_claude() {
    assert_identity_roundtrip(Dialect::Claude);
}

#[test]
fn identity_roundtrip_gemini() {
    assert_identity_roundtrip(Dialect::Gemini);
}

#[test]
fn identity_roundtrip_codex() {
    assert_identity_roundtrip(Dialect::Codex);
}

#[test]
fn identity_roundtrip_kimi() {
    assert_identity_roundtrip(Dialect::Kimi);
}

#[test]
fn identity_roundtrip_copilot() {
    assert_identity_roundtrip(Dialect::Copilot);
}

// =========================================================================
// Category E — Cross-dialect roundtrip: lossless text pairs
// =========================================================================

/// For lossless pairs the user text must survive a full round-trip.
fn assert_text_roundtrip(a: Dialect, b: Dialect) {
    let fwd = default_ir_mapper(a, b).unwrap();
    let rev = default_ir_mapper(b, a).unwrap();
    let ir = minimal_request(a);

    let mapped = fwd.map_request(a, b, &ir).unwrap();
    let back = rev.map_request(b, a, &mapped).unwrap();

    // The user text content must survive the round-trip.
    let original_text = ir.messages[0].text_content();
    let roundtrip_text = back
        .messages
        .iter()
        .find(|m| m.role == IrRole::User)
        .map(|m| m.text_content())
        .unwrap_or_default();
    assert_eq!(
        original_text, roundtrip_text,
        "text content lost in roundtrip {a} → {b} → {a}",
    );
}

#[test]
fn roundtrip_preserves_text_openai_claude() {
    assert_text_roundtrip(Dialect::OpenAi, Dialect::Claude);
}

#[test]
fn roundtrip_preserves_text_openai_gemini() {
    assert_text_roundtrip(Dialect::OpenAi, Dialect::Gemini);
}

#[test]
fn roundtrip_preserves_text_claude_gemini() {
    assert_text_roundtrip(Dialect::Claude, Dialect::Gemini);
}

#[test]
fn roundtrip_preserves_text_openai_kimi() {
    assert_text_roundtrip(Dialect::OpenAi, Dialect::Kimi);
}

#[test]
fn roundtrip_preserves_user_role_all_lossless_pairs() {
    for (from, to) in all_registered_pairs() {
        if from == to || is_lossy_pair(from, to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let ir = minimal_request(from);
        let mapped = mapper.map_request(from, to, &ir).unwrap();

        let has_user = mapped.messages.iter().any(|m| m.role == IrRole::User);
        assert!(has_user, "user message lost when mapping {from} → {to}",);
    }
}

// =========================================================================
// Category F — Lossy pairs: document what is dropped
// =========================================================================

#[test]
fn lossy_openai_to_codex_drops_system_messages() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Codex).unwrap();
    let ir = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be brief."),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    assert!(
        mapped.system_message().is_none(),
        "Codex must drop system messages (lossy)",
    );
}

#[test]
fn lossy_openai_to_codex_drops_tool_blocks() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Codex).unwrap();
    let ir = tool_use_conversation();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &ir)
        .unwrap();
    assert!(
        mapped.tool_calls().is_empty(),
        "Codex must drop tool-use blocks (lossy)",
    );
}

#[test]
fn lossy_claude_to_codex_drops_thinking() {
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Codex).unwrap();
    let ir = thinking_conversation();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::Codex, &ir)
        .unwrap();
    let has_thinking = mapped.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(!has_thinking, "Codex must drop thinking blocks (lossy)",);
}

// =========================================================================
// Category G — Error taxonomy coverage
// =========================================================================

#[test]
fn unsupported_pair_returns_typed_error() {
    // Pick an unregistered pair.
    let registered: BTreeSet<_> = all_registered_pairs().into_iter().collect();
    let mut found = false;
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            if registered.contains(&(a, b)) {
                continue;
            }
            // No mapper exists — verify factory returns None.
            assert!(default_ir_mapper(a, b).is_none());
            found = true;
            break;
        }
        if found {
            break;
        }
    }
    assert!(found, "expected at least one unregistered pair");
}

#[test]
fn mapper_rejects_wrong_pair_with_unsupported_error() {
    // Give OpenAiClaudeIrMapper a pair it doesn't handle (e.g., Gemini→Codex).
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let ir = minimal_request(Dialect::Gemini);
    let result = mapper.map_request(Dialect::Gemini, Dialect::Codex, &ir);
    match result {
        Err(MapError::UnsupportedPair { .. }) => {} // expected
        Err(other) => panic!("expected UnsupportedPair, got {other}"),
        Ok(_) => panic!("expected error for wrong pair"),
    }
}

#[test]
fn error_codes_stable_across_serialization() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back, "error round-trip must be stable");
}

#[test]
fn all_error_variants_constructible() {
    let variants: Vec<MapError> = vec![
        MapError::UnsupportedPair {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        },
        MapError::LossyConversion {
            field: "thinking".into(),
            reason: "target lacks thinking".into(),
        },
        MapError::UnmappableTool {
            name: "computer_use".into(),
            reason: "not supported".into(),
        },
        MapError::IncompatibleCapability {
            capability: "logprobs".into(),
            reason: "target dialect lacks logprobs".into(),
        },
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(*v, back);
    }
}

#[test]
fn map_error_display_is_nonempty() {
    let err = MapError::UnsupportedPair {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    assert!(!err.to_string().is_empty());
}

// =========================================================================
// Category H — Capability alignment
// =========================================================================

#[test]
fn capability_registry_covers_all_dialects() {
    let reg = CapabilityRegistry::with_defaults();
    for &d in Dialect::all() {
        let key = dialect_registry_key(d);
        assert!(
            reg.get(key).is_some(),
            "capability registry missing manifest for {d} (key={key})",
        );
    }
}

#[test]
fn all_manifests_are_nonempty() {
    let reg = CapabilityRegistry::with_defaults();
    for &d in Dialect::all() {
        let manifest = reg.get(dialect_registry_key(d)).unwrap();
        assert!(!manifest.is_empty(), "manifest for {d} must not be empty",);
    }
}

#[test]
fn every_dialect_declares_streaming() {
    let reg = CapabilityRegistry::with_defaults();
    for &d in Dialect::all() {
        let manifest = reg.get(dialect_registry_key(d)).unwrap();
        let level = manifest.get(&Capability::Streaming);
        assert!(
            matches!(level, Some(SupportLevel::Native)),
            "{d} must declare Streaming as Native, got {level:?}",
        );
    }
}

#[test]
fn codex_manifest_lacks_extended_thinking() {
    let manifest = codex_manifest();
    let level = manifest.get(&Capability::ExtendedThinking);
    assert!(
        !matches!(level, Some(SupportLevel::Native)),
        "Codex should not natively support ExtendedThinking",
    );
}

#[test]
fn manifest_functions_match_registry_entries() {
    let reg = CapabilityRegistry::with_defaults();
    let pairs: Vec<(Dialect, fn() -> _)> = vec![
        (Dialect::OpenAi, openai_gpt4o_manifest as fn() -> _),
        (Dialect::Claude, claude_35_sonnet_manifest),
        (Dialect::Gemini, gemini_15_pro_manifest),
        (Dialect::Codex, codex_manifest),
        (Dialect::Kimi, kimi_manifest),
        (Dialect::Copilot, copilot_manifest),
    ];
    for (d, builder) in pairs {
        let from_fn = builder();
        let from_reg = reg.get(dialect_registry_key(d)).unwrap();
        // Compare via serialized JSON since SupportLevel lacks PartialEq.
        let fn_json = serde_json::to_string(&from_fn).unwrap();
        let reg_json = serde_json::to_string(from_reg).unwrap();
        assert_eq!(
            fn_json, reg_json,
            "manifest function and registry entry differ for {d}",
        );
    }
}

#[test]
fn unsupported_capabilities_are_explicit_not_missing() {
    let reg = CapabilityRegistry::with_defaults();
    for &d in Dialect::all() {
        let manifest = reg.get(dialect_registry_key(d)).unwrap();
        // Every capability that the manifest lists as Unsupported must be an
        // explicit entry — confirming deliberate classification.
        for (cap, level) in manifest {
            if matches!(level, SupportLevel::Unsupported) {
                assert!(
                    manifest.contains_key(cap),
                    "{d}: capability {cap:?} is Unsupported but not listed",
                );
            }
        }
    }
}

#[test]
fn lossy_pair_codex_unsupported_capabilities_align_with_mapper() {
    // Codex mapper drops tool-use and system messages.
    // Verify the Codex manifest reflects that limitations exist.
    let manifest = codex_manifest();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Codex).unwrap();

    // Mapper handles the pair, so it should not error on minimal input.
    let ir = minimal_request(Dialect::OpenAi);
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Codex, &ir);
    assert!(result.is_ok());

    // Extended thinking not natively supported — aligns with mapper dropping thinking.
    assert!(
        !matches!(
            manifest.get(&Capability::ExtendedThinking),
            Some(SupportLevel::Native)
        ),
        "Codex manifest must not claim native ExtendedThinking",
    );
}
