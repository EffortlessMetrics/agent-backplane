#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the `abp-emulation` crate.
//!
//! Covers: EmulationStrategy enum variants, capability emulation, emulation
//! labeling, fidelity metrics, error handling, cross-dialect emulation, tool
//! emulation, streaming emulation, registry/lookup, configuration, compatibility
//! matrix, warnings/degradation notices, EmulationResult types, emulation chains.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{Capability, CapabilityRequirement, MinSupport};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy,
    FidelityLabel, apply_emulation, can_emulate, compute_fidelity, default_strategy,
    emulate_code_execution, emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output,
};
use std::collections::BTreeMap;

// ── Helpers ────────────────────────────────────────────────────────────

fn sys_user_conv(sys: &str, user: &str) -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, sys))
        .push(IrMessage::text(IrRole::User, user))
}

fn user_conv(msg: &str) -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, msg))
}

fn multi_turn() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Q1"))
        .push(IrMessage::text(IrRole::Assistant, "A1"))
        .push(IrMessage::text(IrRole::User, "Q2"))
        .push(IrMessage::text(IrRole::Assistant, "A2"))
        .push(IrMessage::text(IrRole::User, "Q3"))
}

fn emulatable() -> Vec<Capability> {
    vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ]
}

fn non_emulatable() -> Vec<Capability> {
    vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::McpClient,
        Capability::McpServer,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::PdfInput,
    ]
}

fn all_caps() -> Vec<Capability> {
    let mut v = emulatable();
    v.extend(non_emulatable());
    v
}

// ════════════════════════════════════════════════════════════════════════
// 1. EmulationStrategy enum variants and selection
// ════════════════════════════════════════════════════════════════════════

