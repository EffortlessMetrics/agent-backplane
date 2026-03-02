// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the abp-emulation crate covering strategy selection,
//! plan construction, fidelity labeling, config overrides, and edge cases.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{Capability, SupportLevel};
use abp_emulation::*;

// ── Helpers ────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

fn user_only_conv() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

fn multi_turn_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Base."))
        .push(IrMessage::text(IrRole::User, "Q1"))
        .push(IrMessage::text(IrRole::Assistant, "A1"))
        .push(IrMessage::text(IrRole::User, "Q2"))
}

fn emulatable_caps() -> Vec<Capability> {
    vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ]
}

fn disabled_caps() -> Vec<Capability> {
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

// ════════════════════════════════════════════════════════════════════════
// 1. System prompt injection strategies
// ════════════════════════════════════════════════════════════════════════

#[test]
fn spi_extended_thinking_injects_step_by_step() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Think step by step"));
}

#[test]
fn spi_image_input_injects_image_description_text() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ImageInput], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Image"));
    assert!(sys_text.contains("text descriptions"));
}

#[test]
fn spi_appends_block_to_existing_system_message() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys = conv.system_message().unwrap();
    assert_eq!(sys.content.len(), 2);
    assert!(sys.text_content().contains("You are helpful."));
    assert!(sys.text_content().contains("step by step"));
}

#[test]
fn spi_creates_system_message_when_absent() {
    let mut conv = user_only_conv();
    assert!(conv.system_message().is_none());

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn spi_injected_block_starts_with_newline() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys = conv.system_message().unwrap();
    if let IrContentBlock::Text { text } = &sys.content[1] {
        assert!(text.starts_with('\n'));
    } else {
        panic!("expected Text block");
    }
}

#[test]
fn spi_multiple_injections_compose_on_same_system_msg() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    let sys = conv.system_message().unwrap();
    assert_eq!(sys.content.len(), 3); // original + 2 injections
    let full = sys.text_content();
    assert!(full.contains("You are helpful."));
    assert!(full.contains("step by step"));
    assert!(full.contains("Image"));
}

#[test]
fn spi_does_not_add_extra_messages() {
    let mut conv = simple_conv();
    let before = conv.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.len(), before);
}

#[test]
fn spi_adds_one_message_when_no_system() {
    let mut conv = user_only_conv();
    assert_eq!(conv.len(), 1);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.len(), 2);
}

