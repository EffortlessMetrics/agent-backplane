#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the ABP emulation framework.
//!
//! Covers: strategy construction/serialization, named strategies, EmulationConfig/plan,
//! EmulationEngine, SystemPromptInjection, PostProcessing, Disabled strategy,
//! edge cases, serde roundtrips, and integration labeling.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::Capability;
use abp_emulation::strategies::*;
use abp_emulation::*;

// ── Helpers ────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn bare_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn multi_turn() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "System"))
        .push(IrMessage::text(IrRole::User, "Q1"))
        .push(IrMessage::text(IrRole::Assistant, "A1"))
        .push(IrMessage::text(IrRole::User, "Q2"))
}

fn image_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    ))
}

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![
        IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}}}),
        },
        IrToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file".into(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}}}),
        },
    ]
}

// ════════════════════════════════════════════════════════════════════════
// §1 — EmulationStrategy construction and serialization
// ════════════════════════════════════════════════════════════════════════

#[test]
fn strategy_construct_system_prompt_injection() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "Think step by step".into(),
    };
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn strategy_construct_post_processing() {
    let s = EmulationStrategy::PostProcessing {
        detail: "Validate JSON".into(),
    };
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn strategy_construct_disabled() {
    let s = EmulationStrategy::Disabled {
        reason: "unsafe operation".into(),
    };
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn strategy_serde_system_prompt_injection_tag() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["type"], "system_prompt_injection");
}