#[test]
fn strategy_variant_system_prompt_injection_construction() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "hello".into(),
    };
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn strategy_variant_post_processing_construction() {
    let s = EmulationStrategy::PostProcessing {
        detail: "truncate".into(),
    };
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn strategy_variant_disabled_construction() {
    let s = EmulationStrategy::Disabled {
        reason: "unsafe".into(),
    };
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn strategy_variants_are_distinct_via_ne() {
    let a = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let b = EmulationStrategy::PostProcessing { detail: "x".into() };
    let c = EmulationStrategy::Disabled { reason: "x".into() };
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
}

#[test]
fn strategy_eq_same_variant_same_data() {
    let a = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let b = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    assert_eq!(a, b);
}

#[test]
fn strategy_ne_same_variant_different_data() {
    let a = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let b = EmulationStrategy::SystemPromptInjection { prompt: "y".into() };
    assert_ne!(a, b);
}

#[test]
fn strategy_default_extended_thinking_is_system_prompt() {
    assert!(matches!(
        default_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn strategy_default_structured_output_is_post_processing() {
    assert!(matches!(
        default_strategy(&Capability::StructuredOutputJsonSchema),
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn strategy_default_code_execution_is_disabled() {
    assert!(matches!(
        default_strategy(&Capability::CodeExecution),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn strategy_default_image_input_is_system_prompt() {
    assert!(matches!(
        default_strategy(&Capability::ImageInput),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn strategy_default_stop_sequences_is_post_processing() {
    assert!(matches!(
        default_strategy(&Capability::StopSequences),
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn strategy_default_streaming_is_disabled() {
    assert!(matches!(
        default_strategy(&Capability::Streaming),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn strategy_default_tool_use_is_disabled() {
    assert!(matches!(
        default_strategy(&Capability::ToolUse),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn strategy_default_every_non_emulatable_is_disabled() {
    for cap in non_emulatable() {
        assert!(
            matches!(default_strategy(&cap), EmulationStrategy::Disabled { .. }),
            "{cap:?} should be Disabled"
        );
    }
}

#[test]
fn strategy_default_every_emulatable_is_not_disabled() {
    for cap in emulatable() {
        assert!(
            !matches!(default_strategy(&cap), EmulationStrategy::Disabled { .. }),
            "{cap:?} should NOT be Disabled"
        );
    }
}

#[test]
fn strategy_engine_resolve_uses_default_without_override() {
    let engine = EmulationEngine::with_defaults();
    for cap in all_caps() {
        assert_eq!(engine.resolve_strategy(&cap), default_strategy(&cap));
    }
}

#[test]
fn strategy_engine_resolve_prefers_config_override() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    assert!(matches!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn strategy_override_does_not_affect_other_caps() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    assert!(matches!(
        engine.resolve_strategy(&Capability::ImageInput),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 2. Capability emulation (when native feature not available)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn cap_emulation_can_emulate_true_for_all_emulatable() {
    for cap in emulatable() {
        assert!(can_emulate(&cap), "{cap:?}");
    }
}

#[test]
fn cap_emulation_can_emulate_false_for_all_non_emulatable() {
    for cap in non_emulatable() {
        assert!(!can_emulate(&cap), "{cap:?}");
    }
}

#[test]
fn cap_emulation_named_structured_output_mentions_json() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_structured_output() {
        assert!(prompt.contains("JSON"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn cap_emulation_named_code_execution_mentions_execute() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_code_execution() {
        assert!(prompt.to_lowercase().contains("execute code"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn cap_emulation_named_extended_thinking_mentions_step() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_extended_thinking() {
        assert!(prompt.contains("step by step"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn cap_emulation_named_image_input_mentions_image() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_image_input() {
        assert!(prompt.contains("Image"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn cap_emulation_named_stop_sequences_is_post_processing() {
    if let EmulationStrategy::PostProcessing { detail } = emulate_stop_sequences() {
        assert!(detail.contains("stop sequence"));
    } else {
        panic!("expected PostProcessing");
    }
}

#[test]
fn cap_emulation_all_named_strategies_unique() {
    let strats = vec![
        emulate_structured_output(),
        emulate_code_execution(),
        emulate_extended_thinking(),
        emulate_image_input(),
        emulate_stop_sequences(),
    ];
    for i in 0..strats.len() {
        for j in (i + 1)..strats.len() {
            assert_ne!(strats[i], strats[j], "indices {i} and {j}");
        }
    }
}

#[test]
fn cap_emulation_apply_injects_into_existing_system() {
    let mut conv = sys_user_conv("You are helpful.", "Hello");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    let text = sys.text_content();
    assert!(text.contains("Think step by step"));
    assert!(text.contains("You are helpful."));
}

#[test]
fn cap_emulation_apply_creates_system_if_missing() {
    let mut conv = user_conv("Hello");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("step by step"));
}

#[test]
fn cap_emulation_code_exec_disabled_reason_mentions_sandbox() {
    if let EmulationStrategy::Disabled { reason } = default_strategy(&Capability::CodeExecution) {
        assert!(reason.to_lowercase().contains("sandbox"));
    }
}

// ════════════════════════════════════════════════════════════════════════
// 3. Emulation labeling (emulated features must be labeled)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn labeling_every_applied_entry_has_non_disabled_strategy() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&emulatable(), &mut conv);
    for entry in &report.applied {
        assert!(!matches!(
            entry.strategy,
            EmulationStrategy::Disabled { .. }
        ));
    }
}

#[test]
fn labeling_applied_len_matches_emulatable_count() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&emulatable(), &mut conv);
    assert_eq!(report.applied.len(), emulatable().len());
}

#[test]
fn labeling_warnings_len_matches_non_emulatable_count() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&non_emulatable(), &mut conv);
    assert_eq!(report.warnings.len(), non_emulatable().len());
}

#[test]
fn labeling_applied_preserves_input_order() {
    let caps = vec![
        Capability::StopSequences,
        Capability::ImageInput,
        Capability::ExtendedThinking,
    ];
    let mut conv = user_conv("test");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&caps, &mut conv);
    for (i, cap) in caps.iter().enumerate() {
        assert_eq!(&report.applied[i].capability, cap);
    }
}

#[test]
fn labeling_warnings_contain_capability_debug_name() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::Streaming], &mut conv);
    assert!(report.warnings[0].contains("Streaming"));
}

#[test]
fn labeling_warnings_contain_not_emulated_text() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ToolBash], &mut conv);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn labeling_check_missing_matches_apply_labels() {
    let engine = EmulationEngine::with_defaults();
    let caps = all_caps();
    let check = engine.check_missing(&caps);
    let mut conv = user_conv("x");
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
}

#[test]
fn labeling_entry_capability_matches_requested() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    assert_eq!(report.applied[0].capability, Capability::ImageInput);
}

#[test]
fn labeling_entry_strategy_matches_resolved() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_conv("hi");
    let report = engine.apply(&[Capability::StopSequences], &mut conv);
    assert_eq!(
        report.applied[0].strategy,
        default_strategy(&Capability::StopSequences)
    );
}

// ════════════════════════════════════════════════════════════════════════
// 4. Emulation fidelity metrics
// ════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_native_caps_labeled_native() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);
    assert_eq!(labels.len(), 2);
    assert!(labels.values().all(|l| *l == FidelityLabel::Native));
}

#[test]
fn fidelity_emulated_caps_labeled_emulated() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_mixed_native_and_emulated() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ImageInput],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_warnings_excluded_from_labels() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_emulated_overrides_native_for_same_cap() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let labels = compute_fidelity(&[Capability::ExtendedThinking], &report);
    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_empty_inputs_empty_labels() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_label_carries_strategy_detail() {
    let mut conv = user_conv("x");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);
    let labels = compute_fidelity(&[], &report);
    if let FidelityLabel::Emulated { strategy } = &labels[&Capability::StopSequences] {
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    } else {
        panic!("expected Emulated");
    }
}

#[test]
fn fidelity_deterministic_serialization() {
    let mut conv1 = user_conv("a");
    let mut conv2 = user_conv("a");
    let engine = EmulationEngine::with_defaults();
    let r1 = engine.apply(&emulatable(), &mut conv1);
    let r2 = engine.apply(&emulatable(), &mut conv2);
    let l1 = compute_fidelity(&[Capability::Streaming], &r1);
    let l2 = compute_fidelity(&[Capability::Streaming], &r2);
    assert_eq!(
        serde_json::to_string(&l1).unwrap(),
        serde_json::to_string(&l2).unwrap()
    );
}

#[test]
fn fidelity_all_emulatable_produce_emulated_labels() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&emulatable(), &mut conv);
    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), emulatable().len());
    for label in labels.values() {
        assert!(matches!(label, FidelityLabel::Emulated { .. }));
    }
}

#[test]
fn fidelity_multiple_native_caps() {
    let report = EmulationReport::default();
    let native = vec![
        Capability::Streaming,
        Capability::ToolUse,
        Capability::ToolRead,
    ];
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 3);
    for label in labels.values() {
        assert_eq!(*label, FidelityLabel::Native);
    }
}

// ════════════════════════════════════════════════════════════════════════
// 5. Emulation error handling (fail-early)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn error_disabled_produces_warning_not_panic() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_conv("x");
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(!report.warnings.is_empty());
}

#[test]
fn error_all_disabled_yields_all_warnings() {
    let engine = EmulationEngine::with_defaults();
    let disabled = non_emulatable();
    let mut conv = user_conv("x");
    let report = engine.apply(&disabled, &mut conv);
    assert_eq!(report.warnings.len(), disabled.len());
    assert!(report.applied.is_empty());
}

#[test]
fn error_check_missing_same_warning_count() {
    let engine = EmulationEngine::with_defaults();
    let check = engine.check_missing(&[Capability::CodeExecution, Capability::Streaming]);
    assert_eq!(check.warnings.len(), 2);
}

#[test]
fn error_override_to_disabled_produces_warning() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_conv("x");
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("user disabled"));
}

#[test]
fn error_custom_disabled_reason_appears_in_warning() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ImageInput,
        EmulationStrategy::Disabled {
            reason: "images not allowed by policy".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let report = engine.check_missing(&[Capability::ImageInput]);
    assert!(report.warnings[0].contains("images not allowed by policy"));
}

#[test]
fn error_empty_conversation_still_produces_report() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn error_report_is_empty_when_no_caps_requested() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn error_disabled_caps_do_not_mutate_conversation() {
    let engine = EmulationEngine::with_defaults();
    for cap in non_emulatable() {
        let original = user_conv("x");
        let mut conv = original.clone();
        engine.apply(std::slice::from_ref(&cap), &mut conv);
        assert_eq!(conv, original, "{cap:?} mutated the conversation");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 6. Cross-dialect emulation (Claude features on OpenAI, etc.)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_emulate_thinking_on_text_only_backend() {
    let mut conv = user_conv("Explain quantum physics");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(
        conv.system_message()
            .unwrap()
            .text_content()
            .contains("step by step")
    );
}

#[test]
fn cross_dialect_emulate_structured_output_on_non_json_backend() {
    let mut conv = user_conv("Give me JSON");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn cross_dialect_emulate_image_input_on_text_only_backend() {
    let mut conv = user_conv("Describe the image");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(
        conv.system_message()
            .unwrap()
            .text_content()
            .contains("Image")
    );
}

#[test]
fn cross_dialect_multiple_caps_on_minimal_backend() {
    let mut conv = user_conv("do stuff");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::ImageInput,
            Capability::StopSequences,
            Capability::StructuredOutputJsonSchema,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn cross_dialect_custom_strategy_enables_tool_use() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ToolUse,
        EmulationStrategy::SystemPromptInjection {
            prompt: "You can call functions by outputting JSON in a specific format.".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_conv("Call a tool");
    let report = engine.apply(&[Capability::ToolUse], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn cross_dialect_mixed_supported_and_unsupported() {
    let mut conv = user_conv("do");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::CodeExecution,
            Capability::ImageInput,
            Capability::ToolBash,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 2);
}

// ════════════════════════════════════════════════════════════════════════
// 7. Tool emulation (mapping tool calling across dialects)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn tool_emulation_default_tool_use_disabled() {
    assert!(!can_emulate(&Capability::ToolUse));
}

#[test]
fn tool_emulation_default_tool_read_disabled() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn tool_emulation_default_tool_write_disabled() {
    assert!(!can_emulate(&Capability::ToolWrite));
}

#[test]
fn tool_emulation_default_tool_edit_disabled() {
    assert!(!can_emulate(&Capability::ToolEdit));
}

#[test]
fn tool_emulation_default_tool_bash_disabled() {
    assert!(!can_emulate(&Capability::ToolBash));
}

#[test]
fn tool_emulation_default_tool_glob_disabled() {
    assert!(!can_emulate(&Capability::ToolGlob));
}

#[test]
fn tool_emulation_default_tool_grep_disabled() {
    assert!(!can_emulate(&Capability::ToolGrep));
}

#[test]
fn tool_emulation_override_tool_use_enables_emulation() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ToolUse,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Function calling: output JSON with function_name and args.".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let s = engine.resolve_strategy(&Capability::ToolUse);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn tool_emulation_preserves_existing_tool_blocks() {
    let tool_msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "read_file".into(),
            input: serde_json::json!({"path": "a.txt"}),
        }],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Read"))
        .push(tool_msg);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.tool_calls().len(), 1);
}

#[test]
fn tool_emulation_all_tool_caps_disabled_by_default() {
    let tool_caps = vec![
        Capability::ToolUse,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
    ];
    for cap in &tool_caps {
        assert!(
            matches!(default_strategy(cap), EmulationStrategy::Disabled { .. }),
            "{cap:?}"
        );
    }
}

#[test]
fn tool_emulation_override_multiple_tool_caps() {
    let mut cfg = EmulationConfig::new();
    let tool_caps = vec![Capability::ToolRead, Capability::ToolWrite];
    for cap in &tool_caps {
        cfg.set(
            cap.clone(),
            EmulationStrategy::SystemPromptInjection {
                prompt: format!("emulate {cap:?}"),
            },
        );
    }
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_conv("x");
    let report = engine.apply(&tool_caps, &mut conv);
    assert_eq!(report.applied.len(), 2);
    assert!(report.warnings.is_empty());
}

// ════════════════════════════════════════════════════════════════════════
// 8. Streaming emulation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_default_is_disabled() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn streaming_disabled_reason_in_warning() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = user_conv("x");
    let report = engine.apply(&[Capability::Streaming], &mut conv);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("Streaming"));
}

#[test]
fn streaming_override_enables_emulation() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing {
            detail: "Chunk response into simulated streaming tokens".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_conv("hi");
    let report = engine.apply(&[Capability::Streaming], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn streaming_override_does_not_mutate_conversation() {
    let original = user_conv("hi");
    let mut conv = original.clone();
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing {
            detail: "chunk".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    engine.apply(&[Capability::Streaming], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn streaming_fidelity_label_when_emulated() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing {
            detail: "chunk".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_conv("x");
    let report = engine.apply(&[Capability::Streaming], &mut conv);
    let labels = compute_fidelity(&[], &report);
    assert!(matches!(
        labels[&Capability::Streaming],
        FidelityLabel::Emulated { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 9. Emulation registry and lookup
// ════════════════════════════════════════════════════════════════════════

#[test]
fn registry_config_stores_and_retrieves_strategy() {
    let mut cfg = EmulationConfig::new();
    let strategy = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    cfg.set(Capability::ExtendedThinking, strategy.clone());
    assert_eq!(
        cfg.strategies.get(&Capability::ExtendedThinking),
        Some(&strategy)
    );
}

#[test]
fn registry_config_absent_key_returns_none() {
    let cfg = EmulationConfig::new();
    assert!(!cfg.strategies.contains_key(&Capability::Streaming));
}

#[test]
fn registry_engine_lookup_all_caps() {
    let engine = EmulationEngine::with_defaults();
    for cap in all_caps() {
        // Should not panic
        let _ = engine.resolve_strategy(&cap);
    }
}

#[test]
fn registry_check_missing_acts_as_dry_run() {
    let engine = EmulationEngine::with_defaults();
    let caps = emulatable();
    let check = engine.check_missing(&caps);
    assert_eq!(check.applied.len(), caps.len());
    assert!(check.warnings.is_empty());
}

#[test]
fn registry_check_missing_disabled_caps() {
    let engine = EmulationEngine::with_defaults();
    let caps = non_emulatable();
    let check = engine.check_missing(&caps);
    assert!(check.applied.is_empty());
    assert_eq!(check.warnings.len(), caps.len());
}

#[test]
fn registry_config_btreemap_sorted_keys() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ToolUse,
        EmulationStrategy::Disabled { reason: "a".into() },
    );
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::Disabled { reason: "b".into() },
    );
    let keys: Vec<_> = cfg.strategies.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

// ════════════════════════════════════════════════════════════════════════
// 10. Emulation configuration
// ════════════════════════════════════════════════════════════════════════

#[test]
fn config_new_is_empty() {
    let cfg = EmulationConfig::new();
    assert!(cfg.strategies.is_empty());
}

#[test]
fn config_default_is_empty() {
    let cfg = EmulationConfig::default();
    assert!(cfg.strategies.is_empty());
}

#[test]
fn config_set_inserts_strategy() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::Disabled {
            reason: "no".into(),
        },
    );
    assert_eq!(cfg.strategies.len(), 1);
    assert!(cfg.strategies.contains_key(&Capability::Streaming));
}

#[test]
fn config_set_overwrites_previous() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::Disabled {
            reason: "first".into(),
        },
    );
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::SystemPromptInjection {
            prompt: "second".into(),
        },
    );
    assert_eq!(cfg.strategies.len(), 1);
    assert!(matches!(
        cfg.strategies[&Capability::Streaming],
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn config_multiple_overrides() {
    let mut cfg = EmulationConfig::new();
    for cap in non_emulatable().into_iter().take(5) {
        cfg.set(
            cap,
            EmulationStrategy::SystemPromptInjection {
                prompt: "override".into(),
            },
        );
    }
    assert_eq!(cfg.strategies.len(), 5);
}

#[test]
fn config_enable_previously_disabled_cap() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate code execution.".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let mut conv = user_conv("run code");
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn config_btreemap_preserves_deterministic_order() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ToolUse,
        EmulationStrategy::Disabled { reason: "a".into() },
    );
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::Disabled { reason: "b".into() },
    );
    let j1 = serde_json::to_string(&cfg).unwrap();
    let j2 = serde_json::to_string(&cfg).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn config_engine_with_defaults_equivalent_to_empty_config() {
    let e1 = EmulationEngine::with_defaults();
    let e2 = EmulationEngine::new(EmulationConfig::new());
    for cap in all_caps() {
        assert_eq!(e1.resolve_strategy(&cap), e2.resolve_strategy(&cap));
    }
}

#[test]
fn config_serde_roundtrip_empty() {
    let cfg = EmulationConfig::new();
    let json = serde_json::to_string(&cfg).unwrap();
    let d: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, d);
}

#[test]
fn config_serde_roundtrip_with_overrides() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "think".into(),
        },
    );
    cfg.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "no".into(),
        },
    );
    cfg.set(
        Capability::StopSequences,
        EmulationStrategy::PostProcessing {
            detail: "truncate".into(),
        },
    );
    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let d: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, d);
}

// ════════════════════════════════════════════════════════════════════════
// 11. Emulation compatibility matrix
// ════════════════════════════════════════════════════════════════════════

#[test]
fn compat_matrix_all_caps_covered() {
    for cap in all_caps() {
        let _ = default_strategy(&cap);
        let _ = can_emulate(&cap);
    }
}

#[test]
fn compat_matrix_emulatable_are_exactly_four() {
    let e = emulatable();
    assert_eq!(e.len(), 4);
}

#[test]
fn compat_matrix_every_cap_either_emulatable_or_disabled() {
    for cap in all_caps() {
        let s = default_strategy(&cap);
        match &s {
            EmulationStrategy::Disabled { .. } => assert!(!can_emulate(&cap)),
            _ => assert!(can_emulate(&cap)),
        }
    }
}

#[test]
fn compat_matrix_system_prompt_caps() {
    let spi_caps = vec![Capability::ExtendedThinking, Capability::ImageInput];
    for cap in &spi_caps {
        assert!(matches!(
            default_strategy(cap),
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }
}

#[test]
fn compat_matrix_post_processing_caps() {
    let pp_caps = vec![
        Capability::StructuredOutputJsonSchema,
        Capability::StopSequences,
    ];
    for cap in &pp_caps {
        assert!(matches!(
            default_strategy(cap),
            EmulationStrategy::PostProcessing { .. }
        ));
    }
}

#[test]
fn compat_matrix_requirement_native_vs_emulated() {
    let req_native = CapabilityRequirement {
        capability: Capability::ExtendedThinking,
        min_support: MinSupport::Native,
    };
    let req_emulated = CapabilityRequirement {
        capability: Capability::ExtendedThinking,
        min_support: MinSupport::Emulated,
    };
    // Both create valid requirements, different min_support thresholds
    assert!(matches!(req_native.min_support, MinSupport::Native));
    assert!(matches!(req_emulated.min_support, MinSupport::Emulated));
}

// ════════════════════════════════════════════════════════════════════════
// 12. Emulation warnings and degradation notices
// ════════════════════════════════════════════════════════════════════════

#[test]
fn warning_disabled_caps_always_produce_warnings() {
    let engine = EmulationEngine::with_defaults();
    for cap in non_emulatable() {
        let mut conv = user_conv("x");
        let report = engine.apply(std::slice::from_ref(&cap), &mut conv);
        assert!(
            !report.warnings.is_empty(),
            "{cap:?} must produce a warning"
        );
    }
}

#[test]
fn warning_disabled_caps_never_in_applied() {
    let engine = EmulationEngine::with_defaults();
    for cap in non_emulatable() {
        let mut conv = user_conv("x");
        let report = engine.apply(std::slice::from_ref(&cap), &mut conv);
        assert!(report.applied.is_empty(), "{cap:?} in applied");
    }
}

#[test]
fn warning_emulated_caps_never_produce_warnings() {
    let engine = EmulationEngine::with_defaults();
    for cap in emulatable() {
        let mut conv = user_conv("x");
        let report = engine.apply(std::slice::from_ref(&cap), &mut conv);
        assert!(report.warnings.is_empty(), "{cap:?} should not warn");
    }
}

#[test]
fn warning_has_unemulatable_true_for_disabled() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.has_unemulatable());
}

#[test]
fn warning_has_unemulatable_false_for_emulatable() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert!(!report.has_unemulatable());
}

#[test]
fn warning_mixed_report_has_unemulatable() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);
    assert!(report.has_unemulatable());
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn warning_text_mentions_reason() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::Streaming,
        EmulationStrategy::Disabled {
            reason: "backend has no SSE support".into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let report = engine.check_missing(&[Capability::Streaming]);
    assert!(report.warnings[0].contains("backend has no SSE support"));
}

// ════════════════════════════════════════════════════════════════════════
// 13. EmulationResult types and conversion
// ════════════════════════════════════════════════════════════════════════

#[test]
fn result_type_report_default_is_empty() {
    assert!(EmulationReport::default().is_empty());
}

#[test]
fn result_type_report_not_empty_with_applied() {
    let r = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    assert!(!r.is_empty());
}

#[test]
fn result_type_report_not_empty_with_warnings() {
    let r = EmulationReport {
        applied: vec![],
        warnings: vec!["warn".into()],
    };
    assert!(!r.is_empty());
}

#[test]
fn result_type_entry_serde_roundtrip() {
    let e = EmulationEntry {
        capability: Capability::ImageInput,
        strategy: emulate_image_input(),
    };
    let json = serde_json::to_string(&e).unwrap();
    let d: EmulationEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(e, d);
}

#[test]
fn result_type_report_serde_roundtrip() {
    let r = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "think".into(),
                },
            },
            EmulationEntry {
                capability: Capability::StopSequences,
                strategy: EmulationStrategy::PostProcessing {
                    detail: "stop".into(),
                },
            },
        ],
        warnings: vec!["w1".into(), "w2".into()],
    };
    let json = serde_json::to_string(&r).unwrap();
    let d: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(r, d);
}

#[test]
fn result_type_fidelity_native_roundtrip() {
    let l = FidelityLabel::Native;
    let json = serde_json::to_string(&l).unwrap();
    assert!(json.contains("\"fidelity\":\"native\""));
    let d: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, d);
}

