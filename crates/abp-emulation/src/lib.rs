// SPDX-License-Identifier: MIT OR Apache-2.0
//! Labeled capability emulation for Agent Backplane.
//!
//! When a backend does not natively support a capability (e.g. `extended_thinking`),
//! ABP can emulate it — but **only** with explicit labeling so the caller knows
//! it is emulated, never silently degraded.
#![deny(unsafe_code)]
#![warn(missing_docs)]

use abp_core::Capability;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Strategy ────────────────────────────────────────────────────────────

/// How a missing capability should be emulated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EmulationStrategy {
    /// Inject text into the system prompt to approximate the capability.
    SystemPromptInjection {
        /// The text to inject.
        prompt: String,
    },
    /// Apply post-processing on the assistant response.
    PostProcessing {
        /// Human-readable description of the post-processing step.
        detail: String,
    },
    /// Cannot be safely emulated — must be explicitly disabled.
    Disabled {
        /// Reason the capability cannot be emulated.
        reason: String,
    },
}

// ── Named Strategies ────────────────────────────────────────────────────

/// Pre-configured strategy: emulate JSON schema output via prompt engineering.
///
/// Instructs the model to produce valid JSON matching a requested schema.
#[must_use]
pub fn emulate_structured_output() -> EmulationStrategy {
    EmulationStrategy::SystemPromptInjection {
        prompt: "You MUST respond with valid JSON matching the requested schema. \
                 Do not include any text outside the JSON object."
            .into(),
    }
}

/// Pre-configured strategy: emulate code execution by reasoning through code.
///
/// The model simulates execution rather than running code in a sandbox.
#[must_use]
pub fn emulate_code_execution() -> EmulationStrategy {
    EmulationStrategy::SystemPromptInjection {
        prompt: "When asked to execute code, reason through the code step by step \
                 and produce the expected output. Do not actually execute code."
            .into(),
    }
}

/// Pre-configured strategy: emulate extended thinking via chain-of-thought prompting.
#[must_use]
pub fn emulate_extended_thinking() -> EmulationStrategy {
    EmulationStrategy::SystemPromptInjection {
        prompt: "Think step by step before answering. Show your reasoning process.".into(),
    }
}

/// Pre-configured strategy: handle image inputs on text-only backends.
///
/// Converts image inputs to text descriptions as a fallback.
#[must_use]
pub fn emulate_image_input() -> EmulationStrategy {
    EmulationStrategy::SystemPromptInjection {
        prompt: "Image inputs have been converted to text descriptions. \
                 Process the descriptions as if viewing the original images."
            .into(),
    }
}

/// Pre-configured strategy: emulate stop sequences via post-processing truncation.
#[must_use]
pub fn emulate_stop_sequences() -> EmulationStrategy {
    EmulationStrategy::PostProcessing {
        detail: "Truncate response at the first occurrence of any specified stop sequence".into(),
    }
}

// ── Fidelity ────────────────────────────────────────────────────────────

/// How a capability is fulfilled — natively by the backend or via emulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fidelity", rename_all = "snake_case")]
pub enum FidelityLabel {
    /// Backend supports this capability natively — no transformation needed.
    Native,
    /// Capability is provided through an emulation strategy.
    Emulated {
        /// The strategy used for emulation.
        strategy: EmulationStrategy,
    },
}

/// Compute per-capability fidelity labels from native support and emulation results.
///
/// Capabilities in `native` get [`FidelityLabel::Native`]; capabilities in
/// `report.applied` get [`FidelityLabel::Emulated`]. Capabilities that appear
/// only in `report.warnings` are omitted (they could not be provided at all).
#[must_use]
pub fn compute_fidelity(
    native: &[Capability],
    report: &EmulationReport,
) -> BTreeMap<Capability, FidelityLabel> {
    let mut labels = BTreeMap::new();
    for cap in native {
        labels.insert(cap.clone(), FidelityLabel::Native);
    }
    for entry in &report.applied {
        labels.insert(
            entry.capability.clone(),
            FidelityLabel::Emulated {
                strategy: entry.strategy.clone(),
            },
        );
    }
    labels
}

// ── Config ──────────────────────────────────────────────────────────────