#[test]
fn strategy_serde_post_processing_tag() {
    let s = EmulationStrategy::PostProcessing {
        detail: "detail".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(r#""type":"post_processing"#));
}

#[test]
fn strategy_serde_disabled_tag() {
    let s = EmulationStrategy::Disabled {
        reason: "nope".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(r#""type":"disabled"#));
}

#[test]
fn strategy_equality_same_variant_same_content() {
    let a = EmulationStrategy::SystemPromptInjection {
        prompt: "same".into(),
    };
    let b = EmulationStrategy::SystemPromptInjection {
        prompt: "same".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn strategy_inequality_same_variant_different_content() {
    let a = EmulationStrategy::SystemPromptInjection {
        prompt: "v1".into(),
    };
    let b = EmulationStrategy::SystemPromptInjection {
        prompt: "v2".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn strategy_inequality_different_variant() {
    let a = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let b = EmulationStrategy::PostProcessing { detail: "x".into() };
    assert_ne!(a, b);
}

#[test]
fn strategy_debug_system_prompt_injection() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "Think step by step".into(),
    };
    let dbg = format!("{s:?}");
    assert!(dbg.contains("SystemPromptInjection"));
    assert!(dbg.contains("Think step by step"));
}

#[test]
fn strategy_debug_post_processing() {
    let s = EmulationStrategy::PostProcessing {
        detail: "Validate JSON".into(),
    };
    assert!(format!("{s:?}").contains("PostProcessing"));
}

#[test]
fn strategy_debug_disabled() {
    let s = EmulationStrategy::Disabled {
        reason: "unsafe".into(),
    };
    assert!(format!("{s:?}").contains("Disabled"));
}

#[test]
fn strategy_clone() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    assert_eq!(s, s.clone());
}

// ════════════════════════════════════════════════════════════════════════
// §2 — Named strategies: verify prompt text, post-processing details
// ════════════════════════════════════════════════════════════════════════

#[test]
fn named_structured_output_is_system_prompt_injection() {
    assert!(matches!(
        emulate_structured_output(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn named_structured_output_mentions_json() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_structured_output() {
        assert!(prompt.contains("JSON"));
    }
}

#[test]
fn named_code_execution_is_system_prompt_injection() {
    assert!(matches!(
        emulate_code_execution(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn named_code_execution_mentions_execute() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_code_execution() {
        assert!(prompt.contains("execute code"));
    }
}

#[test]
fn named_extended_thinking_is_system_prompt_injection() {
    assert!(matches!(
        emulate_extended_thinking(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn named_extended_thinking_mentions_step_by_step() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_extended_thinking() {
        assert!(prompt.contains("step by step"));
    }
}

#[test]
fn named_image_input_is_system_prompt_injection() {
    assert!(matches!(
        emulate_image_input(),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn named_image_input_mentions_image() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_image_input() {
        assert!(prompt.contains("Image"));
    }
}

#[test]
fn named_stop_sequences_is_post_processing() {
    assert!(matches!(
        emulate_stop_sequences(),
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn named_stop_sequences_mentions_stop_sequence() {
    if let EmulationStrategy::PostProcessing { detail } = emulate_stop_sequences() {
        assert!(detail.contains("stop sequence"));
    }
}

// ════════════════════════════════════════════════════════════════════════
// §3 — EmulationPlan (EmulationConfig + check_missing): creating plans,
//       listing strategies, applying plans
// ════════════════════════════════════════════════════════════════════════

#[test]
fn config_new_starts_empty() {
    assert!(EmulationConfig::new().strategies.is_empty());
}

#[test]
fn config_default_is_empty() {
    assert!(EmulationConfig::default().strategies.is_empty());
}

#[test]
fn config_set_single_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think first.".into(),
        },
    );
    assert_eq!(config.strategies.len(), 1);
    assert!(config
        .strategies
        .contains_key(&Capability::ExtendedThinking));
}

#[test]
fn config_set_multiple_strategies() {
    let mut config = EmulationConfig::new();
    config.set(Capability::ExtendedThinking, emulate_extended_thinking());
    config.set(Capability::ImageInput, emulate_image_input());
    assert_eq!(config.strategies.len(), 2);
}

#[test]
fn config_overwrite_existing_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "v1".into(),
        },
    );
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "v2".into(),
        },
    );
    assert_eq!(config.strategies.len(), 1);
    if let EmulationStrategy::SystemPromptInjection { prompt } =
        &config.strategies[&Capability::ExtendedThinking]
    {
        assert_eq!(prompt, "v2");
    }
}

#[test]
fn check_missing_lists_emulatable() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn check_missing_lists_disabled_as_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn check_missing_matches_apply_report_structure() {
    let engine = EmulationEngine::with_defaults();
    let caps = [
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::ImageInput,
    ];
    let check = engine.check_missing(&caps);
    let mut conv = bare_conv();
    let apply = engine.apply(&caps, &mut conv);
    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
}

#[test]
fn check_missing_is_read_only() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

// ════════════════════════════════════════════════════════════════════════
// §4 — EmulationEngine: applying to conversations, labeling emulated
// ════════════════════════════════════════════════════════════════════════

#[test]
fn engine_with_defaults_uses_default_strategies() {
    let engine = EmulationEngine::with_defaults();
    let caps = [
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
        Capability::ImageInput,
        Capability::StopSequences,
    ];
    for cap in &caps {
        assert_eq!(engine.resolve_strategy(cap), default_strategy(cap));
    }
}

#[test]
fn engine_config_override_replaces_default() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "policy override".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    assert!(matches!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn engine_apply_returns_report_with_entries() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
}

#[test]
fn engine_apply_returns_correct_strategy_types() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
            Capability::ImageInput,
            Capability::StopSequences,
        ],
        &mut conv,
    );
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        report.applied[1].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
    assert!(matches!(
        report.applied[2].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        report.applied[3].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn engine_apply_preserves_ordering_in_report() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let caps = [
        Capability::ImageInput,
        Capability::ExtendedThinking,
        Capability::StopSequences,
    ];
    let report = engine.apply(&caps, &mut conv);
    for (i, cap) in caps.iter().enumerate() {
        assert_eq!(&report.applied[i].capability, cap);
    }
}

#[test]
fn engine_apply_labels_never_silent_degradation() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::CodeExecution,
            Capability::Streaming,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len() + report.warnings.len(), 3);
}

#[test]
fn engine_clone_preserves_config() {
    let engine = EmulationEngine::with_defaults();
    let cloned = engine.clone();
    assert_eq!(
        engine.resolve_strategy(&Capability::ExtendedThinking),
        cloned.resolve_strategy(&Capability::ExtendedThinking)
    );
}

#[test]
fn engine_free_fn_apply_emulation_works() {
    let config = EmulationConfig::new();
    let mut conv = simple_conv();
    let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

// ════════════════════════════════════════════════════════════════════════
// §5 — SystemPromptInjection: verify prompt is prepended/injected
// ════════════════════════════════════════════════════════════════════════

#[test]
fn injection_appends_to_existing_system_message() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("You are helpful."));
    assert!(sys.contains("Think step by step"));
}

#[test]
fn injection_creates_system_message_if_missing() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0]
        .text_content()
        .contains("Think step by step"));
}

#[test]
fn injection_creates_system_on_empty_conversation() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn injection_two_capabilities_compose_in_system() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    assert!(sys.contains("Image"));
    assert!(sys.contains("You are helpful."));
}

#[test]
fn injection_sequential_applies_accumulate() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    engine.apply(&[Capability::ImageInput], &mut conv);

    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("Think step by step"));
    assert!(sys.contains("Image"));
}

#[test]
fn injection_multi_turn_single_system_msg() {
    let mut conv = multi_turn();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys_count = conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .count();
    assert_eq!(sys_count, 1);
}

// ════════════════════════════════════════════════════════════════════════
// §6 — PostProcessing: verify processing steps are recorded
// ════════════════════════════════════════════════════════════════════════

#[test]
fn post_processing_does_not_mutate_conversation() {
    let original = bare_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn post_processing_recorded_in_report() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn post_processing_stop_sequences_does_not_mutate() {
    let original = bare_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StopSequences], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn post_processing_mixed_with_injection() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        report.applied[1].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// §7 — Disabled strategy: verify it fails early with reason
// ════════════════════════════════════════════════════════════════════════

#[test]
fn disabled_produces_warning() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn disabled_not_in_applied() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert!(report.applied.is_empty());
}

#[test]
fn disabled_does_not_modify_conversation() {
    let original = bare_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn disabled_multiple_caps_multiple_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
    ]);
    assert_eq!(report.warnings.len(), 3);
    assert!(report.has_unemulatable());
}