#[test]
fn result_type_fidelity_emulated_roundtrip() {
    let l = FidelityLabel::Emulated {
        strategy: EmulationStrategy::PostProcessing { detail: "x".into() },
    };
    let json = serde_json::to_string(&l).unwrap();
    assert!(json.contains("\"fidelity\":\"emulated\""));
    let d: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(l, d);
}

#[test]
fn result_type_strategy_serde_type_tags() {
    let spi = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let pp = EmulationStrategy::PostProcessing { detail: "x".into() };
    let dis = EmulationStrategy::Disabled { reason: "x".into() };
    assert!(
        serde_json::to_string(&spi)
            .unwrap()
            .contains("\"type\":\"system_prompt_injection\"")
    );
    assert!(
        serde_json::to_string(&pp)
            .unwrap()
            .contains("\"type\":\"post_processing\"")
    );
    assert!(
        serde_json::to_string(&dis)
            .unwrap()
            .contains("\"type\":\"disabled\"")
    );
}

#[test]
fn result_type_deserialization_from_known_json() {
    let json = r#"{"type":"system_prompt_injection","prompt":"test prompt"}"#;
    let s: EmulationStrategy = serde_json::from_str(json).unwrap();
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert_eq!(prompt, "test prompt");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn result_type_fidelity_map_roundtrip() {
    let mut conv = user_conv("x");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&emulatable(), &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    let json = serde_json::to_string(&labels).unwrap();
    let d: BTreeMap<Capability, FidelityLabel> = serde_json::from_str(&json).unwrap();
    assert_eq!(labels, d);
}

// ════════════════════════════════════════════════════════════════════════
// 14. Emulation chain (multiple emulation layers)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn chain_apply_twice_accumulates_system_content() {
    let mut conv = sys_user_conv("base", "hello");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    engine.apply(&[Capability::ImageInput], &mut conv);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.content.len(), 3);
    let full = sys.text_content();
    assert!(full.contains("step by step"));
    assert!(full.contains("Image"));
}