/// Per-capability emulation overrides.
///
/// Uses [`BTreeMap`] for deterministic serialization.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmulationConfig {
    /// Explicit strategy overrides keyed by capability.
    pub strategies: BTreeMap<Capability, EmulationStrategy>,
}

impl EmulationConfig {
    /// Create an empty config (all capabilities use defaults).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a strategy override for a specific capability.
    pub fn set(&mut self, capability: Capability, strategy: EmulationStrategy) {
        self.strategies.insert(capability, strategy);
    }
}

// ── Defaults ────────────────────────────────────────────────────────────

/// Return the default emulation strategy for a capability.
///
/// - `ExtendedThinking` → system-prompt injection
/// - `StructuredOutputJsonSchema` → post-processing
/// - `CodeExecution` → disabled
/// - Everything else → disabled with a generic reason
#[must_use]
pub fn default_strategy(capability: &Capability) -> EmulationStrategy {
    match capability {
        Capability::ExtendedThinking => EmulationStrategy::SystemPromptInjection {
            prompt: "Think step by step before answering.".into(),
        },
        Capability::StructuredOutputJsonSchema => EmulationStrategy::PostProcessing {
            detail: "Parse and validate JSON from text response".into(),
        },
        Capability::CodeExecution => EmulationStrategy::Disabled {
            reason: "Cannot safely emulate sandboxed code execution".into(),
        },
        Capability::ImageInput => emulate_image_input(),
        Capability::StopSequences => emulate_stop_sequences(),
        other => EmulationStrategy::Disabled {
            reason: format!("No emulation available for {other:?}"),
        },
    }
}

/// Returns `true` if the capability has a non-[`Disabled`](EmulationStrategy::Disabled)
/// default strategy.
#[must_use]
pub fn can_emulate(capability: &Capability) -> bool {
    !matches!(
        default_strategy(capability),
        EmulationStrategy::Disabled { .. }
    )
}

// ── Report ──────────────────────────────────────────────────────────────

/// Record of a single emulation that was applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmulationEntry {
    /// The capability that was emulated.
    pub capability: Capability,
    /// The strategy that was used.
    pub strategy: EmulationStrategy,
}

/// Summary of all emulations applied during a single pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmulationReport {
    /// Emulations that were applied.
    pub applied: Vec<EmulationEntry>,
    /// Human-readable warnings (e.g. disabled capabilities that were requested).
    pub warnings: Vec<String>,
}

impl EmulationReport {
    /// Returns `true` if no emulations were applied and no warnings were generated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.applied.is_empty() && self.warnings.is_empty()
    }

    /// Returns `true` if any requested capability could not be emulated (has warnings).
    #[must_use]
    pub fn has_unemulatable(&self) -> bool {
        !self.warnings.is_empty()
    }
}

// ── Engine ──────────────────────────────────────────────────────────────

/// Applies emulation strategies to an [`IrConversation`].
///
/// The engine never silently degrades — every emulation is recorded in the
/// returned [`EmulationReport`], and disabled capabilities produce warnings.
#[derive(Debug, Clone)]
pub struct EmulationEngine {
    config: EmulationConfig,
}

impl EmulationEngine {
    /// Create an engine with the given config.
    #[must_use]
    pub fn new(config: EmulationConfig) -> Self {
        Self { config }
    }

