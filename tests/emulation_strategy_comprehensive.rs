// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the abp-emulation crate.

use abp_capability::{generate_report, negotiate};
use abp_core::ir::{IrConversation, IrMessage, IrRole};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    SupportLevel as CoreSupportLevel,
};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationEntry, EmulationReport, EmulationStrategy,
    FidelityLabel, apply_emulation, can_emulate, compute_fidelity, default_strategy,
    emulate_code_execution, emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output,
};
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// 1. EmulationStrategy construction and variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn strategy_system_prompt_injection_construction() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test prompt".into(),
    };
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn strategy_post_processing_construction() {
    let s = EmulationStrategy::PostProcessing {
        detail: "validate output".into(),
    };
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn strategy_disabled_construction() {
    let s = EmulationStrategy::Disabled {
        reason: "unsafe".into(),
    };
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn strategy_system_prompt_injection_holds_prompt() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "hello world".into(),
    };
    if let EmulationStrategy::SystemPromptInjection { prompt } = &s {
        assert_eq!(prompt, "hello world");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn strategy_post_processing_holds_detail() {
    let s = EmulationStrategy::PostProcessing {
        detail: "trim whitespace".into(),
    };
    if let EmulationStrategy::PostProcessing { detail } = &s {
        assert_eq!(detail, "trim whitespace");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn strategy_disabled_holds_reason() {
    let s = EmulationStrategy::Disabled {
        reason: "not possible".into(),
    };
    if let EmulationStrategy::Disabled { reason } = &s {
        assert_eq!(reason, "not possible");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn strategy_clone_equality() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let s2 = s.clone();
    assert_eq!(s, s2);
}

#[test]
fn strategy_variants_not_equal() {
    let a = EmulationStrategy::SystemPromptInjection { prompt: "a".into() };
    let b = EmulationStrategy::PostProcessing { detail: "a".into() };
    assert_ne!(a, b);
}

#[test]
fn strategy_same_variant_different_data() {
    let a = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    let b = EmulationStrategy::SystemPromptInjection { prompt: "y".into() };
    assert_ne!(a, b);
}

#[test]
fn strategy_debug_impl() {
    let s = EmulationStrategy::Disabled {
        reason: "debug".into(),
    };
    let debug = format!("{s:?}");
    assert!(debug.contains("Disabled"));
    assert!(debug.contains("debug"));
}

// ═══════════════════════════════════════════════════════════════════════
// Named strategy constructors
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn named_emulate_structured_output_is_system_prompt() {
    let s = emulate_structured_output();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn named_emulate_structured_output_mentions_json() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_structured_output() {
        assert!(prompt.contains("JSON"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn named_emulate_code_execution_is_system_prompt() {
    let s = emulate_code_execution();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn named_emulate_code_execution_mentions_code() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_code_execution() {
        assert!(prompt.to_lowercase().contains("code"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn named_emulate_extended_thinking_is_system_prompt() {
    let s = emulate_extended_thinking();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn named_emulate_extended_thinking_mentions_step() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_extended_thinking() {
        assert!(prompt.to_lowercase().contains("step"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn named_emulate_image_input_is_system_prompt() {
    let s = emulate_image_input();
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn named_emulate_image_input_mentions_image() {
    if let EmulationStrategy::SystemPromptInjection { prompt } = emulate_image_input() {
        assert!(prompt.to_lowercase().contains("image"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn named_emulate_stop_sequences_is_post_processing() {
    let s = emulate_stop_sequences();
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn named_emulate_stop_sequences_mentions_truncate() {
    if let EmulationStrategy::PostProcessing { detail } = emulate_stop_sequences() {
        assert!(detail.to_lowercase().contains("truncate"));
    } else {
        panic!("wrong variant");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Emulation detection and labeling (default_strategy, can_emulate)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn default_strategy_extended_thinking_is_system_prompt() {
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
fn default_strategy_image_input_is_system_prompt() {
    let s = default_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_strategy_stop_sequences_is_post_processing() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn default_strategy_streaming_is_disabled() {
    let s = default_strategy(&Capability::Streaming);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_use_is_disabled() {
    let s = default_strategy(&Capability::ToolUse);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_read_is_disabled() {
    let s = default_strategy(&Capability::ToolRead);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_write_is_disabled() {
    let s = default_strategy(&Capability::ToolWrite);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_edit_is_disabled() {
    let s = default_strategy(&Capability::ToolEdit);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_tool_bash_is_disabled() {
    let s = default_strategy(&Capability::ToolBash);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_logprobs_is_disabled() {
    let s = default_strategy(&Capability::Logprobs);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_seed_determinism_is_disabled() {
    let s = default_strategy(&Capability::SeedDeterminism);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_mcp_client_is_disabled() {
    let s = default_strategy(&Capability::McpClient);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_mcp_server_is_disabled() {
    let s = default_strategy(&Capability::McpServer);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_pdf_input_is_disabled() {
    let s = default_strategy(&Capability::PdfInput);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_session_resume_is_disabled() {
    let s = default_strategy(&Capability::SessionResume);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn default_strategy_checkpointing_is_disabled() {
    let s = default_strategy(&Capability::Checkpointing);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn can_emulate_returns_true_for_extended_thinking() {
    assert!(can_emulate(&Capability::ExtendedThinking));
}

#[test]
fn can_emulate_returns_true_for_structured_output() {
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn can_emulate_returns_true_for_image_input() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn can_emulate_returns_true_for_stop_sequences() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn can_emulate_returns_false_for_code_execution() {
    assert!(!can_emulate(&Capability::CodeExecution));
}

#[test]
fn can_emulate_returns_false_for_streaming() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn can_emulate_returns_false_for_tool_use() {
    assert!(!can_emulate(&Capability::ToolUse));
}

#[test]
fn can_emulate_returns_false_for_tool_read() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn can_emulate_returns_false_for_logprobs() {
    assert!(!can_emulate(&Capability::Logprobs));
}

#[test]
fn can_emulate_returns_false_for_seed_determinism() {
    assert!(!can_emulate(&Capability::SeedDeterminism));
}

// ═══════════════════════════════════════════════════════════════════════
// FidelityLabel
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_label_native() {
    let label = FidelityLabel::Native;
    assert!(matches!(label, FidelityLabel::Native));
}

#[test]
fn fidelity_label_emulated() {
    let label = FidelityLabel::Emulated {
        strategy: EmulationStrategy::SystemPromptInjection {
            prompt: "test".into(),
        },
    };
    assert!(matches!(label, FidelityLabel::Emulated { .. }));
}

#[test]
fn fidelity_label_clone_eq() {
    let a = FidelityLabel::Native;
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn fidelity_label_native_not_eq_emulated() {
    let a = FidelityLabel::Native;
    let b = FidelityLabel::Emulated {
        strategy: EmulationStrategy::Disabled { reason: "x".into() },
    };
    assert_ne!(a, b);
}

#[test]
fn compute_fidelity_native_only() {
    let native = vec![Capability::Streaming, Capability::ToolUse];
    let report = EmulationReport::default();
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
}

#[test]
fn compute_fidelity_emulated_only() {
    let native: Vec<Capability> = vec![];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "think".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn compute_fidelity_mixed() {
    let native = vec![Capability::Streaming];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec!["CodeExecution not emulated".into()],
    };
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
    // Warning-only capabilities are omitted
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn compute_fidelity_empty() {
    let labels = compute_fidelity(&[], &EmulationReport::default());
    assert!(labels.is_empty());
}

#[test]
fn compute_fidelity_emulated_overrides_native() {
    // If same capability appears in both native and report.applied, emulated wins
    let native = vec![Capability::ExtendedThinking];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::PostProcessing {
                detail: "override".into(),
            },
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// EmulationConfig
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_new_is_empty() {
    let config = EmulationConfig::new();
    assert!(config.strategies.is_empty());
}

#[test]
fn config_default_is_empty() {
    let config = EmulationConfig::default();
    assert!(config.strategies.is_empty());
}

#[test]
fn config_set_inserts_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "test".into(),
        },
    );
    assert_eq!(config.strategies.len(), 1);
    assert!(
        config
            .strategies
            .contains_key(&Capability::ExtendedThinking)
    );
}

#[test]
fn config_set_overwrites() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "first".into(),
        },
    );
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "second".into(),
        },
    );
    assert_eq!(config.strategies.len(), 1);
    assert!(matches!(
        config.strategies[&Capability::ExtendedThinking],
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn config_set_multiple_capabilities() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled { reason: "a".into() },
    );
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection { prompt: "b".into() },
    );
    config.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing { detail: "c".into() },
    );
    assert_eq!(config.strategies.len(), 3);
}

#[test]
fn config_clone_equality() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::Streaming,
        EmulationStrategy::Disabled { reason: "x".into() },
    );
    let config2 = config.clone();
    assert_eq!(config, config2);
}

// ═══════════════════════════════════════════════════════════════════════
// EmulationReport
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn report_default_is_empty() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn report_with_applied_not_empty() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    assert!(!report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn report_with_warnings_not_empty() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["warning".into()],
    };
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

#[test]
fn report_with_both_applied_and_warnings() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec!["some warning".into()],
    };
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

// ═══════════════════════════════════════════════════════════════════════
// EmulationEngine
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn engine_with_defaults_resolves_to_default_strategy() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert_eq!(s, default_strategy(&Capability::ExtendedThinking));
}

#[test]
fn engine_config_override_takes_precedence() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user choice".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn engine_resolve_unoverridden_falls_back() {
    let config = EmulationConfig::new();
    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// EmulationEngine::check_missing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn check_missing_empty_capabilities() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
}

#[test]
fn check_missing_emulatable_capability() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn check_missing_disabled_capability() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn check_missing_mixed() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::CodeExecution,
        Capability::Streaming,
    ]);
    assert_eq!(report.applied.len(), 2); // thinking + structured output
    assert_eq!(report.warnings.len(), 2); // code exec + streaming
}

#[test]
fn check_missing_with_config_override() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "simulate".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Capability + emulation interaction (apply)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn apply_system_prompt_injection_to_existing_system_message() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"));
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    assert!(sys.text_content().contains("Think step by step"));
    assert!(sys.text_content().contains("You are helpful."));
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn apply_system_prompt_injection_creates_system_message() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(
        conv.messages[0]
            .text_content()
            .contains("Think step by step")
    );
}

#[test]
fn apply_post_processing_does_not_mutate_conversation() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn apply_disabled_generates_warning_no_mutation() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "run code"));
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(conv, original);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn apply_empty_capabilities_empty_report() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);
    assert!(report.is_empty());
}

#[test]
fn apply_multiple_system_prompt_injections_append() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base."))
        .push(IrMessage::text(IrRole::User, "Hello"));
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );
    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Base."));
    assert!(sys_text.contains("Think step by step"));
    assert!(sys_text.to_lowercase().contains("image"));
}

#[test]
fn apply_mixed_strategies_in_one_pass() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Start."))
        .push(IrMessage::text(IrRole::User, "Do everything"));
    let engine = EmulationEngine::with_defaults();
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
fn apply_with_config_override_enables_disabled_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate execution.".into(),
        },
    );
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "run code"));
    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn apply_with_config_override_disables_emulatable_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "user disabled".into(),
        },
    );
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "think"));
    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Free-function apply_emulation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn free_apply_emulation_with_default_config() {
    let config = EmulationConfig::new();
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "hello"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

#[test]
fn free_apply_emulation_with_custom_config() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing {
            detail: "buffer".into(),
        },
    );
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "stream"));
    let report = apply_emulation(&config, &[Capability::Streaming], &mut conv);
    assert_eq!(report.applied.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Emulation vs native behavior differences
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn emulated_adds_system_prompt_native_does_not() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));
    let mut conv_emulated = original.clone();
    let mut conv_native = original.clone();

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv_emulated);
    engine.apply(&[], &mut conv_native);

    // Emulated path added a system message
    assert!(conv_emulated.system_message().is_some());
    // Native path (no emulation) did not
    assert!(conv_native.system_message().is_none());
}

#[test]
fn native_capability_not_in_fidelity_when_not_listed() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_distinguishes_native_from_emulated() {
    let native = vec![Capability::Streaming];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Emulation limits and degradation handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn disabled_strategy_warning_contains_capability_name() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.warnings[0].contains("CodeExecution"));
}

#[test]
fn disabled_strategy_warning_contains_reason() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution]);
    assert!(report.warnings[0].contains("Cannot safely emulate"));
}

#[test]
fn multiple_disabled_capabilities_multiple_warnings() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Logprobs,
    ]);
    assert_eq!(report.warnings.len(), 4);
    assert!(report.applied.is_empty());
}