#[test]
fn chain_second_apply_independent_report() {
    let mut conv = sys_user_conv("base", "hi");
    let engine = EmulationEngine::with_defaults();
    let r1 = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let r2 = engine.apply(&[Capability::ImageInput], &mut conv);
    assert_eq!(r1.applied.len(), 1);
    assert_eq!(r2.applied.len(), 1);
    assert_eq!(r1.applied[0].capability, Capability::ExtendedThinking);
    assert_eq!(r2.applied[0].capability, Capability::ImageInput);
}

#[test]
fn chain_different_engines_applied_sequentially() {
    let mut cfg1 = EmulationConfig::new();
    cfg1.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Step 1: code emulation".into(),
        },
    );
    let mut cfg2 = EmulationConfig::new();
    cfg2.set(
        Capability::Streaming,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Step 2: streaming emulation".into(),
        },
    );

    let mut conv = user_conv("run");
    EmulationEngine::new(cfg1).apply(&[Capability::CodeExecution], &mut conv);
    EmulationEngine::new(cfg2).apply(&[Capability::Streaming], &mut conv);

    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Step 1"));
    assert!(sys.contains("Step 2"));
}

#[test]
fn chain_fidelity_merges_across_passes() {
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::with_defaults();
    let r1 = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let r2 = engine.apply(&[Capability::ImageInput], &mut conv);

    let mut combined = EmulationReport::default();
    combined.applied.extend(r1.applied);
    combined.applied.extend(r2.applied);

    let labels = compute_fidelity(&[Capability::Streaming], &combined);
    assert_eq!(labels.len(), 3);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
    assert!(matches!(
        labels[&Capability::ImageInput],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn chain_three_passes_all_inject_into_same_system_msg() {
    let mut conv = sys_user_conv("base", "hi");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    engine.apply(&[Capability::ImageInput], &mut conv);

    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ToolUse,
        EmulationStrategy::SystemPromptInjection {
            prompt: "tool emulation".into(),
        },
    );
    EmulationEngine::new(cfg).apply(&[Capability::ToolUse], &mut conv);

    let sys = conv.system_message().unwrap();
    assert_eq!(sys.content.len(), 4);
}

#[test]
fn chain_post_processing_then_injection() {
    let mut conv = sys_user_conv("sys", "user");
    let engine = EmulationEngine::with_defaults();
    let r1 = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    let r2 = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(matches!(
        r1.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
    assert!(matches!(
        r2.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert_eq!(conv.system_message().unwrap().content.len(), 2);
}

#[test]
fn chain_four_emulatable_caps_one_by_one() {
    let mut conv = sys_user_conv("base", "q");
    let engine = EmulationEngine::with_defaults();
    let mut total_applied = 0;
    for cap in emulatable() {
        let report = engine.apply(&[cap], &mut conv);
        total_applied += report.applied.len();
    }
    assert_eq!(total_applied, emulatable().len());
}

// ════════════════════════════════════════════════════════════════════════
// 15. Additional coverage: free function, traits, edge cases
// ════════════════════════════════════════════════════════════════════════

#[test]
fn free_fn_apply_with_default_config() {
    let cfg = EmulationConfig::new();
    let mut conv = sys_user_conv("sys", "usr");
    let report = apply_emulation(&cfg, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn free_fn_apply_with_custom_config() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "simulate".into(),
        },
    );
    let mut conv = user_conv("x");
    let report = apply_emulation(&cfg, &[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(conv.system_message().is_some());
}

#[test]
fn free_fn_apply_empty_caps_noop() {
    let cfg = EmulationConfig::new();
    let original = user_conv("x");
    let mut conv = original.clone();
    let report = apply_emulation(&cfg, &[], &mut conv);
    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn trait_engine_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<EmulationEngine>();
}

#[test]
fn trait_engine_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<EmulationEngine>();
}

#[test]
fn trait_config_is_send_sync() {
    fn assert_ss<T: Send + Sync>() {}
    assert_ss::<EmulationConfig>();
}

#[test]
fn trait_report_is_send_sync() {
    fn assert_ss<T: Send + Sync>() {}
    assert_ss::<EmulationReport>();
}

#[test]
fn trait_strategy_is_send_sync() {
    fn assert_ss<T: Send + Sync>() {}
    assert_ss::<EmulationStrategy>();
}

#[test]
fn trait_fidelity_is_send_sync() {
    fn assert_ss<T: Send + Sync>() {}
    assert_ss::<FidelityLabel>();
}

#[test]
fn clone_engine_independence() {
    let e1 = EmulationEngine::with_defaults();
    let e2 = e1.clone();
    let mut c1 = user_conv("a");
    let mut c2 = user_conv("a");
    let r1 = e1.apply(&[Capability::ExtendedThinking], &mut c1);
    let r2 = e2.apply(&[Capability::ExtendedThinking], &mut c2);
    assert_eq!(r1.applied.len(), r2.applied.len());
    assert_eq!(c1, c2);
}

#[test]
fn clone_config_independence() {
    let mut cfg1 = EmulationConfig::new();
    cfg1.set(
        Capability::Streaming,
        EmulationStrategy::Disabled { reason: "a".into() },
    );
    let mut cfg2 = cfg1.clone();
    cfg2.set(
        Capability::Streaming,
        EmulationStrategy::SystemPromptInjection { prompt: "b".into() },
    );
    assert!(matches!(
        cfg1.strategies[&Capability::Streaming],
        EmulationStrategy::Disabled { .. }
    ));
    assert!(matches!(
        cfg2.strategies[&Capability::Streaming],
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 16. Edge cases
// ════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_conversation_system_prompt_created() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn edge_apply_to_conversation_with_only_assistant() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::Assistant, "response"));
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
}

#[test]
fn edge_large_prompt_injection() {
    let long_prompt = "x".repeat(10_000);
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: long_prompt.clone(),
        },
    );
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::new(cfg);
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains(&long_prompt));
}

#[test]
fn edge_unicode_in_strategy_prompt() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "思考してください 🤔".into(),
        },
    );
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::new(cfg);
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("🤔"));
}

#[test]
fn edge_empty_string_prompt_injection() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: String::new(),
        },
    );
    let mut conv = user_conv("hi");
    let engine = EmulationEngine::new(cfg);
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn edge_special_chars_in_disabled_reason() {
    let mut cfg = EmulationConfig::new();
    cfg.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: r#"reason with "quotes" and \backslash"#.into(),
        },
    );
    let engine = EmulationEngine::new(cfg);
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert!(report.warnings[0].contains("quotes"));
}