#[test]
fn disabled_tool_read_by_default() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn disabled_explicit_via_config() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user policy".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = bare_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("user policy"));
}

#[test]
fn disabled_code_execution_reason_mentions_sandbox() {
    if let EmulationStrategy::Disabled { reason } = default_strategy(&Capability::CodeExecution) {
        assert!(reason.contains("sandboxed"));
    } else {
        panic!("expected Disabled");
    }
}

#[test]
fn disabled_unknown_cap_reason_mentions_name() {
    if let EmulationStrategy::Disabled { reason } = default_strategy(&Capability::ToolGlob) {
        assert!(reason.contains("ToolGlob"));
    } else {
        panic!("expected Disabled");
    }
}

// ════════════════════════════════════════════════════════════════════════
// §8 — Edge cases: empty conversation, no capabilities, all disabled
// ════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_capabilities_no_emulation() {
    let mut conv = bare_conv();
    let original = conv.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn edge_empty_conversation_creates_system() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn edge_all_disabled_no_mutation() {
    let original = bare_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::CodeExecution,
        ],
        &mut conv,
    );
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 3);
    assert!(report.has_unemulatable());
    assert_eq!(conv, original);
}

#[test]
fn edge_all_emulatable() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
            Capability::ImageInput,
            Capability::StopSequences,
        ],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn edge_duplicate_capability_in_list() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ExtendedThinking],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 2);
}

#[test]
fn edge_report_default_is_empty() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn edge_report_not_empty_with_applied() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection { prompt: "x".into() },
        }],
        warnings: vec![],
    };
    assert!(!report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn edge_report_not_empty_with_warnings() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["warn".into()],
    };
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

#[test]
fn edge_report_clone_equals() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report, report.clone());
}

#[test]
fn edge_config_debug_output() {
    assert!(format!("{:?}", EmulationConfig::new()).contains("EmulationConfig"));
}

#[test]
fn edge_config_enable_normally_disabled() {
    let mut config = EmulationConfig::new();
    config.set(Capability::CodeExecution, emulate_code_execution());
    let mut conv = bare_conv();
    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn edge_config_disable_normally_enabled() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ImageInput,
        EmulationStrategy::Disabled {
            reason: "policy restriction".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let mut conv = bare_conv();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn edge_config_change_strategy_type() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::StructuredOutputJsonSchema,
        emulate_structured_output(),
    );
    let mut conv = bare_conv();
    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(conv
        .system_message()
        .unwrap()
        .text_content()
        .contains("JSON"));
}