#[test]
fn disabled_with_custom_reason_in_warning() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "custom reason here".into(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert!(report.warnings[0].contains("custom reason here"));
}

#[test]
fn has_unemulatable_true_when_disabled_caps_present() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::CodeExecution],
        &mut IrConversation::new().push(IrMessage::text(IrRole::User, "x")),
    );
    assert!(report.has_unemulatable());
}

#[test]
fn has_unemulatable_false_when_all_caps_emulated() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking],
        &mut IrConversation::new().push(IrMessage::text(IrRole::User, "x")),
    );
    assert!(!report.has_unemulatable());
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Serde roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_system_prompt_injection() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "hello".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_roundtrip_post_processing() {
    let s = EmulationStrategy::PostProcessing {
        detail: "validate JSON".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_roundtrip_disabled() {
    let s = EmulationStrategy::Disabled {
        reason: "not possible".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn serde_roundtrip_emulation_config_with_multiple_entries() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "step by step".into(),
        },
    );
    config.set(
        Capability::StructuredOutputJsonSchema,
        EmulationStrategy::PostProcessing {
            detail: "parse JSON".into(),
        },
    );
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
fn serde_roundtrip_empty_config() {
    let config = EmulationConfig::new();
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn serde_roundtrip_emulation_report_full() {
    let report = EmulationReport {
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
                    detail: "truncate".into(),
                },
            },
        ],
        warnings: vec!["CodeExecution disabled".into(), "Streaming disabled".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn serde_roundtrip_empty_report() {
    let report = EmulationReport::default();
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
            detail: "validate".into(),
        },
    };
    let json = serde_json::to_string(&label).unwrap();
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn serde_roundtrip_emulation_entry() {
    let entry = EmulationEntry {
        capability: Capability::ImageInput,
        strategy: emulate_image_input(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let decoded: EmulationEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, decoded);
}

#[test]
fn serde_strategy_tagged_with_type_field() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "system_prompt_injection");
}

#[test]
fn serde_strategy_post_processing_tagged() {
    let s = EmulationStrategy::PostProcessing {
        detail: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "post_processing");
}

#[test]
fn serde_strategy_disabled_tagged() {
    let s = EmulationStrategy::Disabled {
        reason: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "disabled");
}

#[test]
fn serde_fidelity_label_tagged_with_fidelity_field() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["fidelity"], "native");
}