#[test]
fn edge_many_caps_repeated() {
    let caps: Vec<Capability> = (0..50).map(|_| Capability::ExtendedThinking).collect();
    let mut conv = sys_user_conv("sys", "usr");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied.len(), 50);
    assert_eq!(conv.system_message().unwrap().content.len(), 51);
}

#[test]
fn edge_tool_result_blocks_preserved() {
    let tool_result = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text {
                text: "result data".into(),
            }],
            is_error: false,
        }],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "q"))
        .push(tool_result);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let tool_msg = &conv.messages[2];
    assert!(matches!(
        tool_msg.content[0],
        IrContentBlock::ToolResult { .. }
    ));
}

#[test]
fn edge_message_count_grows_only_by_system_insertion() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::User, "b"))
        .push(IrMessage::text(IrRole::User, "c"));
    assert_eq!(conv.len(), 3);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&emulatable(), &mut conv);
    assert_eq!(conv.len(), 4);
}

#[test]
fn edge_preserves_thinking_blocks() {
    let asst = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "thinking...".into(),
            },
            IrContentBlock::Text {
                text: "answer".into(),
            },
        ],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "q"))
        .push(asst);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let a = conv.last_assistant().unwrap();
    assert!(matches!(a.content[0], IrContentBlock::Thinking { .. }));
}

