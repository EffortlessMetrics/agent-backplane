// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the emulation engine.

use abp_core::Capability;
use abp_core::ir::{IrConversation, IrMessage, IrRole};
use abp_emulation::*;

// ── Factory function output tests ──────────────────────────────────────

#[test]
fn emulate_structured_output_returns_system_prompt_injection() {
    let s = emulate_structured_output();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("JSON"));
    }
}

#[test]
fn emulate_code_execution_returns_system_prompt_injection() {
    let s = emulate_code_execution();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("execute code"));
    }
}

#[test]
fn emulate_extended_thinking_returns_system_prompt_injection() {
    let s = emulate_extended_thinking();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("step by step"));
    }
}

#[test]
fn emulate_image_input_returns_system_prompt_injection() {
    let s = emulate_image_input();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert!(prompt.contains("Image"));
    }
}

#[test]
fn emulate_stop_sequences_returns_post_processing() {
    let s = emulate_stop_sequences();
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
    if let EmulationStrategy::PostProcessing { detail } = &s {
        assert!(detail.contains("stop sequence"));
    }
}

// ── Default strategy mapping for new capabilities ──────────────────────

#[test]
fn default_strategy_image_input_is_emulatable() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(!matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_stop_sequences_is_emulatable() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(!matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn can_emulate_image_input() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn can_emulate_stop_sequences() {
    assert!(can_emulate(&Capability::StopSequences));
}

// ── EmulationReport accuracy ───────────────────────────────────────────

#[test]
fn report_reflects_applied_strategies() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base"))
        .push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[1].capability, Capability::ImageInput);
    assert!(report.warnings.is_empty());
}

#[test]
fn report_records_disabled_as_warnings() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::Streaming], &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("Streaming"));
}

#[test]
fn report_entry_strategy_matches_resolved() {
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let report = engine.apply(&[Capability::StopSequences], &mut conv);
    assert_eq!(report.applied.len(), 1);

    let resolved = engine.resolve_strategy(&Capability::StopSequences);
    assert_eq!(report.applied[0].strategy, resolved);
}

// ── Engine applies correct strategy per capability ─────────────────────

#[test]
fn engine_applies_system_prompt_for_image_input() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Describe this image"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert!(conv.system_message().is_some());
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Image"));
}

#[test]
fn engine_applies_post_processing_for_stop_sequences() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));
    let mut conv = original.clone();

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
    // PostProcessing does not mutate the conversation
    assert_eq!(conv, original);
}

#[test]
fn engine_applies_extended_thinking_default() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Why?"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let text = conv.system_message().unwrap().text_content();
    assert!(text.contains("Think step by step"));
}

// ── Composability: multiple emulations in one request ──────────────────

#[test]
fn multiple_system_prompt_injections_compose() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Complex task"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Think step by step"));
    assert!(sys_text.contains("Image"));
    assert!(sys_text.contains("You are helpful."));
}

#[test]
fn mixed_strategy_types_compose() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Do everything"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,           // SystemPromptInjection
            Capability::StopSequences,              // PostProcessing
            Capability::StructuredOutputJsonSchema, // PostProcessing
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 3);
    assert!(report.warnings.is_empty());
}

#[test]
fn composing_emulated_and_disabled() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Mix"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::Streaming, // disabled
            Capability::ImageInput,
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 1);
}

// ── Fidelity labels ────────────────────────────────────────────────────

#[test]
fn fidelity_labels_native_capabilities() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);

    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
}

#[test]
fn fidelity_labels_emulated_capabilities() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

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
fn fidelity_labels_mixed_native_and_emulated() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);

    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);

    assert_eq!(labels.len(), 3);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ImageInput],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_labels_empty_inputs() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_emulated_entry_carries_strategy() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);

    let labels = compute_fidelity(&[], &report);
    if let FidelityLabel::Emulated { strategy } = &labels[&Capability::StopSequences] {
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    } else {
        panic!("expected Emulated fidelity label");
    }
}

// ── Strategy selection via config overrides ─────────────────────────────

#[test]
fn config_override_selects_custom_strategy() {
    let mut config = EmulationConfig::new();
    config.set(Capability::CodeExecution, emulate_code_execution());

    let engine = EmulationEngine::new(config);
    let strategy = engine.resolve_strategy(&Capability::CodeExecution);

    assert!(matches!(
        strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn config_override_enables_normally_disabled_capability() {
    let mut config = EmulationConfig::new();
    config.set(Capability::CodeExecution, emulate_code_execution());

    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Run code"));

    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn config_override_with_structured_output_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::StructuredOutputJsonSchema,
        emulate_structured_output(),
    );

    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Give JSON"));

    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("JSON"));
}

#[test]
fn config_override_can_disable_normally_emulatable() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ImageInput,
        EmulationStrategy::Disabled {
            reason: "policy restriction".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Image"));
    let report = engine.apply(&[Capability::ImageInput], &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

// ── Edge cases ─────────────────────────────────────────────────────────

#[test]
fn no_emulation_needed_empty_list() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let original = conv.clone();

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);

    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn all_capabilities_emulated() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Everything"));

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
    assert!(!report.is_empty());
}

#[test]
fn all_capabilities_disabled() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Nothing works"));
    let original = conv.clone();

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
fn partially_emulated_report() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Partial"));

    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking, // emulated
            Capability::Streaming,        // disabled
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

#[test]
fn check_missing_without_mutation() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking, Capability::CodeExecution]);

    assert_eq!(report.applied.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn check_missing_matches_apply_report() {
    let engine = EmulationEngine::with_defaults();
    let caps = [Capability::ImageInput, Capability::Streaming];

    let check_report = engine.check_missing(&caps);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let apply_report = engine.apply(&caps, &mut conv);

    assert_eq!(check_report.applied.len(), apply_report.applied.len());
    assert_eq!(check_report.warnings.len(), apply_report.warnings.len());
}

// ── Serde round-trips for new types ────────────────────────────────────

#[test]
fn fidelity_label_serde_roundtrip() {
    let labels = vec![
        FidelityLabel::Native,
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "test".into(),
            },
        },
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::PostProcessing {
                detail: "truncate".into(),
            },
        },
    ];

    for label in &labels {
        let json = serde_json::to_string(label).unwrap();
        let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(*label, decoded);
    }
}

#[test]
fn factory_strategies_serde_roundtrip() {
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
fn compute_fidelity_serde_roundtrip() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);

    let json = serde_json::to_string(&labels).unwrap();
    let decoded: std::collections::BTreeMap<Capability, FidelityLabel> =
        serde_json::from_str(&json).unwrap();
    assert_eq!(labels, decoded);
}
