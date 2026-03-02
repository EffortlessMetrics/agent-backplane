// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for the ABP emulation engine.

use abp_core::Capability;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
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
        .push(IrMessage::text(IrRole::System, "Base system prompt."))
        .push(IrMessage::text(IrRole::User, "First question"))
        .push(IrMessage::text(IrRole::Assistant, "First answer"))
        .push(IrMessage::text(IrRole::User, "Follow-up question"))
}

/// All capabilities that have emulatable defaults.
fn emulatable_caps() -> Vec<Capability> {
    vec![
        Capability::ExtendedThinking,
        Capability::StructuredOutputJsonSchema,
        Capability::ImageInput,
        Capability::StopSequences,
    ]
}

/// All capabilities that are disabled by default.
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
// 1. Default emulation engine has standard strategies
// ════════════════════════════════════════════════════════════════════════

#[test]
fn default_engine_has_empty_config() {
    let engine = EmulationEngine::with_defaults();
    // Resolves via default_strategy, not config overrides
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn default_engine_extended_thinking_prompt_content() {
    let s = default_strategy(&Capability::ExtendedThinking);
    if let EmulationStrategy::SystemPromptInjection { prompt } = s {
        assert!(prompt.contains("step by step"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn default_engine_structured_output_detail_content() {
    let s = default_strategy(&Capability::StructuredOutputJsonSchema);
    if let EmulationStrategy::PostProcessing { detail } = s {
        assert!(detail.contains("JSON"));
    } else {
        panic!("expected PostProcessing");
    }
}

#[test]
fn default_engine_code_execution_reason_content() {
    let s = default_strategy(&Capability::CodeExecution);
    if let EmulationStrategy::Disabled { reason } = s {
        assert!(reason.contains("sandbox"));
    } else {
        panic!("expected Disabled");
    }
}

#[test]
fn default_engine_image_input_prompt_content() {
    let s = default_strategy(&Capability::ImageInput);
    if let EmulationStrategy::SystemPromptInjection { prompt } = s {
        assert!(prompt.contains("Image"));
    } else {
        panic!("expected SystemPromptInjection");
    }
}

#[test]
fn default_engine_stop_sequences_detail_content() {
    let s = default_strategy(&Capability::StopSequences);
    if let EmulationStrategy::PostProcessing { detail } = s {
        assert!(detail.contains("stop sequence"));
    } else {
        panic!("expected PostProcessing");
    }
}

#[test]
fn default_engine_all_disabled_caps_return_disabled() {
    for cap in disabled_caps() {
        let s = default_strategy(&cap);
        assert!(
            matches!(s, EmulationStrategy::Disabled { .. }),
            "{cap:?} should be disabled by default"
        );
    }
}

#[test]
fn default_engine_all_emulatable_caps_return_non_disabled() {
    for cap in emulatable_caps() {
        let s = default_strategy(&cap);
        assert!(
            !matches!(s, EmulationStrategy::Disabled { .. }),
            "{cap:?} should be emulatable by default"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════
// 2. Emulation applies system prompt for unsupported capabilities
// ════════════════════════════════════════════════════════════════════════

#[test]
fn system_prompt_injection_appends_to_existing_system_msg() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys = conv.system_message().unwrap();
    // Original content preserved, new content appended
    assert!(sys.text_content().contains("You are helpful."));
    assert!(sys.text_content().contains("step by step"));
    assert!(sys.content.len() == 2, "should have 2 content blocks");
}

#[test]
fn system_prompt_injection_prepends_new_system_msg() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn system_prompt_injection_newline_separated() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys = conv.system_message().unwrap();
    // The injected block starts with \n
    if let IrContentBlock::Text { text } = &sys.content[1] {
        assert!(text.starts_with('\n'));
    } else {
        panic!("expected Text block");
    }
}

#[test]
fn image_input_injects_system_prompt() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ImageInput], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Image"));
}

// ════════════════════════════════════════════════════════════════════════
// 3. Emulation report tracks what was applied
// ════════════════════════════════════════════════════════════════════════

#[test]
fn report_applied_entries_match_capabilities_order() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let caps = [Capability::ImageInput, Capability::ExtendedThinking];
    let report = engine.apply(&caps, &mut conv);

    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.applied[0].capability, Capability::ImageInput);
    assert_eq!(report.applied[1].capability, Capability::ExtendedThinking);
}

#[test]
fn report_applied_entries_carry_correct_strategy() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn report_warnings_contain_capability_name() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ToolUse], &mut conv);

    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("ToolUse"));
}