#[test]
fn edge_preserves_image_blocks() {
    let img = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "desc".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
        ],
    );
    let mut conv = IrConversation::new().push(img);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ImageInput], &mut conv);
    let user = &conv.messages[1];
    assert!(matches!(user.content[1], IrContentBlock::Image { .. }));
}

#[test]
fn edge_preserves_message_metadata() {
    let mut msg = IrMessage::text(IrRole::User, "hi");
    msg.metadata.insert("k".into(), serde_json::json!("v"));
    let mut conv = IrConversation::new().push(msg);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(
        conv.messages[1].metadata.get("k"),
        Some(&serde_json::json!("v"))
    );
}

#[test]
fn edge_newline_prefix_on_injected_text() {
    let mut conv = sys_user_conv("base", "hi");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    if let IrContentBlock::Text { text } = &sys.content[1] {
        assert!(text.starts_with('\n'));
    }
}

#[test]
fn edge_preserves_original_system_text() {
    let mut conv = sys_user_conv("Original instructions.", "hi");
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    if let IrContentBlock::Text { text } = &sys.content[0] {
        assert_eq!(text, "Original instructions.");
    }
}

#[test]
fn edge_overhead_system_prompt_injection_adds_one_content_block() {
    let mut conv = sys_user_conv("base", "hi");
    let orig_blocks = conv.system_message().unwrap().content.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(
        conv.system_message().unwrap().content.len(),
        orig_blocks + 1
    );
}