#[test]
fn serde_fidelity_label_emulated_tagged() {
    let label = FidelityLabel::Emulated {
        strategy: EmulationStrategy::Disabled { reason: "x".into() },
    };
    let json = serde_json::to_string(&label).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["fidelity"], "emulated");
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn apply_same_capability_twice() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Hi."))
        .push(IrMessage::text(IrRole::User, "Go"));
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ExtendedThinking],
        &mut conv,
    );
    // Both are applied
    assert_eq!(report.applied.len(), 2);
    // System message gets two injections
    let sys_text = conv.system_message().unwrap().text_content();
    let count = sys_text.matches("Think step by step").count();
    assert_eq!(count, 2);
}

#[test]
fn apply_same_disabled_capability_twice() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[Capability::CodeExecution, Capability::CodeExecution]);
    assert_eq!(report.warnings.len(), 2);
}

#[test]
fn apply_to_empty_conversation() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert_eq!(conv.messages.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

#[test]
fn apply_preserves_user_messages() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "My question"))
        .push(IrMessage::text(IrRole::Assistant, "My answer"))
        .push(IrMessage::text(IrRole::User, "Follow-up"));
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    // System message prepended, all others intact
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[1].text_content(), "My question");
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[3].role, IrRole::User);
    assert_eq!(conv.messages[3].text_content(), "Follow-up");
}

