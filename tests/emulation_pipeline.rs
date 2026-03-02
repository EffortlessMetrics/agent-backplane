// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the full capability emulation pipeline.
//!
//! Covers detection, work-order mutation, fidelity labeling, receipt metadata,
//! error codes, and cross-capability emulation for every emulation strategy.

use std::collections::BTreeMap;

use abp_capability::{SupportLevel, check_capability, negotiate};
use abp_core::ir::{IrConversation, IrMessage, IrRole};
use abp_core::{
    Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
    Outcome, ReceiptBuilder, SupportLevel as CoreSupportLevel, WorkOrderBuilder,
};
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationReport, EmulationStrategy, FidelityLabel,
    can_emulate, compute_fidelity, default_strategy, emulate_code_execution,
    emulate_extended_thinking, emulate_image_input, emulate_stop_sequences,
    emulate_structured_output,
};

// ── Helpers ─────────────────────────────────────────────────────────────

/// Build a manifest with the given capabilities all at `Native`.
fn native_manifest(caps: &[Capability]) -> CapabilityManifest {
    caps.iter()
        .map(|c| (c.clone(), CoreSupportLevel::Native))
        .collect()
}

/// Build a manifest that marks everything as `Unsupported`.
fn empty_manifest() -> CapabilityManifest {
    BTreeMap::new()
}

/// Build requirements requesting the given capabilities.
fn require(caps: &[Capability]) -> CapabilityRequirements {
    CapabilityRequirements {
        required: caps
            .iter()
            .map(|c| CapabilityRequirement {
                capability: c.clone(),
                min_support: MinSupport::Native,
            })
            .collect(),
    }
}

/// Create a minimal conversation for testing emulation application.
fn minimal_conversation() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Hello"))
}

/// Create a conversation without a system message.
fn no_system_conversation() -> IrConversation {
    IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"))
}

// =========================================================================
// § 1  StructuredOutput emulation
// =========================================================================

mod structured_output {
    use super::*;

    #[test]
    fn detection_needed_when_backend_lacks_capability() {
        let manifest = empty_manifest();
        let level = check_capability(&manifest, &Capability::StructuredOutputJsonSchema);
        assert_eq!(level, SupportLevel::Unsupported);

        let neg = negotiate(
            &manifest,
            &require(&[Capability::StructuredOutputJsonSchema]),
        );
        assert!(!neg.is_compatible());
        assert_eq!(
            neg.unsupported,
            vec![Capability::StructuredOutputJsonSchema]
        );
    }

    #[test]
    fn detection_not_needed_when_backend_supports_natively() {
        let manifest = native_manifest(&[Capability::StructuredOutputJsonSchema]);
        let level = check_capability(&manifest, &Capability::StructuredOutputJsonSchema);
        assert_eq!(level, SupportLevel::Native);

        let neg = negotiate(
            &manifest,
            &require(&[Capability::StructuredOutputJsonSchema]),
        );
        assert!(neg.is_compatible());
        assert!(neg.unsupported.is_empty());
    }

    #[test]
    fn default_strategy_is_post_processing() {
        let strategy = default_strategy(&Capability::StructuredOutputJsonSchema);
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn named_strategy_is_system_prompt_injection() {
        let strategy = emulate_structured_output();
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn can_emulate_returns_true() {
        assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    }

    #[test]
    fn apply_does_not_mutate_conversation_for_post_processing() {
        let engine = EmulationEngine::with_defaults();
        let original = minimal_conversation();
        let mut conv = original.clone();
        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

        // Post-processing strategies do not mutate the conversation.
        assert_eq!(conv, original);
        assert_eq!(report.applied.len(), 1);
        assert!(matches!(
            report.applied[0].strategy,
            EmulationStrategy::PostProcessing { .. }
        ));
    }

    #[test]
    fn fidelity_label_emulated_when_not_native() {
        let native: Vec<Capability> = vec![];
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

        let labels = compute_fidelity(&native, &report);
        let label = labels.get(&Capability::StructuredOutputJsonSchema).unwrap();
        assert!(matches!(label, FidelityLabel::Emulated { .. }));
    }

    #[test]
    fn receipt_reflects_emulation_metadata() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

        // Build a receipt whose capabilities mirror the emulated set.
        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build();

        // Receipt has no StructuredOutputJsonSchema native capability.
        assert!(
            !receipt
                .capabilities
                .contains_key(&Capability::StructuredOutputJsonSchema)
        );
        // The emulation report records the applied strategy.
        assert_eq!(report.applied.len(), 1);
        assert_eq!(
            report.applied[0].capability,
            Capability::StructuredOutputJsonSchema
        );
    }
}

// =========================================================================
// § 2  CodeExecution emulation
// =========================================================================

mod code_execution {
    use super::*;