#[test]
fn report_warnings_contain_reason() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::CodeExecution], &mut conv);

    assert!(report.warnings[0].contains("not emulated"));
    assert!(report.warnings[0].contains("sandbox"));
}

#[test]
fn report_is_empty_when_truly_empty() {
    let report = EmulationReport::default();
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn report_not_empty_with_applied() {
    let report = EmulationReport {
        applied: vec![EmulationEntry {
            capability: Capability::ExtendedThinking,
            strategy: EmulationStrategy::SystemPromptInjection {
                prompt: "test".into(),
            },
        }],
        warnings: vec![],
    };
    assert!(!report.is_empty());
}

#[test]
fn report_not_empty_with_warnings() {
    let report = EmulationReport {
        applied: vec![],
        warnings: vec!["something".into()],
    };
    assert!(!report.is_empty());
    assert!(report.has_unemulatable());
}

// ════════════════════════════════════════════════════════════════════════
// 4. Emulation preserves existing conversation content
// ════════════════════════════════════════════════════════════════════════

#[test]
fn preserves_user_messages() {
    let mut conv = multi_turn_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let user_msgs: Vec<_> = conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::User)
        .collect();
    assert_eq!(user_msgs.len(), 2);
    assert_eq!(user_msgs[0].text_content(), "First question");
    assert_eq!(user_msgs[1].text_content(), "Follow-up question");
}

#[test]
fn preserves_assistant_messages() {
    let mut conv = multi_turn_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let asst_msgs = conv.messages_by_role(IrRole::Assistant);
    assert_eq!(asst_msgs.len(), 1);
    assert_eq!(asst_msgs[0].text_content(), "First answer");
}

#[test]
fn preserves_message_order() {
    let mut conv = multi_turn_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
    assert_eq!(conv.messages[2].role, IrRole::Assistant);
    assert_eq!(conv.messages[3].role, IrRole::User);
}

#[test]
fn preserves_original_system_prompt_text() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys = conv.system_message().unwrap();
    // First content block is the original
    if let IrContentBlock::Text { text } = &sys.content[0] {
        assert_eq!(text, "You are helpful.");
    } else {
        panic!("expected Text");
    }
}

#[test]
fn preserves_tool_content_blocks() {
    let tool_msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "read_file".into(),
            input: serde_json::json!({"path": "a.txt"}),
        }],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Read it"))
        .push(tool_msg);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let tool_blocks = conv.tool_calls();
    assert_eq!(tool_blocks.len(), 1);
}

#[test]
fn preserves_message_metadata() {
    let mut msg = IrMessage::text(IrRole::User, "Hello");
    msg.metadata
        .insert("custom_key".into(), serde_json::json!("custom_val"));
    let mut conv = IrConversation::new().push(msg);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    // User message is now at index 1 (system inserted at 0)
    let user_msg = &conv.messages[1];
    assert_eq!(
        user_msg.metadata.get("custom_key"),
        Some(&serde_json::json!("custom_val"))
    );
}

// ════════════════════════════════════════════════════════════════════════
// 5. Multiple capabilities emulated simultaneously
// ════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_system_injections_all_appear() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("step by step"));
    assert!(sys_text.contains("Image"));
}

#[test]
fn mixed_injection_and_post_processing() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::StopSequences],
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

#[test]
fn all_four_emulatable_caps_at_once() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&emulatable_caps(), &mut conv);

    assert_eq!(report.applied.len(), 4);
    assert!(report.warnings.is_empty());
}

#[test]
fn mix_of_emulatable_and_disabled() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::Streaming,
            Capability::ImageInput,
            Capability::CodeExecution,
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 2);
    assert_eq!(report.warnings.len(), 2);
}

// ════════════════════════════════════════════════════════════════════════
// 6. Emulation with no capabilities is a no-op
// ════════════════════════════════════════════════════════════════════════

#[test]
fn empty_capabilities_no_mutation() {
    let original = simple_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);

    assert!(report.is_empty());
    assert_eq!(conv, original);
}

#[test]
fn empty_capabilities_on_empty_conversation() {
    let mut conv = IrConversation::new();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[], &mut conv);

    assert!(report.is_empty());
    assert!(conv.is_empty());
}

#[test]
fn check_missing_empty_caps_returns_empty_report() {
    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&[]);
    assert!(report.is_empty());
}