#[test]
fn spi_factory_emulate_structured_output_contains_json() {
    let s = emulate_structured_output();
    if let EmulationStrategy::SystemPromptInjection { prompt } = s {
        assert!(prompt.contains("JSON"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn spi_factory_emulate_code_execution_mentions_execute() {
    let s = emulate_code_execution();
    if let EmulationStrategy::SystemPromptInjection { prompt } = s {
        assert!(prompt.contains("execute code"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn spi_factory_emulate_extended_thinking_mentions_step() {
    let s = emulate_extended_thinking();
    if let EmulationStrategy::SystemPromptInjection { prompt } = s {
        assert!(prompt.contains("step by step"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn spi_factory_emulate_image_input_mentions_image() {
    let s = emulate_image_input();
    if let EmulationStrategy::SystemPromptInjection { prompt } = s {
        assert!(prompt.contains("Image"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn spi_preserves_user_messages_unchanged() {
    let mut conv = multi_turn_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let users = conv.messages_by_role(IrRole::User);
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].text_content(), "Q1");
    assert_eq!(users[1].text_content(), "Q2");
}

#[test]
fn spi_preserves_assistant_messages_unchanged() {
    let mut conv = multi_turn_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let assistants = conv.messages_by_role(IrRole::Assistant);
    assert_eq!(assistants.len(), 1);
    assert_eq!(assistants[0].text_content(), "A1");
}

#[test]
fn spi_preserves_tool_use_blocks() {
    let tool_msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "read_file".into(),
            input: serde_json::json!({"path": "a.txt"}),
        }],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Go"))
        .push(tool_msg);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(conv.tool_calls().len(), 1);
}

#[test]
fn spi_preserves_message_metadata() {
    let mut msg = IrMessage::text(IrRole::User, "Hi");
    msg.metadata.insert("key".into(), serde_json::json!("val"));
    let mut conv = IrConversation::new().push(msg);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let user_msg = &conv.messages[1];
    assert_eq!(
        user_msg.metadata.get("key"),
        Some(&serde_json::json!("val"))
    );
}

#[test]
fn spi_custom_strategy_override_uses_custom_prompt() {
    let custom = "CUSTOM THINKING INSTRUCTION";
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: custom.into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = simple_conv();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains(custom));
    assert!(!sys_text.contains("Think step by step before answering."));
}

// ════════════════════════════════════════════════════════════════════════
// 2. Post-processing strategies
// ════════════════════════════════════════════════════════════════════════

#[test]
fn pp_structured_output_default_is_post_processing() {
    let s = default_strategy(&Capability::StructuredOutputJsonSchema);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn pp_stop_sequences_default_is_post_processing() {
    let s = default_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn pp_factory_emulate_stop_sequences_mentions_stop() {
    let s = emulate_stop_sequences();
    if let EmulationStrategy::PostProcessing { detail } = s {
        assert!(detail.contains("stop sequence"));
    } else {
        panic!("expected PostProcessing");
    }
}

#[test]
fn pp_does_not_mutate_conversation() {
    let original = user_only_conv();
    let mut conv = original.clone();

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

    assert_eq!(conv, original);
}

#[test]
fn pp_stop_sequences_does_not_mutate() {
    let original = simple_conv();
    let mut conv = original.clone();

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StopSequences], &mut conv);

    assert_eq!(conv, original);
}

#[test]
fn pp_is_recorded_in_report() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn pp_preserves_message_count_multi_turn() {
    let len = multi_turn_conv().len();
    let mut conv = multi_turn_conv();

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

    assert_eq!(conv.len(), len);
}

#[test]
fn pp_combined_with_disabled_no_mutation() {
    let original = user_only_conv();
    let mut conv = original.clone();

    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[
            Capability::StructuredOutputJsonSchema,
            Capability::StopSequences,
            Capability::CodeExecution,
            Capability::Streaming,
        ],
        &mut conv,
    );

    assert_eq!(conv, original);
}

// ════════════════════════════════════════════════════════════════════════
// 3. Emulation plan construction (check_missing + apply agreement)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn plan_check_missing_matches_apply_count() {
    let engine = EmulationEngine::with_defaults();
    let caps = [Capability::ExtendedThinking, Capability::CodeExecution];

    let check = engine.check_missing(&caps);
    let mut conv = user_only_conv();
    let apply = engine.apply(&caps, &mut conv);

    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
}

#[test]
fn plan_check_missing_matches_apply_entries() {
    let engine = EmulationEngine::with_defaults();
    let caps = [
        Capability::ImageInput,
        Capability::Streaming,
        Capability::StopSequences,
    ];

    let check = engine.check_missing(&caps);
    let mut conv = user_only_conv();
    let apply = engine.apply(&caps, &mut conv);

    for (a, b) in check.applied.iter().zip(apply.applied.iter()) {
        assert_eq!(a.capability, b.capability);
        assert_eq!(a.strategy, b.strategy);
    }
}

#[test]
fn plan_check_missing_empty_returns_empty() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
}

#[test]
fn plan_check_missing_all_emulatable() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&emulatable_caps());

    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn plan_check_missing_all_disabled() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&disabled_caps());

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), disabled_caps().len());
}

#[test]
fn plan_check_missing_preserves_order() {
    let engine = EmulationEngine::with_defaults();
    let caps = [
        Capability::StopSequences,
        Capability::ImageInput,
        Capability::ExtendedThinking,
    ];
    let report = engine.check_missing(&caps);

    assert_eq!(report.applied[0].capability, Capability::StopSequences);
    assert_eq!(report.applied[1].capability, Capability::ImageInput);
    assert_eq!(report.applied[2].capability, Capability::ExtendedThinking);
}

#[test]
fn plan_check_missing_with_overrides() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "sim".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let report = engine.check_missing(&[Capability::CodeExecution]);

    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
}

#[test]
fn plan_resolve_strategy_default_fallback() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::Streaming);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn plan_resolve_strategy_config_override() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing {
            detail: "buffer".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::Streaming);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

// ════════════════════════════════════════════════════════════════════════
// 4. Capability emulation decisions (can_emulate / default_strategy)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn decision_extended_thinking_emulatable() {
    assert!(can_emulate(&Capability::ExtendedThinking));
}