#[test]
fn edge_partially_emulated_report() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::Streaming],
        &mut conv,
    );
    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

// ════════════════════════════════════════════════════════════════════════
// §9 — Serialization roundtrips for all strategy types
// ════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_all_strategy_variants() {
    let strategies = vec![
        EmulationStrategy::SystemPromptInjection {
            prompt: "test".into(),
        },
        EmulationStrategy::PostProcessing {
            detail: "validate".into(),
        },
        EmulationStrategy::Disabled {
            reason: "nope".into(),
        },
    ];
    for s in &strategies {
        let json = serde_json::to_string(s).unwrap();
        let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*s, decoded);
    }
}

#[test]
fn serde_roundtrip_all_factory_strategies() {
    let strategies = vec![
        emulate_structured_output(),
        emulate_code_execution(),
        emulate_extended_thinking(),
        emulate_image_input(),
        emulate_stop_sequences(),
    ];
    for s in &strategies {
        let json = serde_json::to_string(s).unwrap();
        let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(*s, decoded);
    }
}

#[test]
fn serde_roundtrip_emulation_config() {
    let mut config = EmulationConfig::new();
    config.set(Capability::ExtendedThinking, emulate_extended_thinking());
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled {
            reason: "unsafe".into(),
        },
    );
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn serde_roundtrip_emulation_report() {
    let report = EmulationReport {
        applied: vec![
            EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think.".into(),
                },
            },
            EmulationEntry {
                capability: Capability::StopSequences,
                strategy: EmulationStrategy::PostProcessing {
                    detail: "truncate".into(),
                },
            },
        ],
        warnings: vec!["CodeExecution not available".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn serde_roundtrip_fidelity_label_native() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn serde_roundtrip_fidelity_label_emulated() {
    let label = FidelityLabel::Emulated {
        strategy: EmulationStrategy::PostProcessing {
            detail: "truncate".into(),
        },
    };
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn serde_roundtrip_compute_fidelity_map() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    let json = serde_json::to_string(&labels).unwrap();
    let decoded: std::collections::BTreeMap<Capability, FidelityLabel> =
        serde_json::from_str(&json).unwrap();
    assert_eq!(labels, decoded);
}

#[test]
fn serde_config_deterministic_ordering() {
    let mut config = EmulationConfig::new();
    config.set(Capability::ExtendedThinking, emulate_extended_thinking());
    config.set(Capability::ImageInput, emulate_image_input());
    let json1 = serde_json::to_string(&config).unwrap();
    let json2 = serde_json::to_string(&config).unwrap();
    assert_eq!(json1, json2, "BTreeMap ensures deterministic ordering");
}

#[test]
fn serde_fidelity_native_tag() {
    let json = serde_json::to_string(&FidelityLabel::Native).unwrap();
    assert!(json.contains(r#""fidelity":"native"#));
}

#[test]
fn serde_fidelity_emulated_tag() {
    let label = FidelityLabel::Emulated {
        strategy: EmulationStrategy::PostProcessing { detail: "d".into() },
    };
    let json = serde_json::to_string(&label).unwrap();
    assert!(json.contains(r#""fidelity":"emulated"#));
}

#[test]
fn serde_roundtrip_stream_chunk() {
    let chunk = StreamChunk {
        content: "hello".into(),
        index: 0,
        is_final: true,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    let decoded: StreamChunk = serde_json::from_str(&json).unwrap();
    assert_eq!(chunk, decoded);
}

#[test]
fn serde_roundtrip_parsed_tool_call() {
    let call = ParsedToolCall {
        name: "test".into(),
        arguments: serde_json::json!({"key": "value"}),
    };
    let json = serde_json::to_string(&call).unwrap();
    let decoded: ParsedToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(call, decoded);
}

#[test]
fn serde_roundtrip_thinking_detail() {
    let details = [
        ThinkingDetail::Brief,
        ThinkingDetail::Standard,
        ThinkingDetail::Detailed,
    ];
    for d in &details {
        let json = serde_json::to_string(d).unwrap();
        let decoded: ThinkingDetail = serde_json::from_str(&json).unwrap();
        assert_eq!(*d, decoded);
    }
}

// ════════════════════════════════════════════════════════════════════════
// §10 — Integration: emulate + verify labeling in receipt/output
// ════════════════════════════════════════════════════════════════════════

#[test]
fn integration_full_pipeline_labels() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ToolUse,
        EmulationStrategy::SystemPromptInjection {
            prompt: ToolUseEmulation::tools_to_prompt(&sample_tools()),
        },
    );

    let engine = EmulationEngine::new(config);
    let missing = [
        Capability::ExtendedThinking,
        Capability::ToolUse,
        Capability::CodeExecution,
    ];
    let native = [Capability::Streaming];

    let mut conv = simple_conv();
    let report = engine.apply(&missing, &mut conv);

    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 1);

    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
    assert!(matches!(
        labels[&Capability::ToolUse],
        FidelityLabel::Emulated { .. }
    ));
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn integration_fidelity_native_only() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(
        &[
            Capability::Streaming,
            Capability::ToolUse,
            Capability::CodeExecution,
        ],
        &report,
    );
    assert_eq!(labels.len(), 3);
    for label in labels.values() {
        assert_eq!(*label, FidelityLabel::Native);
    }
}

#[test]
fn integration_fidelity_emulated_overrides_native() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::Streaming,
            strategy: EmulationStrategy::PostProcessing {
                detail: "override".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert!(matches!(
        labels[&Capability::Streaming],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn integration_fidelity_warnings_not_labeled() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["CodeExecution not emulated: unsafe".into()],
    };
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn integration_fidelity_mixed_native_emulated_warned() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ImageInput,
            strategy: emulate_image_input(),
        }],
        warnings: vec!["CodeExecution: disabled".into()],
    };
    let labels = compute_fidelity(&[Capability::Streaming], &report);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ImageInput],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn integration_fidelity_empty_inputs() {
    let labels = compute_fidelity(&[], &EmulationReport::default());
    assert!(labels.is_empty());
}

#[test]
fn integration_fidelity_emulated_carries_strategy() {
    let mut conv = bare_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);
    let labels = compute_fidelity(&[], &report);
    if let FidelityLabel::Emulated { strategy } = &labels[&Capability::StopSequences] {
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    } else {
        panic!("expected Emulated fidelity label");
    }
}