// ════════════════════════════════════════════════════════════════════════
// 7. Custom emulation strategies
// ════════════════════════════════════════════════════════════════════════

#[test]
fn custom_strategy_overrides_default() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::PostProcessing {
            detail: "custom post-processing for thinking".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let s = engine.resolve_strategy(&Capability::ExtendedThinking);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn custom_strategy_enables_disabled_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::Streaming,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Simulate streaming by chunking output.".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::Streaming], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(report.warnings.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn custom_strategy_disables_emulatable_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "disabled by user".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = user_only_conv();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    assert!(report.applied.is_empty());
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn custom_strategy_only_affects_specified_cap() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::Disabled {
            reason: "off".into(),
        },
    );

    let engine = EmulationEngine::new(config);
    // ImageInput should still use default (system prompt injection)
    let s = engine.resolve_strategy(&Capability::ImageInput);
    assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
}

#[test]
fn multiple_custom_overrides() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Pretend to execute.".into(),
        },
    );
    config.set(
        Capability::Streaming,
        EmulationStrategy::PostProcessing {
            detail: "buffer and flush".into(),
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
fn config_set_overwrites_previous_override() {
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
        panic!("expected PostProcessing from second override");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 8. Strategy ordering/priority
// ════════════════════════════════════════════════════════════════════════

#[test]
fn config_override_takes_priority_over_default() {
    let custom_prompt = "Custom thinking instructions.";
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: custom_prompt.into(),
        },
    );

    let engine = EmulationEngine::new(config);
    let mut conv = simple_conv();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains(custom_prompt));
    // Should NOT contain the default prompt
    assert!(!sys_text.contains("Think step by step before answering."));
}

#[test]
fn capabilities_applied_in_input_order() {
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

#[test]
fn injection_order_matches_cap_order_in_system_prompt() {
    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    let sys = conv.system_message().unwrap();
    // Original + 2 injected blocks
    assert_eq!(sys.content.len(), 3);
}

#[test]
fn disabled_caps_interleaved_dont_affect_applied_order() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::Streaming, // disabled
            Capability::ImageInput,
            Capability::CodeExecution, // disabled
            Capability::StopSequences,
        ],
        &mut conv,
    );

    assert_eq!(report.applied.len(), 3);
    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert_eq!(report.applied[1].capability, Capability::ImageInput);
    assert_eq!(report.applied[2].capability, Capability::StopSequences);
    assert_eq!(report.warnings.len(), 2);
}

// ════════════════════════════════════════════════════════════════════════
// 9. Emulation for specific capabilities
// ════════════════════════════════════════════════════════════════════════

#[test]
fn extended_thinking_injects_step_by_step() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("Think step by step"));
}

#[test]
fn image_input_mentions_text_descriptions() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ImageInput], &mut conv);

    let sys_text = conv.system_message().unwrap().text_content();
    assert!(sys_text.contains("text descriptions"));
}

#[test]
fn structured_output_is_post_processing_not_injection() {
    let original = user_only_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

    // Post-processing doesn't mutate conversation
    assert_eq!(conv, original);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

#[test]
fn stop_sequences_is_post_processing() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::StopSequences);
    assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
}

#[test]
fn code_execution_disabled_by_default() {
    let engine = EmulationEngine::with_defaults();
    let s = engine.resolve_strategy(&Capability::CodeExecution);
    assert!(matches!(s, EmulationStrategy::Disabled { .. }));
}

#[test]
fn pdf_input_disabled_by_default() {
    assert!(!can_emulate(&Capability::PdfInput));
}

#[test]
fn logprobs_disabled_by_default() {
    assert!(!can_emulate(&Capability::Logprobs));
}

#[test]
fn seed_determinism_disabled_by_default() {
    assert!(!can_emulate(&Capability::SeedDeterminism));
}

#[test]
fn tool_read_disabled_by_default() {
    assert!(!can_emulate(&Capability::ToolRead));
}

#[test]
fn tool_write_disabled_by_default() {
    assert!(!can_emulate(&Capability::ToolWrite));
}

#[test]
fn session_resume_disabled_by_default() {
    assert!(!can_emulate(&Capability::SessionResume));
}

#[test]
fn mcp_client_disabled_by_default() {
    assert!(!can_emulate(&Capability::McpClient));
}

// ════════════════════════════════════════════════════════════════════════
// 10. Emulation engine serialization
// ════════════════════════════════════════════════════════════════════════