#[test]
fn decision_structured_output_emulatable() {
    assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn decision_image_input_emulatable() {
    assert!(can_emulate(&Capability::ImageInput));
}

#[test]
fn decision_stop_sequences_emulatable() {
    assert!(can_emulate(&Capability::StopSequences));
}

#[test]
fn decision_code_execution_not_emulatable() {
    assert!(!can_emulate(&Capability::CodeExecution));
}

#[test]
fn decision_streaming_not_emulatable() {
    assert!(!can_emulate(&Capability::Streaming));
}

#[test]
fn decision_tool_use_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolUse));
}

#[test]
fn decision_tool_read_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn decision_tool_write_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolWrite));
}

#[test]
fn decision_tool_edit_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolEdit));
}

#[test]
fn decision_tool_bash_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolBash));
}

#[test]
fn decision_tool_glob_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolGlob));
}

#[test]
fn decision_tool_grep_not_emulatable() {
    assert!(!can_emulate(&Capability::ToolGrep));
}

#[test]
fn decision_logprobs_not_emulatable() {
    assert!(!can_emulate(&Capability::Logprobs));
}

#[test]
fn decision_seed_determinism_not_emulatable() {
    assert!(!can_emulate(&Capability::SeedDeterminism));
}

#[test]
fn decision_pdf_input_not_emulatable() {
    assert!(!can_emulate(&Capability::PdfInput));
}

#[test]
fn decision_mcp_client_not_emulatable() {
    assert!(!can_emulate(&Capability::McpClient));
}

#[test]
fn decision_mcp_server_not_emulatable() {
    assert!(!can_emulate(&Capability::McpServer));
}

#[test]
fn decision_session_resume_not_emulatable() {
    assert!(!can_emulate(&Capability::SessionResume));
}

#[test]
fn decision_session_fork_not_emulatable() {
    assert!(!can_emulate(&Capability::SessionFork));
}

#[test]
fn decision_checkpointing_not_emulatable() {
    assert!(!can_emulate(&Capability::Checkpointing));
}

#[test]
fn decision_all_emulatable_match_non_disabled_default() {
    for cap in emulatable_caps() {
        assert!(
            !matches!(default_strategy(&cap), EmulationStrategy::Disabled { .. }),
            "{cap:?} default should not be Disabled"
        );
    }
}

#[test]
fn decision_all_disabled_match_disabled_default() {
    for cap in disabled_caps() {
        assert!(
            matches!(default_strategy(&cap), EmulationStrategy::Disabled { .. }),
            "{cap:?} default should be Disabled"
        );
    }
}

#[test]
fn decision_default_extended_thinking_is_spi() {
    assert!(matches!(
        default_strategy(&Capability::ExtendedThinking),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn decision_default_structured_output_is_pp() {
    assert!(matches!(
        default_strategy(&Capability::StructuredOutputJsonSchema),
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn decision_default_code_execution_is_disabled() {
    assert!(matches!(
        default_strategy(&Capability::CodeExecution),
        EmulationStrategy::Disabled { .. }
    ));
}

#[test]
fn decision_default_image_input_is_spi() {
    assert!(matches!(
        default_strategy(&Capability::ImageInput),
        EmulationStrategy::SystemPromptInjection { .. }
    ));
}

#[test]
fn decision_default_stop_sequences_is_pp() {
    assert!(matches!(
        default_strategy(&Capability::StopSequences),
        EmulationStrategy::PostProcessing { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 5. Error handling for unsupported emulation (disabled)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn disabled_generates_warning() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("not emulated"));
}

#[test]
fn disabled_warning_contains_capability_name() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ToolUse], &mut conv);

    assert!(report.warnings[0].contains("ToolUse"));
}

#[test]
fn disabled_warning_contains_reason() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    assert!(report.warnings[0].contains("sandbox"));
}

#[test]
fn disabled_does_not_mutate_conversation() {
    let original = simple_conv();
    let mut conv = original.clone();

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::CodeExecution], &mut conv);

    assert_eq!(conv, original);
}

#[test]
fn disabled_all_caps_produce_all_warnings() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&disabled_caps(), &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), disabled_caps().len());
}