#[test]
fn integration_vision_then_thinking() {
    let mut conv = image_conv();
    VisionEmulation::apply(&mut conv);
    ThinkingEmulation::standard().inject(&mut conv);

    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("image(s)"));
    assert!(sys.contains("<thinking>"));
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn integration_tool_use_parse_and_convert() {
    let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "test.txt"}}
</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    let call = calls[0].as_ref().unwrap();
    let block = ToolUseEmulation::to_tool_use_block(call, "id-1");
    assert!(matches!(block, IrContentBlock::ToolUse { .. }));
}

// ════════════════════════════════════════════════════════════════════════
// Strategies module: ThinkingEmulation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn thinking_detail_levels_differ() {
    let brief = ThinkingEmulation::brief().prompt_text();
    let standard = ThinkingEmulation::standard().prompt_text();
    let detailed = ThinkingEmulation::detailed().prompt_text();
    assert_ne!(brief, standard);
    assert_ne!(standard, detailed);
    assert_ne!(brief, detailed);
}

#[test]
fn thinking_brief_mentions_step_by_step() {
    assert!(ThinkingEmulation::brief()
        .prompt_text()
        .contains("step by step"));
}

#[test]
fn thinking_standard_mentions_thinking_tags() {
    let text = ThinkingEmulation::standard().prompt_text();
    assert!(text.contains("<thinking>"));
    assert!(text.contains("</thinking>"));
}

#[test]
fn thinking_detailed_mentions_verification() {
    assert!(ThinkingEmulation::detailed()
        .prompt_text()
        .contains("Verify"));
}

#[test]
fn thinking_detailed_mentions_sub_problems() {
    assert!(ThinkingEmulation::detailed()
        .prompt_text()
        .contains("sub-problems"));
}

#[test]
fn thinking_inject_preserves_user_messages() {
    let emu = ThinkingEmulation::standard();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Solve 2+2"));
    emu.inject(&mut conv);
    assert_eq!(conv.messages.len(), 2);
    assert_eq!(conv.messages[1].text_content(), "Solve 2+2");
}