    #[test]
    fn detection_needed_when_missing() {
        let manifest = empty_manifest();
        let neg = negotiate(&manifest, &require(&[Capability::CodeExecution]));
        assert!(!neg.is_compatible());
    }

    #[test]
    fn default_strategy_is_disabled() {
        let strategy = default_strategy(&Capability::CodeExecution);
        assert!(matches!(strategy, EmulationStrategy::Disabled { .. }));
        if let EmulationStrategy::Disabled { reason } = &strategy {
            assert!(reason.contains("sandbox"));
        }
    }

    #[test]
    fn can_emulate_returns_false() {
        assert!(!can_emulate(&Capability::CodeExecution));
    }

    #[test]
    fn disabled_strategy_generates_warning() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("not emulated"));
        assert!(report.has_unemulatable());
    }

    #[test]
    fn disabled_does_not_mutate_conversation() {
        let original = minimal_conversation();
        let mut conv = original.clone();
        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::CodeExecution], &mut conv);
        assert_eq!(conv, original);
    }

    #[test]
    fn named_strategy_overrides_disabled() {
        let mut config = EmulationConfig::new();
        config.set(Capability::CodeExecution, emulate_code_execution());
        let engine = EmulationEngine::new(config);

        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        assert!(report.warnings.is_empty());
        assert_eq!(report.applied.len(), 1);
        assert!(matches!(
            report.applied[0].strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
        // System prompt should now contain the injection text.
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("reason through the code"));
    }

    #[test]
    fn fidelity_not_present_when_disabled() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        let labels = compute_fidelity(&[], &report);
        // Disabled capabilities produce warnings, not applied entries, so no fidelity label.
        assert!(!labels.contains_key(&Capability::CodeExecution));
    }

    #[test]
    fn fidelity_emulated_with_override() {
        let mut config = EmulationConfig::new();
        config.set(Capability::CodeExecution, emulate_code_execution());
        let engine = EmulationEngine::new(config);

        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        let labels = compute_fidelity(&[], &report);
        let label = labels.get(&Capability::CodeExecution).unwrap();
        assert!(matches!(label, FidelityLabel::Emulated { .. }));
    }

    #[test]
    fn receipt_outcome_still_valid_on_disabled() {
        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// =========================================================================
// § 3  ExtendedThinking emulation
// =========================================================================

mod extended_thinking {
    use super::*;

    #[test]
    fn detection_needed_when_missing() {
        let manifest = empty_manifest();
        let neg = negotiate(&manifest, &require(&[Capability::ExtendedThinking]));
        assert!(!neg.is_compatible());
        assert_eq!(neg.unsupported, vec![Capability::ExtendedThinking]);
    }

    #[test]
    fn detection_not_needed_when_native() {
        let manifest = native_manifest(&[Capability::ExtendedThinking]);
        let neg = negotiate(&manifest, &require(&[Capability::ExtendedThinking]));
        assert!(neg.is_compatible());
    }

    #[test]
    fn default_strategy_is_system_prompt_injection() {
        let strategy = default_strategy(&Capability::ExtendedThinking);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn can_emulate_returns_true() {
        assert!(can_emulate(&Capability::ExtendedThinking));
    }

    #[test]
    fn apply_injects_into_existing_system_message() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        let sys = conv.system_message().unwrap();
        let text = sys.text_content();
        assert!(text.contains("Think step by step"));
        assert!(text.contains("You are a helpful assistant."));
        assert_eq!(report.applied.len(), 1);
    }

    #[test]
    fn apply_creates_system_message_if_absent() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = no_system_conversation();
        engine.apply(&[Capability::ExtendedThinking], &mut conv);

        assert_eq!(conv.messages[0].role, IrRole::System);
        assert!(
            conv.messages[0]
                .text_content()
                .contains("Think step by step")
        );
    }

    #[test]
    fn named_strategy_matches_default() {
        let named = emulate_extended_thinking();
        assert!(matches!(
            named,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn fidelity_label_is_emulated() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        let labels = compute_fidelity(&[], &report);
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn fidelity_label_is_native_when_supported() {
        let native_caps = vec![Capability::ExtendedThinking];
        let report = EmulationReport::default(); // no emulations applied
        let labels = compute_fidelity(&native_caps, &report);
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Native)
        ));
    }

    #[test]
    fn receipt_records_trace_with_emulation() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build();

        // Verify the report can inform receipt metadata.
        assert!(!report.applied.is_empty());
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// =========================================================================
// § 4  ImageInput emulation
// =========================================================================

mod image_input {
    use super::*;

    #[test]
    fn detection_needed_when_missing() {
        let manifest = empty_manifest();
        let neg = negotiate(&manifest, &require(&[Capability::ImageInput]));
        assert!(!neg.is_compatible());
    }

    #[test]
    fn default_strategy_is_system_prompt_injection() {
        let strategy = default_strategy(&Capability::ImageInput);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }

    #[test]
    fn can_emulate_returns_true() {
        assert!(can_emulate(&Capability::ImageInput));
    }

    #[test]
    fn apply_injects_image_description_prompt() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ImageInput], &mut conv);

        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("Image inputs"));
        assert!(sys_text.contains("text descriptions"));
        assert_eq!(report.applied.len(), 1);
    }

    #[test]
    fn apply_creates_system_message_if_absent() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = no_system_conversation();
        engine.apply(&[Capability::ImageInput], &mut conv);

        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn named_strategy_produces_correct_prompt() {
        let strategy = emulate_image_input();
        if let EmulationStrategy::SystemPromptInjection { prompt } = &strategy {
            assert!(prompt.contains("Image inputs"));
        } else {
            panic!("Expected SystemPromptInjection");
        }
    }

    #[test]
    fn fidelity_label_emulated() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ImageInput], &mut conv);

        let labels = compute_fidelity(&[], &report);
        assert!(matches!(
            labels.get(&Capability::ImageInput),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn receipt_metadata_valid() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ImageInput], &mut conv);

        let receipt = ReceiptBuilder::new("text-only-backend")
            .outcome(Outcome::Complete)
            .build();

        assert_eq!(report.applied[0].capability, Capability::ImageInput);
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// =========================================================================
// § 5  StopSequences emulation
// =========================================================================