#[test]
fn disabled_has_unemulatable_true() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::Streaming], &mut conv);

    assert!(report.has_unemulatable());
}

#[test]
fn disabled_interleaved_with_emulatable() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::Streaming,
            Capability::ImageInput,
            Capability::CodeExecution,
            Capability::StopSequences,
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 3);
    assert_eq!(report.warnings.len(), 2);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[1].capability, Capability::ImageInput);
    assert_eq!(report.applied[2].capability, Capability::StopSequences);
}

#[test]
fn disabled_default_reason_for_generic_cap() {
    let s = default_strategy(&Capability::Streaming);
    if let EmulationStrategy::Disabled { reason } = s {
        assert!(reason.contains("No emulation available"));
    } else {
        panic!("expected Disabled");
    }
}

#[test]
fn disabled_code_execution_reason_mentions_sandbox() {
    let s = default_strategy(&Capability::CodeExecution);
    if let EmulationStrategy::Disabled { reason } = s {
        assert!(reason.contains("sandbox"));
    } else {
        panic!("expected Disabled");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 6. Integration with SupportLevel::Emulated
// ════════════════════════════════════════════════════════════════════════

#[test]
fn support_level_emulated_exists() {
    let _level = SupportLevel::Emulated;
}

#[test]
fn fidelity_native_for_natively_supported_caps() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);

    assert_eq!(labels.len(), 2);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
}

#[test]
fn fidelity_emulated_for_emulated_caps() {
    let mut conv = user_only_conv();
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
    let mut conv = user_only_conv();
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
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::CodeExecution],
        &mut conv,
    );

    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 1);
    assert!(labels.contains_key(&Capability::ExtendedThinking));
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn fidelity_emulated_overrides_native_for_same_cap() {
    let mut conv = user_only_conv();
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
fn fidelity_label_carries_strategy() {
    let mut conv = user_only_conv();
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
fn fidelity_empty_inputs_produces_empty_map() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn fidelity_btreemap_deterministic_serialization() {
    let mut conv1 = user_only_conv();
    let mut conv2 = user_only_conv();
    let engine = EmulationEngine::with_defaults();

    let r1 = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv1,
    );
    let r2 = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv2,
    );

    let l1 = compute_fidelity(&[Capability::Streaming], &r1);
    let l2 = compute_fidelity(&[Capability::Streaming], &r2);

    let j1 = serde_json::to_string(&l1).unwrap();
    let j2 = serde_json::to_string(&l2).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn fidelity_serde_roundtrip() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);
    let labels = compute_fidelity(&[Capability::Streaming], &report);

    let json = serde_json::to_string(&labels).unwrap();
    let decoded: BTreeMap<Capability, FidelityLabel> = serde_json::from_str(&json).unwrap();
    assert_eq!(labels, decoded);
}

// ════════════════════════════════════════════════════════════════════════
// 7. Edge cases
// ════════════════════════════════════════════════════════════════════════

// -- Empty strategies --

#[test]
fn edge_empty_caps_no_mutation() {
    let original = simple_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);

    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn edge_empty_caps_on_empty_conversation() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);

    assert!(report.is_empty());
    assert!(conv.is_empty());
}

#[test]
fn edge_apply_to_empty_conversation_creates_system() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

// -- Conflicting strategies --

#[test]
fn edge_config_override_overrides_previous_set() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "first".into(),
        },
    );
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "second".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    if let EmulationStrategy::PostProcessing { detail } = s {
        assert_eq!(detail, "second");
    } else {
        panic!("expected second override (PostProcessing)");
    }
}

#[test]
fn edge_config_override_enables_disabled_cap() {
    let mut config = EmulationConfig::new();
    config.set(Capability::CodeExecution, emulate_code_execution());

    let engine = EmulationEngine::new(config);
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn edge_config_override_disables_emulatable_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ImageInput,
        EmulationStrategy::Disabled {
            reason: "policy".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ImageInput], &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn edge_override_only_affects_targeted_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    // Other caps still use defaults
    let s = engine.resolve_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn edge_repeated_same_capability_applies_each_time() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::ExtendedThinking,
            Capability::ExtendedThinking,
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 3);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.content.len(), 4); // original + 3 injected
}