#[test]
fn thinking_extract_with_tags() {
    let text = "Preamble <thinking>I need to think</thinking>The answer is 42.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert_eq!(thinking, "I need to think");
    assert!(answer.contains("42"));
}

#[test]
fn thinking_extract_no_tags() {
    let text = "Plain answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.is_empty());
    assert_eq!(answer, text);
}

#[test]
fn thinking_extract_empty_tags() {
    let (thinking, _) = ThinkingEmulation::extract_thinking("<thinking></thinking>The answer.");
    assert!(thinking.is_empty());
}

#[test]
fn thinking_extract_multiline() {
    let text = "<thinking>\nStep 1: analyze\nStep 2: solve\n</thinking>\nFinal answer.";
    let (thinking, answer) = ThinkingEmulation::extract_thinking(text);
    assert!(thinking.contains("Step 1"));
    assert!(thinking.contains("Step 2"));
    assert!(answer.contains("Final answer."));
}

#[test]
fn thinking_to_block_some() {
    let text = "<thinking>Step 1: analyze</thinking>Answer.";
    let block = ThinkingEmulation::to_thinking_block(text);
    assert!(block.is_some());
    if let Some(IrContentBlock::Thinking { text: t }) = block {
        assert_eq!(t, "Step 1: analyze");
    }
}

#[test]
fn thinking_to_block_none() {
    assert!(ThinkingEmulation::to_thinking_block("No tags.").is_none());
}

// ════════════════════════════════════════════════════════════════════════
// Strategies module: ToolUseEmulation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn tool_prompt_empty_tools() {
    assert!(ToolUseEmulation::tools_to_prompt(&[]).is_empty());
}

#[test]
fn tool_prompt_contains_tool_names() {
    let prompt = ToolUseEmulation::tools_to_prompt(&sample_tools());
    assert!(prompt.contains("read_file"));
    assert!(prompt.contains("write_file"));
}

#[test]
fn tool_prompt_contains_instructions() {
    let prompt = ToolUseEmulation::tools_to_prompt(&sample_tools());
    assert!(prompt.contains("<tool_call>"));
    assert!(prompt.contains("</tool_call>"));
}

#[test]
fn tool_inject_creates_system_message() {
    let mut conv = bare_conv();
    ToolUseEmulation::inject_tools(&mut conv, &sample_tools());
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("read_file"));
}

#[test]
fn tool_inject_appends_to_existing_system() {
    let mut conv = simple_conv();
    ToolUseEmulation::inject_tools(&mut conv, &sample_tools());
    let sys = conv.system_message().unwrap().text_content();
    assert!(sys.contains("You are helpful."));
    assert!(sys.contains("read_file"));
}

#[test]
fn tool_inject_empty_tools_noop() {
    let mut conv = bare_conv();
    let original = conv.clone();
    ToolUseEmulation::inject_tools(&mut conv, &[]);
    assert_eq!(conv, original);
}

#[test]
fn tool_parse_single_call() {
    let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "test.txt"}}
</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 1);
    let call = calls[0].as_ref().unwrap();
    assert_eq!(call.name, "read_file");
    assert_eq!(call.arguments["path"], "test.txt");
}

#[test]
fn tool_parse_multiple_calls() {
    let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "a.txt"}}
</tool_call>
Between
<tool_call>
{"name": "write_file", "arguments": {"path": "b.txt", "content": "hello"}}
</tool_call>"#;
    let calls = ToolUseEmulation::parse_tool_calls(text);
    assert_eq!(calls.len(), 2);
}

#[test]
fn tool_parse_no_calls() {
    assert!(ToolUseEmulation::parse_tool_calls("no tools").is_empty());
}

#[test]
fn tool_parse_invalid_json() {
    let calls = ToolUseEmulation::parse_tool_calls("<tool_call>\nnot json\n</tool_call>");
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("invalid JSON"));
}

#[test]
fn tool_parse_missing_name() {
    let calls =
        ToolUseEmulation::parse_tool_calls("<tool_call>\n{\"arguments\": {}}\n</tool_call>");
    assert!(calls[0].is_err());
    assert!(calls[0].as_ref().unwrap_err().contains("missing 'name'"));
}