#[test]
fn config_serde_empty() {
    let config = EmulationConfig::new();
    let json = serde_json::to_string(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn config_serde_with_overrides() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ExtendedThinking,
        EmulationStrategy::SystemPromptInjection {
            prompt: "Think deeply.".into(),
        },
    );
    config.set(
        Capability::Streaming,
        EmulationStrategy::Disabled {
            reason: "nope".into(),
        },
    );
    config.set(
        Capability::StopSequences,
        EmulationStrategy::PostProcessing {
            detail: "truncate".into(),
        },
    );

    let json = serde_json::to_string_pretty(&config).unwrap();
    let decoded: EmulationConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config, decoded);
}

#[test]
fn strategy_json_contains_type_tag() {
    let s = EmulationStrategy::SystemPromptInjection {
        prompt: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"system_prompt_injection\""));
}

#[test]
fn strategy_post_processing_json_tag() {
    let s = EmulationStrategy::PostProcessing {
        detail: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"post_processing\""));
}

#[test]
fn strategy_disabled_json_tag() {
    let s = EmulationStrategy::Disabled {
        reason: "test".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"disabled\""));
}

#[test]
fn report_serde_roundtrip_with_mixed_entries() {
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
fn fidelity_label_native_json() {
    let label = FidelityLabel::Native;
    let json = serde_json::to_string(&label).unwrap();
    assert!(json.contains("\"fidelity\":\"native\""));
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

#[test]
fn fidelity_label_emulated_json() {
    let label = FidelityLabel::Emulated {
        strategy: EmulationStrategy::SystemPromptInjection { prompt: "x".into() },
    };
    let json = serde_json::to_string(&label).unwrap();
    assert!(json.contains("\"fidelity\":\"emulated\""));
    let decoded: FidelityLabel = serde_json::from_str(&json).unwrap();
    assert_eq!(label, decoded);
}

// ════════════════════════════════════════════════════════════════════════
// 11. Emulation report structure
// ════════════════════════════════════════════════════════════════════════

#[test]
fn report_default_is_empty() {
    let report = EmulationReport::default();
    assert!(report.applied.is_empty());
    assert!(report.warnings.is_empty());
    assert!(report.is_empty());
    assert!(!report.has_unemulatable());
}

#[test]
fn check_missing_produces_same_structure_as_apply() {
    let engine = EmulationEngine::with_defaults();
    let caps = [
        Capability::ExtendedThinking,
        Capability::CodeExecution,
        Capability::ImageInput,
    ];

    let check = engine.check_missing(&caps);
    let mut conv = user_only_conv();
    let apply = engine.apply(&caps, &mut conv);

    assert_eq!(check.applied.len(), apply.applied.len());
    assert_eq!(check.warnings.len(), apply.warnings.len());
    for (a, b) in check.applied.iter().zip(apply.applied.iter()) {
        assert_eq!(a.capability, b.capability);
        assert_eq!(a.strategy, b.strategy);
    }
}

#[test]
fn report_entries_have_correct_capability_strategy_pairs() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
        ],
        &mut conv,
    );

    assert_eq!(report.applied[0].capability, Capability::ExtendedThinking);
    assert!(matches!(
        report.applied[0].strategy,
        EmulationStrategy::SystemPromptInjection { .. }
    ));
    assert_eq!(
        report.applied[1].capability,
        Capability::StructuredOutputJsonSchema
    );
    assert!(matches!(
        report.applied[1].strategy,
        EmulationStrategy::PostProcessing { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 12. Emulation doesn't modify passthrough mode (disabled caps)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn disabled_cap_no_conversation_mutation() {
    let original = multi_turn_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::CodeExecution], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn all_disabled_caps_no_mutation() {
    let original = multi_turn_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&disabled_caps(), &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn post_processing_does_not_mutate() {
    let original = user_only_conv();
    let mut conv = original.clone();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv, original);
}

#[test]
fn post_processing_and_disabled_combined_no_mutation() {
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
// 13. Fidelity labels (emulation labels in receipts)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn compute_fidelity_native_only() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);
    assert_eq!(labels.len(), 2);
    assert!(labels.values().all(|l| *l == FidelityLabel::Native));
}

#[test]
fn compute_fidelity_emulated_only() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    let labels = compute_fidelity(&[], &report);
    assert_eq!(labels.len(), 2);
    for label in labels.values() {
        assert!(matches!(label, FidelityLabel::Emulated { .. }));
    }
}

#[test]
fn compute_fidelity_mixed() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let labels = compute_fidelity(&[Capability::Streaming, Capability::ToolUse], &report);

    assert_eq!(labels.len(), 3);
    assert_eq!(labels[&Capability::Streaming], FidelityLabel::Native);
    assert_eq!(labels[&Capability::ToolUse], FidelityLabel::Native);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn compute_fidelity_warnings_excluded() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::CodeExecution],
        &mut conv,
    );

    let labels = compute_fidelity(&[], &report);
    // CodeExecution generated a warning, not an applied entry
    assert_eq!(labels.len(), 1);
    assert!(labels.contains_key(&Capability::ExtendedThinking));
    assert!(!labels.contains_key(&Capability::CodeExecution));
}