#[test]
fn edge_multiple_overrides_in_config() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Pretend.".into(),
        },
    );
    config.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing {
            detail: "buffer".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = user_only_conv();
    let report = engine.apply(
        &[Capability::CodeExecution, Capability::Streaming],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    assert!(report.warnings.is_empty());
}

#[test]
fn edge_large_number_of_overrides() {
    let mut config = EmulationConfig::new();
    for cap in disabled_caps() {
        config.set(
            cap,
            EmulationStrategy::SystemPromptInjection {
                prompt: "overridden".into(),
            },
        );
    }

    let engine = EmulationEngine::new(config);
    let mut conv = user_only_conv();
    let report = engine.apply(&disabled_caps(), &mut conv);

    assert_eq!(report.applied.len(), disabled_caps().len());
    assert!(report.warnings.is_empty());
}

#[test]
fn edge_config_serde_empty_roundtrip() {
    let config = EmulationConfig::new();
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn edge_config_serde_with_all_variant_types() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "think".into(),
        },
    );
    config.set(
        Capability::StopSequences,
        EmulationStrategy::PostProcessing {
            detail: "truncate".into(),
        },
    );
    config.set(
        Capability::Streaming,
        EmulationStrategy::Disabled {
            reason: "no".into(),
        },
    );

    let json = serde_json::to_string_pretty(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

// -- Conversation shape edge cases --

#[test]
fn edge_conversation_with_image_block_preserved() {
    let img_msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64".into(),
            },
        ],
    );
    let mut conv = IrConversation::new().push(img_msg);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ImageInput], &mut conv);

    let user_msg = &conv.messages[1];
    assert_eq!(user_msg.content.len(), 2);
    assert!(matches!(user_msg.content[1], IrContentBlock::Image { .. }));
}

#[test]
fn edge_conversation_with_thinking_block_preserved() {
    let asst = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "answer".into(),
            },
        ],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Q"))
        .push(asst);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let a = conv.last_assistant().unwrap();
    assert!(matches!(a.content[0], IrContentBlock::Thinking { .. }));
}

#[test]
fn edge_conversation_with_tool_result_preserved() {
    let tool_result = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text {
                text: "file content".into(),
            }],
            is_error: false,
        }],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Go"))
        .push(tool_result);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let tool_msg = conv.messages_by_role(IrRole::Tool);
    assert_eq!(tool_msg.len(), 1);
}

// -- Serde edge cases --

#[test]
fn edge_strategy_serde_roundtrip_all_variants() {
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
fn edge_strategy_json_type_tags() {
    let spi = EmulationStrategy::SystemPromptInjection { prompt: "x".into() };
    assert!(
        serde_json::to_string(&spi)
            .unwrap()
            .contains("\"type\":\"system_prompt_injection\"")
    );

    let pp = EmulationStrategy::PostProcessing { detail: "x".into() };
    assert!(
        serde_json::to_string(&pp)
            .unwrap()
            .contains("\"type\":\"post_processing\"")
    );

    let dis = EmulationStrategy::Disabled { reason: "x".into() };
    assert!(
        serde_json::to_string(&dis)
            .unwrap()
            .contains("\"type\":\"disabled\"")
    );
}

#[test]
fn edge_fidelity_label_serde_roundtrip() {
    let labels = vec![
        FidelityLabel::Native,
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::SystemPromptInjection { prompt: "t".into() },
        },
        FidelityLabel::Emulated {
            strategy: EmulationStrategy::PostProcessing { detail: "d".into() },
        },
    ];
    for l in &labels {
        let json = serde_json::to_string(l).unwrap();
        let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(*l, decoded);
    }
}

#[test]
fn edge_fidelity_native_json_tag() {
    let json = serde_json::to_string(&FidelityLabel::Native).unwrap();
    assert!(json.contains("\"fidelity\":\"native\""));
}