#[test]
fn apply_multiple_injections_only_one_system_message() {
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::ImageInput,
            Capability::StopSequences,
        ],
        &mut conv,
    );
    // Only two system prompt injections (stop sequences is post-processing)
    let sys_count = conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .count();
    assert_eq!(sys_count, 1);
}

#[test]
fn system_prompt_injection_appends_to_existing_content_blocks() {
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Original."))
        .push(IrMessage::text(IrRole::User, "Ask"));
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    let sys = conv.system_message().unwrap();
    // Should have 2 content blocks: original text + injected text
    assert_eq!(sys.content.len(), 2);
}

#[test]
fn engine_clone() {
    let engine = EmulationEngine::with_defaults();
    let engine2 = engine.clone();
    let s1 = engine.resolve_strategy(&Capability::ExtendedThinking);
    let s2 = engine2.resolve_strategy(&Capability::ExtendedThinking);
    assert_eq!(s1, s2);
}

#[test]
fn engine_debug_impl() {
    let engine = EmulationEngine::with_defaults();
    let debug = format!("{engine:?}");
    assert!(debug.contains("EmulationEngine"));
}

#[test]
fn config_debug_impl() {
    let config = EmulationConfig::new();
    let debug = format!("{config:?}");
    assert!(debug.contains("EmulationConfig"));
}