#[test]
fn compute_fidelity_emulated_overrides_native_for_same_cap() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

    // If the same cap appears in both native and emulated, emulated wins
    // (applied after native in compute_fidelity)
    let labels = compute_fidelity(&[Capability::ExtendedThinking], &report);
    assert_eq!(labels.len(), 1);
    assert!(matches!(
        labels[&Capability::ExtendedThinking],
        FidelityLabel::Emulated { .. }
    ));
}

#[test]
fn fidelity_label_carries_strategy_detail() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&[Capability::StopSequences], &mut conv);

    let labels = compute_fidelity(&[], &report);
    if let FidelityLabel::Emulated { strategy } = &labels[&Capability::StopSequences] {
        if let EmulationStrategy::PostProcessing { detail } = strategy {
            assert!(detail.contains("stop sequence"));
        } else {
            panic!("expected PostProcessing strategy");
        }
    } else {
        panic!("expected Emulated label");
    }
}

#[test]
fn fidelity_btreemap_is_deterministic() {
    let mut conv = user_only_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv,
    );

    let labels1 = compute_fidelity(&[Capability::Streaming], &report);
    let json1 = serde_json::to_string(&labels1).unwrap();

    let mut conv2 = user_only_conv();
    let report2 = engine.apply(
        &[Capability::ExtendedThinking, Capability::ImageInput],
        &mut conv2,
    );
    let labels2 = compute_fidelity(&[Capability::Streaming], &report2);
    let json2 = serde_json::to_string(&labels2).unwrap();

    assert_eq!(json1, json2);
}

// ════════════════════════════════════════════════════════════════════════
// 14. Performance with many capabilities
// ════════════════════════════════════════════════════════════════════════

#[test]
fn apply_all_known_capabilities() {
    let mut all_caps = emulatable_caps();
    all_caps.extend(disabled_caps());

    let mut conv = simple_conv();
    let engine = EmulationEngine::with_defaults();
    let report = engine.apply(&all_caps, &mut conv);

    let total = report.applied.len() + report.warnings.len();
    assert_eq!(total, all_caps.len());
}

#[test]
fn check_missing_all_known_capabilities() {
    let mut all_caps = emulatable_caps();
    all_caps.extend(disabled_caps());

    let engine = EmulationEngine::with_defaults();
    let report = engine.check_missing(&all_caps);

    let total = report.applied.len() + report.warnings.len();
    assert_eq!(total, all_caps.len());
}

#[test]
fn repeated_same_capability() {
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

    // Each occurrence is applied independently
    assert_eq!(report.applied.len(), 3);
    let sys = conv.system_message().unwrap();
    // Original block + 3 injected blocks
    assert_eq!(sys.content.len(), 4);
}

#[test]
fn large_number_of_overrides() {
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

    // All previously disabled caps should now be applied
    assert_eq!(report.applied.len(), disabled_caps().len());
    assert!(report.warnings.is_empty());
}

// ════════════════════════════════════════════════════════════════════════
// 15. Thread safety of emulation engine
// ════════════════════════════════════════════════════════════════════════

#[test]
fn engine_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<EmulationEngine>();
}

#[test]
fn engine_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<EmulationEngine>();
}

#[test]
fn config_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EmulationConfig>();
}

#[test]
fn report_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EmulationReport>();
}

#[test]
fn strategy_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EmulationStrategy>();
}

#[test]
fn fidelity_label_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FidelityLabel>();
}