    /// Create an engine with default strategies (no overrides).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(EmulationConfig::new())
    }

    /// Resolve the strategy for a capability, preferring config overrides.
    #[must_use]
    pub fn resolve_strategy(&self, capability: &Capability) -> EmulationStrategy {
        self.config
            .strategies
            .get(capability)
            .cloned()
            .unwrap_or_else(|| default_strategy(capability))
    }

    /// Check which capabilities can be emulated without mutating a conversation.
    ///
    /// Returns a report describing which capabilities would be emulated and
    /// which cannot. Use [`EmulationReport::has_unemulatable`] to determine if
    /// any requested capabilities are unavailable.
    pub fn check_missing(&self, capabilities: &[Capability]) -> EmulationReport {
        let mut report = EmulationReport::default();

        for cap in capabilities {
            let strategy = self.resolve_strategy(cap);
            match &strategy {
                EmulationStrategy::Disabled { reason } => {
                    report
                        .warnings
                        .push(format!("Capability {cap:?} not emulated: {reason}"));
                }
                _ => {
                    report.applied.push(EmulationEntry {
                        capability: cap.clone(),
                        strategy,
                    });
                }
            }
        }
        report
    }

    /// Apply emulations for the given capabilities to a conversation.
    ///
    /// Returns a report describing every action taken.
    pub fn apply(&self, capabilities: &[Capability], conv: &mut IrConversation) -> EmulationReport {
        let mut report = EmulationReport::default();

        for cap in capabilities {
            let strategy = self.resolve_strategy(cap);
            match &strategy {
                EmulationStrategy::SystemPromptInjection { prompt } => {
                    inject_system_prompt(conv, prompt);
                    report.applied.push(EmulationEntry {
                        capability: cap.clone(),
                        strategy,
                    });
                }
                EmulationStrategy::PostProcessing { .. } => {
                    // Post-processing is recorded but does not mutate the
                    // conversation — it is applied after the response.
                    report.applied.push(EmulationEntry {
                        capability: cap.clone(),
                        strategy,
                    });
                }
                EmulationStrategy::Disabled { reason } => {
                    report
                        .warnings
                        .push(format!("Capability {cap:?} not emulated: {reason}"));
                }
            }
        }
        report
    }
}