#[test]
fn report_debug_impl() {
    let report = EmulationReport::default();
    let debug = format!("{report:?}");
    assert!(debug.contains("EmulationReport"));
}

#[test]
fn entry_debug_impl() {
    let entry = EmulationEntry {
        capability: Capability::Streaming,
        strategy: EmulationStrategy::Disabled { reason: "x".into() },
    };
    let debug = format!("{entry:?}");
    assert!(debug.contains("EmulationEntry"));
}

#[test]
fn empty_prompt_system_prompt_injection() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: String::new(),
        },
    );
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "test"));
    let engine = EmulationEngine::new(config);
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(report.applied.len(), 1);
    assert!(conv.system_message().is_some());
}

#[test]
fn empty_reason_disabled_strategy() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: String::new(),
        },
    );
    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::ExtendedThinking]);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn strategy_with_very_long_prompt() {
    let long_prompt = "x".repeat(10_000);
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: long_prompt.clone(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    if let EmulationStrategy::SystemPromptInjection { prompt } = decoded {
        assert_eq!(prompt.len(), 10_000);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn strategy_with_unicode_prompt() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "日本語テスト 🤖 émulation".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

#[test]
fn strategy_with_special_chars_in_detail() {
    let s = EmulationStrategy::PostProcessing {
        detail: "parse \"JSON\" with {braces} and [brackets]".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let decoded: EmulationStrategy = serde_json::from_str(&json).unwrap();
    assert_eq!(s, decoded);
}

// ═══════════════════════════════════════════════════════════════════════
// Capability + emulation + negotiation interaction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn negotiation_emulatable_feeds_into_emulation_engine() {
    // Backend supports streaming natively, but not extended thinking
    let mut manifest: CapabilityManifest = BTreeMap::new();
    manifest.insert(Capability::Streaming, CoreSupportLevel::Native);
    manifest.insert(Capability::ExtendedThinking, CoreSupportLevel::Emulated);

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ExtendedThinking,
                min_support: MinSupport::Emulated,
            },
        ],
    };

    let neg = negotiate(&manifest, &reqs);
    assert!(neg.is_compatible());
    assert_eq!(neg.native, vec![Capability::Streaming]);
    assert_eq!(neg.emulatable, vec![Capability::ExtendedThinking]);

    // Now apply emulation only for the emulatable capabilities
    let engine = EmulationEngine::with_defaults();
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are a bot."))
        .push(IrMessage::text(IrRole::User, "Hi"));
    let emu_report = engine.apply(&neg.emulatable, &mut conv);
    assert_eq!(emu_report.applied.len(), 1);

    // Compute fidelity
    let labels = compute_fidelity(&neg.native, &emu_report);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn unsupported_capabilities_produce_disabled_emulation() {
    let manifest: CapabilityManifest = BTreeMap::new();
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::CodeExecution,
            min_support: MinSupport::Emulated,
        }],
    };
    let neg = negotiate(&manifest, &reqs);
    assert!(!neg.is_compatible());

    // Even emulation engine can't help
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&neg.unsupported);
    assert!(report.has_unemulatable());
}