#[test]
fn tool_parse_unclosed_tag() {
    let calls = ToolUseEmulation::parse_tool_calls("<tool_call>\n{\"name\": \"f\"}\nno close");
    assert!(calls[0].as_ref().unwrap_err().contains("unclosed"));
}

#[test]
fn tool_parse_missing_arguments_defaults_null() {
    let calls =
        ToolUseEmulation::parse_tool_calls("<tool_call>\n{\"name\": \"list\"}\n</tool_call>");
    let call = calls[0].as_ref().unwrap();
    assert!(call.arguments.is_null());
}

#[test]
fn tool_to_tool_use_block() {
    let call = ParsedToolCall {
        name: "read_file".into(),
        arguments: serde_json::json!({"path": "a.txt"}),
    };
    let block = ToolUseEmulation::to_tool_use_block(&call, "tc-001");
    if let IrContentBlock::ToolUse { id, name, input } = block {
        assert_eq!(id, "tc-001");
        assert_eq!(name, "read_file");
        assert_eq!(input["path"], "a.txt");
    } else {
        panic!("expected ToolUse block");
    }
}

#[test]
fn tool_format_result_success() {
    let result = ToolUseEmulation::format_tool_result("read_file", "contents", false);
    assert!(result.contains("read_file"));
    assert!(!result.contains("error"));
}

#[test]
fn tool_format_result_error() {
    let result = ToolUseEmulation::format_tool_result("write_file", "denied", true);
    assert!(result.contains("error"));
    assert!(result.contains("denied"));
}

#[test]
fn tool_extract_text_outside_calls() {
    let text = "Pre. <tool_call>\n{\"name\":\"f\"}\n</tool_call> Post.";
    let outside = ToolUseEmulation::extract_text_outside_tool_calls(text);
    assert!(outside.contains("Pre."));
    assert!(outside.contains("Post."));
    assert!(!outside.contains("tool_call"));
}

#[test]
fn tool_extract_text_no_calls() {
    assert_eq!(
        ToolUseEmulation::extract_text_outside_tool_calls("Just text."),
        "Just text."
    );
}

#[test]
fn tool_null_parameters_omitted() {
    let tools = vec![IrToolDefinition {
        name: "noop".into(),
        description: "Does nothing".into(),
        parameters: serde_json::Value::Null,
    }];
    let prompt = ToolUseEmulation::tools_to_prompt(&tools);
    assert!(!prompt.contains("Parameters:"));
}

// ════════════════════════════════════════════════════════════════════════
// Strategies module: VisionEmulation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn vision_has_images_true() {
    assert!(VisionEmulation::has_images(&image_conv()));
}

#[test]
fn vision_has_images_false() {
    assert!(!VisionEmulation::has_images(&bare_conv()));
}

#[test]
fn vision_replace_returns_count() {
    let mut conv = image_conv();
    assert_eq!(
        VisionEmulation::replace_images_with_placeholders(&mut conv),
        1
    );
}

#[test]
fn vision_replace_no_images_returns_zero() {
    let mut conv = bare_conv();
    assert_eq!(
        VisionEmulation::replace_images_with_placeholders(&mut conv),
        0
    );
}

#[test]
fn vision_replace_multiple_images() {
    let mut conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "a".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "b".into(),
            },
        ],
    ));
    assert_eq!(
        VisionEmulation::replace_images_with_placeholders(&mut conv),
        2
    );
}

#[test]
fn vision_fallback_noop_for_zero() {
    let mut conv = bare_conv();
    let original = conv.clone();
    VisionEmulation::inject_vision_fallback_prompt(&mut conv, 0);
    assert_eq!(conv, original);
}

#[test]
fn vision_apply_full_pipeline() {
    let mut conv = image_conv();
    let count = VisionEmulation::apply(&mut conv);
    assert_eq!(count, 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.messages[0].text_content().contains("1 image(s)"));
    assert!(!VisionEmulation::has_images(&conv));
}

#[test]
fn vision_apply_no_images_noop() {
    let mut conv = bare_conv();
    let original = conv.clone();
    assert_eq!(VisionEmulation::apply(&mut conv), 0);
    assert_eq!(conv, original);
}

#[test]
fn vision_across_multiple_messages() {
    let mut conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "a".into(),
            }],
        ))
        .push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/gif".into(),
                data: "b".into(),
            }],
        ));
    assert_eq!(VisionEmulation::apply(&mut conv), 2);
    assert!(!VisionEmulation::has_images(&conv));
}