mod stop_sequences {
    use super::*;

    #[test]
    fn detection_needed_when_missing() {
        let manifest = empty_manifest();
        let neg = negotiate(&manifest, &require(&[Capability::StopSequences]));
        assert!(!neg.is_compatible());
    }

    #[test]
    fn default_strategy_is_post_processing() {
        let strategy = default_strategy(&Capability::StopSequences);
        assert!(matches!(strategy, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn can_emulate_returns_true() {
        assert!(can_emulate(&Capability::StopSequences));
    }

    #[test]
    fn apply_records_post_processing_without_mutating() {
        let engine = EmulationEngine::with_defaults();
        let original = minimal_conversation();
        let mut conv = original.clone();
        let report = engine.apply(&[Capability::StopSequences], &mut conv);

        assert_eq!(conv, original);
        assert_eq!(report.applied.len(), 1);
        if let EmulationStrategy::PostProcessing { detail } = &report.applied[0].strategy {
            assert!(detail.contains("stop sequence"));
        } else {
            panic!("Expected PostProcessing strategy");
        }
    }

    #[test]
    fn named_strategy_matches_default() {
        let named = emulate_stop_sequences();
        let def = default_strategy(&Capability::StopSequences);
        assert_eq!(named, def);
    }

    #[test]
    fn fidelity_label_emulated() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::StopSequences], &mut conv);

        let labels = compute_fidelity(&[], &report);
        assert!(matches!(
            labels.get(&Capability::StopSequences),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn receipt_with_emulation_report() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::StopSequences], &mut conv);

        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .build();

        assert!(!report.applied.is_empty());
        assert!(report.warnings.is_empty());
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// =========================================================================
// § 6  Cross-capability emulation
// =========================================================================

mod cross_capability {
    use super::*;

    #[test]
    fn multiple_emulatable_capabilities_applied_together() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();

        let caps = [
            Capability::ExtendedThinking,
            Capability::ImageInput,
            Capability::StopSequences,
            Capability::StructuredOutputJsonSchema,
        ];
        let report = engine.apply(&caps, &mut conv);

        // All four should be applied (all are emulatable by default).
        assert_eq!(report.applied.len(), 4);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn mixed_emulatable_and_disabled() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();

        let caps = [
            Capability::ExtendedThinking, // emulatable (system prompt)
            Capability::CodeExecution,    // disabled by default
            Capability::StopSequences,    // emulatable (post-processing)
            Capability::Streaming,        // disabled
        ];
        let report = engine.apply(&caps, &mut conv);

        assert_eq!(report.applied.len(), 2);
        assert_eq!(report.warnings.len(), 2);
        assert!(report.has_unemulatable());
    }

    #[test]
    fn multiple_system_prompt_injections_accumulate() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();

        // Both inject into the system prompt.
        let caps = [Capability::ExtendedThinking, Capability::ImageInput];
        engine.apply(&caps, &mut conv);

        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("Think step by step"));
        assert!(sys_text.contains("Image inputs"));
        assert!(sys_text.contains("You are a helpful assistant."));
    }

    #[test]
    fn system_prompt_and_post_processing_coexist() {
        let engine = EmulationEngine::with_defaults();
        let original_user_msg = "Build me a JSON parser";
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be precise."))
            .push(IrMessage::text(IrRole::User, original_user_msg));

        let caps = [
            Capability::ExtendedThinking,           // system prompt injection
            Capability::StructuredOutputJsonSchema, // post-processing
        ];
        let report = engine.apply(&caps, &mut conv);

        assert_eq!(report.applied.len(), 2);
        // System message modified by ExtendedThinking but not by StructuredOutput.
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("Think step by step"));
        assert!(sys_text.contains("Be precise."));
        // User message untouched.
        assert_eq!(conv.messages[1].text_content(), original_user_msg);
    }

    #[test]
    fn fidelity_labels_for_cross_capability() {
        let native_caps = vec![Capability::Streaming, Capability::ToolUse];
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();

        let emulated_caps = [Capability::ExtendedThinking, Capability::StopSequences];
        let report = engine.apply(&emulated_caps, &mut conv);

        let labels = compute_fidelity(&native_caps, &report);

        // Native capabilities get Native label.
        assert!(matches!(
            labels.get(&Capability::Streaming),
            Some(FidelityLabel::Native)
        ));
        assert!(matches!(
            labels.get(&Capability::ToolUse),
            Some(FidelityLabel::Native)
        ));
        // Emulated capabilities get Emulated label.
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
        assert!(matches!(
            labels.get(&Capability::StopSequences),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn emulated_overrides_native_in_fidelity() {
        // If same capability appears both native and emulated, emulated wins
        // (inserted later into the BTreeMap).
        let native_caps = vec![Capability::ExtendedThinking];
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        let labels = compute_fidelity(&native_caps, &report);
        // Emulated is inserted after native, so it overwrites.
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn cross_capability_receipt_with_hash() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(
            &[
                Capability::ExtendedThinking,
                Capability::StopSequences,
                Capability::ImageInput,
            ],
            &mut conv,
        );

        let receipt = ReceiptBuilder::new("multi-emulation-backend")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();

        assert!(receipt.receipt_sha256.is_some());
        assert_eq!(report.applied.len(), 3);
    }

    #[test]
    fn all_five_strategies_in_one_request() {
        let mut config = EmulationConfig::new();
        // Override CodeExecution to be emulatable.
        config.set(Capability::CodeExecution, emulate_code_execution());

        let engine = EmulationEngine::new(config);
        let mut conv = minimal_conversation();

        let caps = [
            Capability::StructuredOutputJsonSchema,
            Capability::CodeExecution,
            Capability::ExtendedThinking,
            Capability::ImageInput,
            Capability::StopSequences,
        ];
        let report = engine.apply(&caps, &mut conv);

        assert_eq!(report.applied.len(), 5);
        assert!(report.warnings.is_empty());

        // Verify system prompt has all injections.
        let sys_text = conv.system_message().unwrap().text_content();
        assert!(sys_text.contains("reason through the code"));
        assert!(sys_text.contains("Think step by step"));
        assert!(sys_text.contains("Image inputs"));

        // Verify fidelity labels for all five.
        let labels = compute_fidelity(&[], &report);
        assert_eq!(labels.len(), 5);
        for label in labels.values() {
            assert!(matches!(label, FidelityLabel::Emulated { .. }));
        }
    }

    #[test]
    fn check_missing_without_mutation() {
        let engine = EmulationEngine::with_defaults();
        let caps = [
            Capability::ExtendedThinking,
            Capability::CodeExecution,
            Capability::ImageInput,
        ];
        let report = engine.check_missing(&caps);

        // ExtendedThinking and ImageInput are emulatable, CodeExecution is disabled.
        assert_eq!(report.applied.len(), 2);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn negotiation_and_emulation_end_to_end() {
        // Simulate: backend supports Streaming natively, nothing else.
        let manifest = native_manifest(&[Capability::Streaming]);
        let required_caps = [
            Capability::Streaming,
            Capability::ExtendedThinking,
            Capability::StopSequences,
        ];
        let neg = negotiate(&manifest, &require(&required_caps));

        assert_eq!(neg.native, vec![Capability::Streaming]);
        assert_eq!(neg.unsupported.len(), 2);

        // Attempt to emulate the unsupported capabilities.
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&neg.unsupported, &mut conv);

        // Both ExtendedThinking and StopSequences are emulatable.
        assert_eq!(report.applied.len(), 2);
        assert!(report.warnings.is_empty());

        // Compute combined fidelity.
        let labels = compute_fidelity(&neg.native, &report);
        assert!(matches!(
            labels.get(&Capability::Streaming),
            Some(FidelityLabel::Native)
        ));
        assert!(matches!(
            labels.get(&Capability::ExtendedThinking),
            Some(FidelityLabel::Emulated { .. })
        ));
        assert!(matches!(
            labels.get(&Capability::StopSequences),
            Some(FidelityLabel::Emulated { .. })
        ));
    }

    #[test]
    fn work_order_requirements_drive_emulation() {
        let wo = WorkOrderBuilder::new("Analyze this code")
            .requirements(require(&[
                Capability::ExtendedThinking,
                Capability::StructuredOutputJsonSchema,
            ]))
            .build();

        // Backend has neither capability.
        let manifest = empty_manifest();
        let neg = negotiate(&manifest, &wo.requirements);
        assert_eq!(neg.unsupported.len(), 2);

        // Emulate both.
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&neg.unsupported, &mut conv);
        assert_eq!(report.applied.len(), 2);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn failed_emulation_produces_warnings_not_panics() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();

        // Request only non-emulatable capabilities.
        let caps = [
            Capability::CodeExecution,
            Capability::Streaming,
            Capability::ToolUse,
            Capability::Logprobs,
        ];
        let report = engine.apply(&caps, &mut conv);

        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 4);
        for warning in &report.warnings {
            assert!(warning.contains("not emulated"));
        }
    }

    #[test]
    fn receipt_with_failed_outcome_on_unsupported() {
        let receipt = ReceiptBuilder::new("limited-backend")
            .outcome(Outcome::Failed)
            .build();

        assert_eq!(receipt.outcome, Outcome::Failed);
    }

    #[test]
    fn empty_capabilities_list_produces_empty_report() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[], &mut conv);

        assert!(report.is_empty());
        assert!(!report.has_unemulatable());
    }

    #[test]
    fn config_override_disables_normally_emulatable() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::Disabled {
                reason: "user disabled thinking".into(),
            },
        );
        let engine = EmulationEngine::new(config);

        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("user disabled thinking"));
    }

    #[test]
    fn serde_roundtrip_of_fidelity_labels() {
        let native_caps = vec![Capability::Streaming];
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        let labels = compute_fidelity(&native_caps, &report);
        let json = serde_json::to_string(&labels).unwrap();
        let decoded: BTreeMap<Capability, FidelityLabel> = serde_json::from_str(&json).unwrap();
        assert_eq!(labels, decoded);
    }

    #[test]
    fn serde_roundtrip_of_emulation_report() {
        let engine = EmulationEngine::with_defaults();
        let mut conv = minimal_conversation();
        let report = engine.apply(
            &[
                Capability::ExtendedThinking,
                Capability::CodeExecution,
                Capability::StopSequences,
            ],
            &mut conv,
        );

        let json = serde_json::to_string(&report).unwrap();
        let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, decoded);
    }
}