#[test]
fn engine_clone_independence() {
    let engine1 = EmulationEngine::with_defaults();
    let engine2 = engine1.clone();

    let mut conv1 = user_only_conv();
    let mut conv2 = user_only_conv();

    let report1 = engine1.apply(&[Capability::ExtendedThinking], &mut conv1);
    let report2 = engine2.apply(&[Capability::ExtendedThinking], &mut conv2);

    assert_eq!(report1.applied.len(), report2.applied.len());
    assert_eq!(conv1, conv2);
}

// ════════════════════════════════════════════════════════════════════════
// Additional: free-function, edge cases, conversation shapes
// ════════════════════════════════════════════════════════════════════════

#[test]
fn free_function_apply_emulation_with_overrides() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::CodeExecution,
        EmulationStrategy::SystemPromptInjection {
            prompt: "simulate execution".into(),
        },
    );

    let mut conv = user_only_conv();
    let report = apply_emulation(&config, &[Capability::CodeExecution], &mut conv);

    assert_eq!(report.applied.len(), 1);
    assert!(conv.system_message().is_some());
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
fn apply_preserves_conversation_length_for_post_processing() {
    let conv_len = multi_turn_conv().len();
    let mut conv = multi_turn_conv();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);
    assert_eq!(conv.len(), conv_len);
}

#[test]
fn apply_increases_message_count_when_system_created() {
    let mut conv = user_only_conv();
    assert_eq!(conv.len(), 1);
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.len(), 2);
}

#[test]
fn apply_does_not_increase_message_count_when_system_exists() {
    let mut conv = simple_conv();
    let before_len = conv.len();
    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);
    assert_eq!(conv.len(), before_len);
}

#[test]
fn conversation_with_image_block_preserved() {
    let img_msg = IrMessage::new(
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
    );
    let mut conv = IrConversation::new().push(img_msg);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ImageInput], &mut conv);

    // Image block in user message is preserved
    let user_msg = &conv.messages[1]; // system was inserted at 0
    assert_eq!(user_msg.content.len(), 2);
    assert!(matches!(user_msg.content[1], IrContentBlock::Image { .. }));
}

#[test]
fn conversation_with_thinking_block_preserved() {
    let asst_msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "Let me think...".into(),
            },
            IrContentBlock::Text {
                text: "Here's my answer.".into(),
            },
        ],
    );
    let mut conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Question"))
        .push(asst_msg);

    let engine = EmulationEngine::with_defaults();
    engine.apply(&[Capability::ExtendedThinking], &mut conv);

    let asst = conv.last_assistant().unwrap();
    assert!(matches!(asst.content[0], IrContentBlock::Thinking { .. }));
}

#[test]
fn can_emulate_matches_default_strategy_for_all_emulatable() {
    for cap in emulatable_caps() {
        assert!(can_emulate(&cap), "{cap:?} should be emulatable");
        assert!(
            !matches!(default_strategy(&cap), EmulationStrategy::Disabled { .. }),
            "{cap:?} default should not be Disabled"
        );
    }
}

#[test]
fn can_emulate_matches_default_strategy_for_all_disabled() {
    for cap in disabled_caps() {
        assert!(!can_emulate(&cap), "{cap:?} should not be emulatable");
        assert!(
            matches!(default_strategy(&cap), EmulationStrategy::Disabled { .. }),
            "{cap:?} default should be Disabled"
        );
    }
}

#[test]
fn named_strategy_functions_produce_distinct_outputs() {
    let strategies: Vec<EmulationStrategy> = vec![
        emulate_structured_output(),
        emulate_code_execution(),
        emulate_extended_thinking(),
        emulate_image_input(),
        emulate_stop_sequences(),
    ];

    for i in 0..strategies.len() {
        for j in (i + 1)..strategies.len() {
            assert_ne!(
                strategies[i], strategies[j],
                "Strategies at index {i} and {j} should differ"
            );
        }
    }
}

#[test]
fn fidelity_compute_empty_report_empty_native() {
    let report = EmulationReport::default();
    let labels = compute_fidelity(&[], &report);
    assert!(labels.is_empty());
}

#[test]
fn config_strategies_btreemap_deterministic_order() {
    let mut config = EmulationConfig::new();
    config.set(
        Capability::ToolUse,
        EmulationStrategy::Disabled { reason: "a".into() },
    );
    config.set(
        Capability::Streaming,
        EmulationStrategy::Disabled { reason: "b".into() },
    );

    let json1 = serde_json::to_string(&config).unwrap();
    let json2 = serde_json::to_string(&config).unwrap();
    assert_eq!(json1, json2);
}