// ════════════════════════════════════════════════════════════════════════
// Strategies module: StreamingEmulation
// ════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_empty_text_single_chunk() {
    let chunks = StreamingEmulation::default_chunk_size().split_into_chunks("");
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content.is_empty());
    assert!(chunks[0].is_final);
}

#[test]
fn streaming_reassemble_roundtrip() {
    let emu = StreamingEmulation::new(10);
    let text = "This is a test of the streaming emulation system.";
    assert_eq!(
        StreamingEmulation::reassemble(&emu.split_into_chunks(text)),
        text
    );
}

#[test]
fn streaming_fixed_reassemble_roundtrip() {
    let emu = StreamingEmulation::new(7);
    let text = "abcdefghijklmnopqrstuvwxyz";
    assert_eq!(StreamingEmulation::reassemble(&emu.split_fixed(text)), text);
}

#[test]
fn streaming_fixed_chunk_sizes() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_fixed("abcdefghijklm");
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].content, "abcde");
    assert_eq!(chunks[1].content, "fghij");
    assert_eq!(chunks[2].content, "klm");
}

#[test]
fn streaming_indices_sequential() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_into_chunks("Hello world, how are you?");
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.index, i);
    }
}

#[test]
fn streaming_only_last_is_final() {
    let emu = StreamingEmulation::new(5);
    let chunks = emu.split_into_chunks("Hello world, how are you?");
    for chunk in &chunks[..chunks.len() - 1] {
        assert!(!chunk.is_final);
    }
    assert!(chunks.last().unwrap().is_final);
}

#[test]
fn streaming_minimum_chunk_size() {
    assert_eq!(StreamingEmulation::new(0).chunk_size(), 1);
}

#[test]
fn streaming_default_is_20() {
    assert_eq!(StreamingEmulation::default_chunk_size().chunk_size(), 20);
}

#[test]
fn streaming_word_boundary_preference() {
    let emu = StreamingEmulation::new(12);
    let chunks = emu.split_into_chunks("Hello world foo bar");
    let reassembled = StreamingEmulation::reassemble(&chunks);
    assert_eq!(reassembled, "Hello world foo bar");
    assert!(chunks[0].content.ends_with(' ') || chunks[0].content == "Hello world foo bar");
}

#[test]
fn streaming_chunk_size_one() {
    let emu = StreamingEmulation::new(1);
    let chunks = emu.split_fixed("abc");
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].content, "a");
    assert_eq!(chunks[1].content, "b");
    assert_eq!(chunks[2].content, "c");
}

// ════════════════════════════════════════════════════════════════════════
// default_strategy / can_emulate coverage
// ════════════════════════════════════════════════════════════════════════

#[test]
fn default_strategy_image_input_matches_factory() {
    assert_eq!(
        default_strategy(&Capability::ImageInput),
        emulate_image_input()
    );
}

#[test]
fn default_strategy_stop_sequences_matches_factory() {
    assert_eq!(
        default_strategy(&Capability::StopSequences),
        emulate_stop_sequences()
    );
}

#[test]
fn default_strategy_all_three_variants_covered() {
    assert!(matches!(
        default_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(matches!(
        default_strategy(&Capability::StructuredOutputJsonSchema),
        EmulationStrategy::PostProcessing { .. }
    ));
    assert!(matches!(
        default_strategy(&Capability::CodeExecution),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn can_emulate_reflects_default_strategy() {
    let caps = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::CodeExecution,
        Capability::StopSequences,
        Capability::StructuredOutputJsonSchema,
        Capability::ToolUse,
    ];
    for cap in &caps {
        let is_disabled = matches!(default_strategy(cap), EmulationStrategy::Disabled { .. });
        assert_eq!(can_emulate(cap), !is_disabled, "mismatch for {cap:?}");
    }
}

#[test]
fn default_strategy_many_disabled() {
    let disabled_caps = [
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolUse,
        Capability::Checkpointing,
        Capability::SessionResume,
    ];
    for cap in &disabled_caps {
        assert!(
            matches!(default_strategy(cap), EmulationStrategy::Disabled { .. }),
            "{cap:?} should default to Disabled"
        );
    }
}