/// Free-function shortcut: apply emulations with a given config.
pub fn apply_emulation(
    config: &EmulationConfig,
    capabilities: &[Capability],
    conv: &mut IrConversation,
) -> EmulationReport {
    EmulationEngine::new(config.clone()).apply(capabilities, conv)
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Inject `text` into the first system message, or prepend a new one.
fn inject_system_prompt(conv: &mut IrConversation, text: &str) {
    if let Some(sys) = conv.messages.iter_mut().find(|m| m.role == IrRole::System) {
        sys.content.push(IrContentBlock::Text {
            text: format!("\n{text}"),
        });
    } else {
        conv.messages
            .insert(0, IrMessage::text(IrRole::System, text));
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -- default strategy tests ------------------------------------------

    #[test]
    fn extended_thinking_default_is_system_prompt_injection() {
        let s = default_strategy(&Capability::ExtendedThinking);
        assert!(matches!(s, EmulationStrategy::SystemPromptInjection { .. }));
    }

    #[test]
    fn structured_output_default_is_post_processing() {
        let s = default_strategy(&Capability::StructuredOutputJsonSchema);
        assert!(matches!(s, EmulationStrategy::PostProcessing { .. }));
    }

    #[test]
    fn code_execution_default_is_disabled() {
        let s = default_strategy(&Capability::CodeExecution);
        assert!(matches!(s, EmulationStrategy::Disabled { .. }));
    }

    #[test]
    fn streaming_default_is_disabled() {
        let s = default_strategy(&Capability::Streaming);
        assert!(matches!(s, EmulationStrategy::Disabled { .. }));
    }

    #[test]
    fn tool_use_default_is_disabled() {
        let s = default_strategy(&Capability::ToolUse);
        assert!(matches!(s, EmulationStrategy::Disabled { .. }));
    }

    // -- can_emulate -----------------------------------------------------

    #[test]
    fn can_emulate_extended_thinking() {
        assert!(can_emulate(&Capability::ExtendedThinking));
    }

    #[test]
    fn can_emulate_structured_output() {
        assert!(can_emulate(&Capability::StructuredOutputJsonSchema));
    }

    #[test]
    fn cannot_emulate_code_execution() {
        assert!(!can_emulate(&Capability::CodeExecution));
    }

    #[test]
    fn cannot_emulate_streaming() {
        assert!(!can_emulate(&Capability::Streaming));
    }

    #[test]
    fn cannot_emulate_tool_read() {
        assert!(!can_emulate(&Capability::ToolRead));
    }

    // -- system prompt injection -----------------------------------------

    #[test]
    fn system_prompt_injection_adds_to_existing_system_message() {
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "Hello"));

        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::ExtendedThinking], &mut conv);

        let sys = conv.system_message().unwrap();
        let text = sys.text_content();
        assert!(text.contains("Think step by step"));
        assert!(text.contains("You are helpful."));
        assert_eq!(report.applied.len(), 1);
    }

    #[test]
    fn system_prompt_injection_creates_system_message_if_missing() {
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hello"));

        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::ExtendedThinking], &mut conv);

        assert_eq!(conv.messages[0].role, IrRole::System);
        let text = conv.messages[0].text_content();
        assert!(text.contains("Think step by step"));
    }

    // -- post processing -------------------------------------------------

    #[test]
    fn post_processing_recorded_in_report() {
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Give me JSON"));

        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

        assert_eq!(report.applied.len(), 1);
        assert!(matches!(
            report.applied[0].strategy,
            EmulationStrategy::PostProcessing { .. }
        ));
    }

    #[test]
    fn post_processing_does_not_mutate_conversation() {
        let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Give me JSON"));
        let mut conv = original.clone();

        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::StructuredOutputJsonSchema], &mut conv);

        assert_eq!(conv, original);
    }

    // -- disabled strategy -----------------------------------------------

    #[test]
    fn disabled_strategy_generates_warning() {
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Run code"));

        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("not emulated"));
    }

    #[test]
    fn disabled_strategy_does_not_modify_conversation() {
        let original = IrConversation::new().push(IrMessage::text(IrRole::User, "Run code"));
        let mut conv = original.clone();

        let engine = EmulationEngine::with_defaults();
        engine.apply(&[Capability::CodeExecution], &mut conv);

        assert_eq!(conv, original);
    }

    // -- report ----------------------------------------------------------

    #[test]
    fn empty_report_when_no_capabilities() {
        let mut conv = IrConversation::new();
        let engine = EmulationEngine::with_defaults();
        let report = engine.apply(&[], &mut conv);
        assert!(report.is_empty());
    }

    #[test]
    fn report_tracks_multiple_emulations() {
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Hi"))
            .push(IrMessage::text(IrRole::User, "Do things"));

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

    // -- config override -------------------------------------------------

    #[test]
    fn config_override_replaces_default_strategy() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::Disabled {
                reason: "user disabled".into(),
            },
        );

        let engine = EmulationEngine::new(config);
        let strategy = engine.resolve_strategy(&Capability::ExtendedThinking);
        assert!(matches!(strategy, EmulationStrategy::Disabled { .. }));
    }

    #[test]
    fn config_override_applied_during_emulation() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::CodeExecution,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Simulate code execution.".into(),
            },
        );

        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Run it"));

        let engine = EmulationEngine::new(config);
        let report = engine.apply(&[Capability::CodeExecution], &mut conv);

        assert_eq!(report.applied.len(), 1);
        assert!(report.warnings.is_empty());
        assert!(conv.system_message().is_some());
    }

    // -- serde round-trip ------------------------------------------------

    #[test]
    fn serde_roundtrip_emulation_config() {
        let mut config = EmulationConfig::new();
        config.set(
            Capability::ExtendedThinking,
            EmulationStrategy::SystemPromptInjection {
                prompt: "Think step by step.".into(),
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
    fn serde_roundtrip_emulation_report() {
        let report = EmulationReport {
            applied: vec![EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: EmulationStrategy::SystemPromptInjection {
                    prompt: "Think.".into(),
                },
            }],
            warnings: vec!["test warning".into()],
        };

        let json = serde_json::to_string(&report).unwrap();
        let decoded: EmulationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, decoded);
    }

    #[test]
    fn serde_roundtrip_strategy_variants() {
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

    // -- free-function apply_emulation -----------------------------------

    #[test]
    fn free_function_apply_emulation_works() {
        let config = EmulationConfig::new();
        let mut conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "base"))
            .push(IrMessage::text(IrRole::User, "hi"));

        let report = apply_emulation(&config, &[Capability::ExtendedThinking], &mut conv);

        assert_eq!(report.applied.len(), 1);
    }

    // -- engine resolve strategy defaults --------------------------------

    #[test]
    fn engine_resolve_falls_back_to_default() {
        let engine = EmulationEngine::with_defaults();
        let s = engine.resolve_strategy(&Capability::Streaming);
        assert!(matches!(s, EmulationStrategy::Disabled { .. }));
    }
}