#[test]
fn edge_fidelity_emulated_json_tag() {
    let label = FidelityLabel::Emulated {
        strategy: EmulationStrategy::Disabled { reason: "r".into() },
    };
    let json = serde_json::to_string(&label).unwrap();
    assert!(json.contains("\"fidelity\":\"emulated\""));
}

#[test]
fn edge_report_serde_roundtrip() {
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
                    detail: "stop".into(),
                },
            },
        ],
        warnings: vec!["w1".into(), "w2".into()],
    };
    let json = serde_json::to_string(&report).unwrap();
    let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(report, decoded);
}

#[test]
fn edge_factory_strategies_serde_roundtrip() {
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
fn edge_named_strategies_all_distinct() {
    let strategies = [
        emulate_structured_output(),
        emulate_code_execution(),
        emulate_extended_thinking(),
        emulate_image_input(),
        emulate_stop_sequences(),
    ];
    for i in 0..strategies.len() {
        for j in (i + 1)..strategies.len() {
            assert_ne!(strategies[i], strategies[j]);
        }
    }
}

// -- Report edge cases --

#[test]
fn edge_report_default_is_empty() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn edge_report_with_only_applied_not_empty() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection { prompt: "t".into() },
        }],
        warnings: vec![],
    };
    assert!(!report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn edge_report_with_only_warnings_not_empty() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["w".into()],
    };
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

// -- Thread safety --

#[test]
fn edge_engine_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<EmulationEngine>();
}

#[test]
fn edge_engine_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<EmulationEngine>();
}

#[test]
fn edge_config_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EmulationConfig>();
}

#[test]
fn edge_report_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EmulationReport>();
}

#[test]
fn edge_strategy_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EmulationStrategy>();
}

#[test]
fn edge_fidelity_label_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FidelityLabel>();
}

// -- Clone independence --

#[test]
fn edge_engine_clone_produces_equal_results() {
    let e1 = EmulationEngine::with_defaults();
    let e2 = e1.clone();

    let mut c1 = user_only_conv();
    let mut c2 = user_only_conv();

    let r1 = e1.apply(&[Capability::ExtendedThinking], &mut c1);
    let r2 = e2.apply(&[Capability::ExtendedThinking], &mut c2);

    assert_eq!(r1.applied.len(), r2.applied.len());
    assert_eq!(c1, c2);
}

// -- Free function --

#[test]
fn edge_free_function_matches_engine() {
    let config = EmulationConfig::new();
    let caps = [Capability::ExtendedThinking, Capability::CodeExecution];

    let mut conv1 = simple_conv();
    let r1 = apply_emulation(&config, &caps, &mut conv1);

    let engine = EmulationEngine::new(config.clone());
    let mut conv2 = simple_conv();
    let r2 = engine.apply(&caps, &mut conv2);

    assert_eq!(r1.applied.len(), r2.applied.len());
    assert_eq!(r1.warnings.len(), r2.warnings.len());
    assert_eq!(conv1, conv2);
}

#[test]
fn edge_free_function_with_overrides() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "simulate".into(),
        },
    );

    let mut conv = user_only_conv();
    let report = apply_emulation(&config, &[Capability::CodeExecution], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(conv.system_message().is_some());
}

// -- Capabilities applied in input order --

#[test]
fn edge_report_entries_match_input_order() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::StopSequences,
            Capability::ExtendedThinking,
            Capability::ImageInput,
        ],
        &mut conv,
    );

    assert_eq!(report.applied[0].capability, Capability::StopSequences);
    assert_eq!(report.applied[1].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[2].capability, Capability::ImageInput);
}

// -- Apply all known caps --

#[test]
fn edge_apply_all_known_capabilities() {
    let mut all = emulatable_caps();
    all.extend(disabled_caps());

    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&all, &mut conv);

    let total = report.applied.len() + report.warnings.len();
    assert_eq!(total, all.len());
}

// -- Config btreemap deterministic --

#[test]
fn edge_config_btreemap_deterministic() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ToolUse,
        EmulationStrategy::Disabled { reason: "a".into() },
    );
    config.set(
        Capability::Streaming,
        EmulationStrategy::Disabled { reason: "b".into() },
    );

    let j1 = serde_json::to_string(&config).unwrap();
    let j2 = serde_json::to_string(&config).unwrap();
    assert_eq!(j1, j2);
}