#[test]
fn all_capabilities_checked_against_can_emulate() {
    let capabilities = [
        Capability::Streaming,
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
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ];
    let emulatable_count = capabilities.iter().filter(|c| can_emulate(c)).count();
    // At least some are emulatable, some are not
    assert!(emulatable_count > 0);
    assert!(emulatable_count < capabilities.len());
}

#[test]
fn check_missing_matches_can_emulate_for_all_caps() {
    let engine = EmulationEngine::with_defaults();
    let capabilities = [
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
        Capability::CodeExecution,
        Capability::Streaming,
        Capability::ToolUse,
        Capability::Logprobs,
    ];
    let report = engine.check_missing(&capabilities);

    for entry in &report.applied {
        assert!(
            can_emulate(&entry.capability),
            "{:?} is in applied but can_emulate returns false",
            entry.capability
        );
    }
}

#[test]
fn compatibility_report_from_fully_emulated_negotiation() {
    let mut manifest: CapabilityManifest = BTreeMap::new();
    manifest.insert(Capability::ToolRead, CoreSupportLevel::Emulated);
    manifest.insert(Capability::ToolWrite, CoreSupportLevel::Emulated);

    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
        ],
    };

    let neg = negotiate(&manifest, &reqs);
    let report = generate_report(&neg);
    assert!(report.compatible);
    assert_eq!(report.emulated_count, 2);
    assert!(report.summary.contains("fully compatible"));
}

#[test]
fn compute_fidelity_btree_ordering() {
    let native = vec![Capability::ToolWrite, Capability::Streaming];
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: emulate_extended_thinking(),
        }],
        warnings: vec![],
    };
    let labels = compute_fidelity(&native, &report);
    // BTreeMap keys are sorted
    let keys: Vec<_> = labels.keys().collect();
    for i in 1..keys.len() {
        assert!(keys[i - 1] < keys[i], "BTreeMap keys should be sorted");
    }
}

#[test]
fn apply_all_emulatable_capabilities() {
    let emulatable = [
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ];
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base."))
        .push(IrMessage::text(IrRole::User, "Go"));
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&emulatable, &mut conv);
    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn check_missing_consistent_with_apply() {
    let caps = vec![
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::StopSequences,
    ];
    let engine = EmulationEngine::with_defaults();
    let check_report = engine.check_missing(&caps);
    let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "test"));
    let apply_report = engine.apply(&caps, &mut conv);
    assert_eq!(check_report.applied.len(), apply_report.applied.len());
    assert_eq!(check_report.warnings.len(), apply_report.warnings.len());
}

#[test]
fn config_deterministic_serialization() {
    let mut config1 = EmulationConfig::new();
    config1.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection { prompt: "a".into() },
    );
    config1.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled { reason: "b".into() },
    );

    let mut config2 = EmulationConfig::new();
    // Insert in reverse order — BTreeMap should produce same JSON
    config2.set(
        Capability::CodeExecution,
        EmulationStrategy::Disabled { reason: "b".into() },
    );
    config2.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection { prompt: "a".into() },
    );

    let json1 = serde_json::to_string(&config1).unwrap();
    let json2 = serde_json::to_string(&config2).unwrap();
    assert_eq!(json1, json2);
}