#[test]
fn edge_overhead_post_processing_zero_blocks() {
    let mut conv = sys_user_conv("base", "hi");
    let orig = conv.system_message().unwrap().content.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv.system_message().unwrap().content.len(), orig);
}

#[test]
fn edge_overhead_disabled_zero_blocks() {
    let mut conv = sys_user_conv("base", "hi");
    let orig = conv.system_message().unwrap().content.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(conv.system_message().unwrap().content.len(), orig);
}

#[test]
fn edge_passthrough_post_processing_does_not_mutate() {
    let original = multi_turn();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[
            Capability::StructuredOutputJsonSchema,
            Capability::StopSequences,
        ],
        &mut conv,
    );
    assert_eq!(conv, original);
}

#[test]
fn edge_passthrough_disabled_does_not_mutate() {
    let original = multi_turn();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&non_emulatable(), &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn edge_mapped_injection_mutates_only_system() {
    let mut conv = multi_turn();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[1].text_content(), "Q1");
    assert_eq!(conv.messages[2].text_content(), "A1");
    assert_eq!(conv.messages[3].text_content(), "Q2");
    assert_eq!(conv.messages[4].text_content(), "A2");
    assert_eq!(conv.messages[5].text_content(), "Q3");
}

#[test]
fn edge_combo_all_emulatable_at_once() {
    let mut conv = sys_user_conv("sys", "usr");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&emulatable(), &mut conv);
    assert_eq!(report.applied.len(), emulatable().len());
    assert!(report.warnings.is_empty());
}

#[test]
fn edge_combo_all_non_emulatable_at_once() {
    let original = user_conv("x");
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&non_emulatable(), &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), non_emulatable().len());
    assert_eq!(conv, original);
}

#[test]
fn edge_combo_duplicate_cap_applied_multiple_times() {
    let mut conv = sys_user_conv("sys", "usr");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ExtendedThinking],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert_eq!(conv.system_message().unwrap().content.len(), 3);
}

#[test]
fn edge_combo_interleaved_preserves_order() {
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::ImageInput,
        Capability::Streaming,
        Capability::StopSequences,
    ];
    let mut conv = user_conv("x");
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&caps, &mut conv);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[1].capability, Capability::ImageInput);
    assert_eq!(report.applied[2].capability, Capability::StopSequences);
}

#[test]
fn edge_report_applied_plus_warnings_equals_input_count() {
    let mut conv = user_conv("x");
    let engine = EmulationEngine::with_defaults();
    let all = all_caps();
    let report = engine.apply(&all, &mut conv);
    assert_eq!(report.applied.len() + report.warnings.len(), all.len());
}
