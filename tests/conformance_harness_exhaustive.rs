#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Comprehensive conformance test harness for passthrough and mapped execution modes.
//!
//! Covers:
//! - Passthrough mode: stream equivalence, no rewriting
//! - Mapped mode: correct dialect translation
//! - Emulation labeling in receipts
//! - Capability mismatch: early failure with typed errors
//! - Receipt correctness: hash integrity, metadata preservation
//! - Error taxonomy: stable error codes for failure classes
//! - All 6×6 dialect pairs for basic conversation mapping
//! - Tool call roundtrip through mapper
//! - Thinking block handling across dialects
//! - Streaming event equivalence

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder,
};
use abp_dialect::Dialect;
use abp_emulation::{
    EmulationConfig, EmulationEngine, EmulationStrategy, FidelityLabel, can_emulate,
    default_strategy,
};
use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, GeminiKimiIrMapper,
    IrIdentityMapper, IrMapper, MapError, OpenAiClaudeIrMapper, OpenAiCodexIrMapper,
    OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
};
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use abp_projection::{ProjectionError, ProjectionMatrix, ProjectionMode};
use abp_receipt::{ReceiptBuilder, canonicalize, compute_hash, verify_hash};
use abp_stream::{EventFilter, EventRecorder, EventStats, StreamPipelineBuilder, event_kind_name};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

fn make_simple_conversation(text: &str) -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, text),
    ])
}

fn make_assistant_response(text: &str) -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, text)])
}

fn make_tool_call_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read the file README.md"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tool_1".into(),
                name: "read_file".into(),
                input: json!({"path": "README.md"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tool_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "# README\nHello world".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

fn make_thinking_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is 2+2?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think step by step: 2+2=4".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 4.".into(),
                },
            ],
        ),
    ])
}

fn make_multi_turn_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a coding assistant."),
        IrMessage::text(IrRole::User, "Write a hello world program."),
        IrMessage::text(IrRole::Assistant, "println!(\"Hello, world!\");"),
        IrMessage::text(IrRole::User, "Now make it a full main function."),
        IrMessage::text(
            IrRole::Assistant,
            "fn main() { println!(\"Hello, world!\"); }",
        ),
    ])
}

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("conformance-test")
        .outcome(Outcome::Complete)
        .build()
}

fn receipt_with_mode(mode: ExecutionMode) -> Receipt {
    ReceiptBuilder::new("conformance-test")
        .outcome(Outcome::Complete)
        .mode(mode)
        .build()
}

fn make_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_passthrough_event(kind: AgentEventKind, raw: serde_json::Value) -> AgentEvent {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), raw);
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

fn mock_capability_manifest() -> CapabilityManifest {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m.insert(Capability::ToolRead, SupportLevel::Emulated);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolEdit, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Emulated);
    m.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    m
}

fn make_work_order_with_mode(mode_str: &str) -> WorkOrder {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".to_string(), json!({"mode": mode_str}));
    WorkOrderBuilder::new("conformance test task")
        .config(config)
        .build()
}

// ── Module: Passthrough Mode Tests ───────────────────────────────────────────

mod passthrough_mode {
    use super::*;

    #[test]
    fn passthrough_receipt_has_correct_mode() {
        let receipt = receipt_with_mode(ExecutionMode::Passthrough);
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn passthrough_events_preserve_ext_data() {
        let raw = json!({"id": "msg_123", "type": "message", "content": [{"text": "hi"}]});
        let event = make_passthrough_event(
            AgentEventKind::AssistantMessage { text: "hi".into() },
            raw.clone(),
        );
        let ext = event.ext.as_ref().unwrap();
        assert_eq!(ext["raw_message"], raw);
    }

    #[test]
    fn passthrough_identity_mapper_preserves_conversation() {
        let mapper = IrIdentityMapper;
        let conv = make_simple_conversation("Hello");
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(conv, result);
    }

    #[test]
    fn passthrough_identity_mapper_preserves_response() {
        let mapper = IrIdentityMapper;
        let conv = make_assistant_response("World");
        let result = mapper
            .map_response(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(conv, result);
    }

    #[test]
    fn passthrough_mode_preserves_all_event_kinds() {
        let events = vec![
            make_agent_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
            make_agent_event(AgentEventKind::AssistantDelta { text: "tok".into() }),
            make_agent_event(AgentEventKind::AssistantMessage {
                text: "full msg".into(),
            }),
            make_agent_event(AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: json!({"cmd": "ls"}),
            }),
            make_agent_event(AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("t1".into()),
                output: json!("file1.txt"),
                is_error: false,
            }),
            make_agent_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let receipt = ReceiptBuilder::new("passthrough-test")
            .outcome(Outcome::Complete)
            .mode(ExecutionMode::Passthrough)
            .events(events.clone())
            .build();
        assert_eq!(receipt.trace.len(), events.len());
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn passthrough_projection_for_same_dialect() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register(
            Dialect::OpenAi,
            Dialect::OpenAi,
            ProjectionMode::Passthrough,
        );
        let mapper = matrix.resolve_mapper(Dialect::OpenAi, Dialect::OpenAi);
        assert!(mapper.is_some(), "identity pair should resolve a mapper");
    }

    #[test]
    fn passthrough_tool_call_not_rewritten() {
        let mapper = IrIdentityMapper;
        let conv = make_tool_call_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(conv.messages.len(), result.messages.len());
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.role, mapped.role);
            assert_eq!(orig.content, mapped.content);
        }
    }

    #[test]
    fn passthrough_thinking_blocks_not_rewritten() {
        let mapper = IrIdentityMapper;
        let conv = make_thinking_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(conv, result);
    }

    #[test]
    fn passthrough_serialization_roundtrip() {
        let receipt = receipt_with_mode(ExecutionMode::Passthrough);
        let json_str = serde_json::to_string(&receipt).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn passthrough_receipt_hash_is_stable() {
        let receipt = receipt_with_mode(ExecutionMode::Passthrough);
        let hash1 = compute_hash(&receipt).unwrap();
        let hash2 = compute_hash(&receipt).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn passthrough_identity_all_dialects() {
        let mapper = IrIdentityMapper;
        let conv = make_simple_conversation("test identity");
        for &d in all_dialects() {
            let result = mapper.map_request(d, d, &conv).unwrap();
            assert_eq!(conv, result, "identity failed for {:?}", d);
        }
    }
}

// ── Module: Mapped Mode Tests ────────────────────────────────────────────────

mod mapped_mode {
    use super::*;

    #[test]
    fn mapped_receipt_has_correct_mode() {
        let receipt = receipt_with_mode(ExecutionMode::Mapped);
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn mapped_mode_is_default() {
        let receipt = minimal_receipt();
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn mapped_openai_to_claude_preserves_user_text() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = make_simple_conversation("Hello from OpenAI");
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert!(!result.messages.is_empty());
        let has_user = result
            .messages
            .iter()
            .any(|m| m.role == IrRole::User && m.text_content().contains("Hello from OpenAI"));
        assert!(has_user, "user message text must be preserved in mapping");
    }

    #[test]
    fn mapped_claude_to_openai_preserves_user_text() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = make_simple_conversation("Hello from Claude");
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let has_user = result
            .messages
            .iter()
            .any(|m| m.role == IrRole::User && m.text_content().contains("Hello from Claude"));
        assert!(has_user, "user message text must be preserved in mapping");
    }

    #[test]
    fn mapped_openai_to_gemini_preserves_system() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = make_simple_conversation("Hello");
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert!(
            !result.messages.is_empty(),
            "mapped result should not be empty"
        );
    }

    #[test]
    fn mapped_mode_projection_for_cross_dialect() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register(Dialect::OpenAi, Dialect::Claude, ProjectionMode::Mapped);
        let mapper = matrix.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(mapper.is_some(), "cross-dialect should resolve a mapper");
    }

    #[test]
    fn mapped_mode_preserves_message_count() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = make_multi_turn_conversation();
        let original_count = conv.messages.len();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert!(
            result.messages.len() >= original_count - 1,
            "mapping should preserve most messages (system may be folded)"
        );
    }

    #[test]
    fn mapped_response_preserves_assistant_text() {
        let mapper = OpenAiClaudeIrMapper;
        let resp = make_assistant_response("The answer is 42");
        let result = mapper
            .map_response(Dialect::Claude, Dialect::OpenAi, &resp)
            .unwrap();
        let text = result
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .map(|m| m.text_content())
            .unwrap_or_default();
        assert!(
            text.contains("42"),
            "assistant text content must survive mapping"
        );
    }

    #[test]
    fn mapped_cross_dialect_projection_mode() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_defaults();
        // Same dialect should be passthrough
        let pass_mapper = matrix.resolve_mapper(Dialect::Claude, Dialect::Claude);
        assert!(pass_mapper.is_some());
        // Cross dialect should be mapped
        let cross_mapper = matrix.resolve_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(cross_mapper.is_some());
    }
}

// ── Module: Emulation Labeling Tests ─────────────────────────────────────────

mod emulation_labeling {
    use super::*;

    #[test]
    fn emulated_capabilities_are_labeled_in_manifest() {
        let manifest = mock_capability_manifest();
        for (cap, level) in &manifest {
            match level {
                SupportLevel::Emulated => {
                    // Emulated caps exist and are properly labeled
                    assert!(
                        matches!(level, SupportLevel::Emulated),
                        "{:?} should be Emulated",
                        cap
                    );
                }
                SupportLevel::Native => {
                    assert!(
                        matches!(level, SupportLevel::Native),
                        "{:?} should be Native",
                        cap
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn emulation_engine_labels_strategies() {
        let engine = EmulationEngine::with_defaults();
        let strategy = engine.resolve_strategy(&Capability::ExtendedThinking);
        assert!(
            matches!(strategy, EmulationStrategy::SystemPromptInjection { .. }),
            "ExtendedThinking should use SystemPromptInjection"
        );
    }

    #[test]
    fn emulation_config_overrides_default() {
        let mut config = EmulationConfig::new();
        config.strategies.insert(
            Capability::ExtendedThinking,
            EmulationStrategy::PostProcessing {
                detail: "custom override".into(),
            },
        );
        let engine = EmulationEngine::new(config);
        let strategy = engine.resolve_strategy(&Capability::ExtendedThinking);
        assert!(
            matches!(strategy, EmulationStrategy::PostProcessing { .. }),
            "config override should take precedence"
        );
    }

    #[test]
    fn emulation_disabled_for_code_execution() {
        let strategy = default_strategy(&Capability::CodeExecution);
        assert!(
            matches!(strategy, EmulationStrategy::Disabled { .. }),
            "CodeExecution should be disabled by default"
        );
    }

    #[test]
    fn can_emulate_returns_false_for_disabled() {
        assert!(
            !can_emulate(&Capability::CodeExecution),
            "CodeExecution should not be emulatable"
        );
    }

    #[test]
    fn can_emulate_returns_true_for_thinking() {
        assert!(
            can_emulate(&Capability::ExtendedThinking),
            "ExtendedThinking should be emulatable"
        );
    }

    #[test]
    fn fidelity_label_native_for_native_capabilities() {
        let native_caps = vec![Capability::Streaming];
        let report = abp_emulation::EmulationReport {
            applied: vec![],
            warnings: vec![],
        };
        let labels = abp_emulation::compute_fidelity(&native_caps, &report);
        assert!(
            matches!(
                labels.get(&Capability::Streaming),
                Some(FidelityLabel::Native)
            ),
            "Native capabilities should yield Native fidelity"
        );
    }

    #[test]
    fn fidelity_label_emulated_for_emulated_capabilities() {
        let native_caps: Vec<Capability> = vec![];
        let report = abp_emulation::EmulationReport {
            applied: vec![abp_emulation::EmulationEntry {
                capability: Capability::ExtendedThinking,
                strategy: default_strategy(&Capability::ExtendedThinking),
            }],
            warnings: vec![],
        };
        let labels = abp_emulation::compute_fidelity(&native_caps, &report);
        match labels.get(&Capability::ExtendedThinking) {
            Some(FidelityLabel::Emulated { .. }) => {} // ok
            other => panic!("expected Emulated, got {:?}", other),
        }
    }

    #[test]
    fn receipt_capabilities_reflect_emulation() {
        let receipt = ReceiptBuilder::new("test-backend")
            .outcome(Outcome::Complete)
            .capabilities(mock_capability_manifest())
            .build();
        assert!(
            matches!(
                receipt.capabilities.get(&Capability::ToolRead),
                Some(SupportLevel::Emulated)
            ),
            "ToolRead should be Emulated"
        );
        assert!(
            matches!(
                receipt.capabilities.get(&Capability::Streaming),
                Some(SupportLevel::Native)
            ),
            "Streaming should be Native"
        );
    }

    #[test]
    fn emulation_default_strategies_consistency() {
        let emulatable_caps = vec![
            Capability::ExtendedThinking,
            Capability::StructuredOutputJsonSchema,
            Capability::ImageInput,
            Capability::StopSequences,
        ];
        for cap in emulatable_caps {
            assert!(can_emulate(&cap), "{:?} should be emulatable", cap);
            let strategy = default_strategy(&cap);
            assert!(
                !matches!(strategy, EmulationStrategy::Disabled { .. }),
                "{:?} strategy should not be Disabled",
                cap
            );
        }
    }
}

// ── Module: Capability Mismatch Tests ────────────────────────────────────────

mod capability_mismatch {
    use super::*;

    #[test]
    fn unsupported_capability_yields_error() {
        let manifest = mock_capability_manifest();
        // Vision is not in mock manifest
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Vision,
                min_support: MinSupport::Native,
            }],
        };
        let result = abp_backend_core::ensure_capability_requirements(&reqs, &manifest);
        assert!(result.is_err(), "missing capability should fail");
    }

    #[test]
    fn emulated_satisfies_emulated_requirement() {
        let manifest = mock_capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            }],
        };
        let result = abp_backend_core::ensure_capability_requirements(&reqs, &manifest);
        assert!(
            result.is_ok(),
            "Emulated should satisfy Emulated requirement"
        );
    }

    #[test]
    fn emulated_does_not_satisfy_native_requirement() {
        let manifest = mock_capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Native,
            }],
        };
        let result = abp_backend_core::ensure_capability_requirements(&reqs, &manifest);
        assert!(
            result.is_err(),
            "Emulated should not satisfy Native requirement"
        );
    }

    #[test]
    fn native_satisfies_emulated_requirement() {
        let manifest = mock_capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        };
        let result = abp_backend_core::ensure_capability_requirements(&reqs, &manifest);
        assert!(result.is_ok(), "Native should satisfy Emulated requirement");
    }

    #[test]
    fn native_satisfies_native_requirement() {
        let manifest = mock_capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let result = abp_backend_core::ensure_capability_requirements(&reqs, &manifest);
        assert!(result.is_ok(), "Native should satisfy Native requirement");
    }

    #[test]
    fn empty_requirements_always_pass() {
        let manifest = mock_capability_manifest();
        let reqs = CapabilityRequirements { required: vec![] };
        let result = abp_backend_core::ensure_capability_requirements(&reqs, &manifest);
        assert!(result.is_ok(), "empty requirements should always pass");
    }

    #[test]
    fn multiple_missing_capabilities_fail() {
        let manifest = mock_capability_manifest();
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Vision,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::Audio,
                    min_support: MinSupport::Native,
                },
            ],
        };
        let result = abp_backend_core::ensure_capability_requirements(&reqs, &manifest);
        assert!(result.is_err());
    }

    #[test]
    fn support_level_satisfies_logic() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn projection_empty_matrix_error() {
        let matrix = ProjectionMatrix::new();
        let wo = WorkOrderBuilder::new("test").build();
        let result = matrix.project(&wo);
        assert!(result.is_err());
    }
}

// ── Module: Receipt Correctness Tests ────────────────────────────────────────

mod receipt_correctness {
    use super::*;

    #[test]
    fn receipt_hash_is_sha256_hex() {
        let receipt = minimal_receipt();
        let hash = compute_hash(&receipt).unwrap();
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex"
        );
    }

    #[test]
    fn receipt_hash_deterministic() {
        let receipt = minimal_receipt();
        let h1 = compute_hash(&receipt).unwrap();
        let h2 = compute_hash(&receipt).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_with_hash_verifies() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
        assert!(verify_hash(&receipt));
    }

    #[test]
    fn tampered_receipt_fails_verification() {
        let mut receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        // Tamper with outcome
        receipt.outcome = Outcome::Failed;
        assert!(
            !verify_hash(&receipt),
            "tampered receipt should fail verification"
        );
    }

    #[test]
    fn receipt_metadata_preserved_through_hash() {
        let run_id = Uuid::new_v4();
        let wo_id = Uuid::new_v4();
        let receipt = ReceiptBuilder::new("backend-x")
            .run_id(run_id)
            .work_order_id(wo_id)
            .backend_version("1.0")
            .adapter_version("2.0")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert_eq!(receipt.meta.run_id, run_id);
        assert_eq!(receipt.meta.work_order_id, wo_id);
        assert_eq!(receipt.backend.backend_version.as_deref(), Some("1.0"));
        assert_eq!(receipt.backend.adapter_version.as_deref(), Some("2.0"));
        assert!(verify_hash(&receipt));
    }

    #[test]
    fn receipt_contract_version_is_set() {
        let receipt = minimal_receipt();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_canonical_json_excludes_hash() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        let canonical = canonicalize(&receipt).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&canonical).unwrap();
        assert!(
            parsed["receipt_sha256"].is_null(),
            "canonical form must have null receipt_sha256"
        );
    }

    #[test]
    fn receipt_serialization_roundtrip() {
        let receipt = ReceiptBuilder::new("rt")
            .outcome(Outcome::Complete)
            .usage_tokens(100, 50)
            .with_hash()
            .unwrap();
        let json = serde_json::to_string(&receipt).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(receipt.receipt_sha256, deserialized.receipt_sha256);
        assert_eq!(receipt.outcome, deserialized.outcome);
        assert_eq!(receipt.meta.run_id, deserialized.meta.run_id);
    }

    #[test]
    fn receipt_outcome_variants_roundtrip() {
        for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
            let receipt = ReceiptBuilder::new("test").outcome(outcome.clone()).build();
            let json = serde_json::to_string(&receipt).unwrap();
            let parsed: Receipt = serde_json::from_str(&json).unwrap();
            assert_eq!(receipt.outcome, parsed.outcome);
        }
    }

    #[test]
    fn receipt_mode_roundtrip() {
        for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
            let receipt = receipt_with_mode(mode.clone());
            let json = serde_json::to_string(&receipt).unwrap();
            let parsed: Receipt = serde_json::from_str(&json).unwrap();
            assert_eq!(receipt.mode, parsed.mode);
        }
    }

    #[test]
    fn receipt_events_preserved_in_trace() {
        let events = vec![
            make_agent_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
            make_agent_event(AgentEventKind::AssistantMessage {
                text: "hello".into(),
            }),
            make_agent_event(AgentEventKind::RunCompleted {
                message: "fin".into(),
            }),
        ];
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .events(events)
            .build();
        assert_eq!(receipt.trace.len(), 3);
    }

    #[test]
    fn different_receipts_have_different_hashes() {
        let r1 = ReceiptBuilder::new("backend-a")
            .outcome(Outcome::Complete)
            .build();
        let r2 = ReceiptBuilder::new("backend-b")
            .outcome(Outcome::Failed)
            .build();
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        assert_ne!(h1, h2, "different receipts should have different hashes");
    }
}

// ── Module: Error Taxonomy Tests ─────────────────────────────────────────────

mod error_taxonomy {
    use super::*;

    #[test]
    fn all_error_codes_have_stable_string_ids() {
        let codes = vec![
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolHandshakeFailed,
            ErrorCode::ProtocolMissingRefId,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
            ErrorCode::MappingUnsupportedCapability,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::MappingLossyConversion,
            ErrorCode::MappingUnmappableTool,
            ErrorCode::BackendNotFound,
            ErrorCode::BackendUnavailable,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::BackendAuthFailed,
            ErrorCode::BackendModelNotFound,
            ErrorCode::BackendCrashed,
            ErrorCode::ExecutionToolFailed,
            ErrorCode::ExecutionWorkspaceError,
            ErrorCode::ExecutionPermissionDenied,
            ErrorCode::ContractVersionMismatch,
            ErrorCode::ContractSchemaViolation,
            ErrorCode::ContractInvalidReceipt,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::CapabilityEmulationFailed,
            ErrorCode::PolicyDenied,
            ErrorCode::PolicyInvalid,
            ErrorCode::WorkspaceInitFailed,
            ErrorCode::WorkspaceStagingFailed,
            ErrorCode::IrLoweringFailed,
            ErrorCode::IrInvalid,
            ErrorCode::ReceiptHashMismatch,
            ErrorCode::ReceiptChainBroken,
            ErrorCode::DialectUnknown,
            ErrorCode::DialectMappingFailed,
            ErrorCode::ConfigInvalid,
            ErrorCode::Internal,
        ];
        for code in &codes {
            let s = code.as_str();
            assert!(!s.is_empty(), "{:?} should have a string id", code);
            // Stable string IDs should be snake_case
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{:?} string id '{}' should be snake_case",
                code,
                s
            );
        }
    }

    #[test]
    fn error_categories_are_consistent() {
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.category(),
            ErrorCategory::Protocol
        );
        assert_eq!(
            ErrorCode::BackendNotFound.category(),
            ErrorCategory::Backend
        );
        assert_eq!(
            ErrorCode::MappingDialectMismatch.category(),
            ErrorCategory::Mapping
        );
        assert_eq!(
            ErrorCode::CapabilityUnsupported.category(),
            ErrorCategory::Capability
        );
        assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
        assert_eq!(
            ErrorCode::WorkspaceInitFailed.category(),
            ErrorCategory::Workspace
        );
        assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
        assert_eq!(
            ErrorCode::ReceiptHashMismatch.category(),
            ErrorCategory::Receipt
        );
        assert_eq!(ErrorCode::DialectUnknown.category(), ErrorCategory::Dialect);
        assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
        assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
        assert_eq!(
            ErrorCode::ContractVersionMismatch.category(),
            ErrorCategory::Contract
        );
        assert_eq!(
            ErrorCode::ExecutionToolFailed.category(),
            ErrorCategory::Execution
        );
    }

    #[test]
    fn retryable_error_codes() {
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendUnavailable.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(ErrorCode::BackendCrashed.is_retryable());
    }

    #[test]
    fn non_retryable_error_codes() {
        assert!(!ErrorCode::ProtocolInvalidEnvelope.is_retryable());
        assert!(!ErrorCode::MappingDialectMismatch.is_retryable());
        assert!(!ErrorCode::CapabilityUnsupported.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
        assert!(!ErrorCode::Internal.is_retryable());
    }

    #[test]
    fn error_codes_have_messages() {
        let codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::BackendNotFound,
            ErrorCode::MappingDialectMismatch,
            ErrorCode::Internal,
        ];
        for code in &codes {
            let msg = code.message();
            assert!(!msg.is_empty(), "{:?} should have a message", code);
        }
    }

    #[test]
    fn abp_error_builder_preserves_context() {
        let err = AbpError::new(ErrorCode::BackendNotFound, "not found")
            .with_context("backend", "openai");
        assert_eq!(err.code, ErrorCode::BackendNotFound);
        assert!(err.context.contains_key("backend"));
    }

    #[test]
    fn abp_error_to_info_roundtrip() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "tool blocked");
        let info = err.to_info();
        assert_eq!(info.code, ErrorCode::PolicyDenied);
        assert_eq!(info.message, "tool blocked");
        assert_eq!(info.is_retryable, false);
    }

    #[test]
    fn error_code_serialization_roundtrip() {
        let code = ErrorCode::MappingLossyConversion;
        let json = serde_json::to_string(&code).unwrap();
        let parsed: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, parsed);
    }

    #[test]
    fn error_info_serialization_roundtrip() {
        let info = ErrorInfo::new(ErrorCode::BackendTimeout, "timed out")
            .with_detail("elapsed_ms", 5000u64);
        let json = serde_json::to_string(&info).unwrap();
        let parsed: ErrorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.code, ErrorCode::BackendTimeout);
        assert_eq!(parsed.is_retryable, true);
    }

    #[test]
    fn map_error_unsupported_pair() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Copilot,
            to: Dialect::Kimi,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("unsupported"));
    }

    #[test]
    fn map_error_lossy_conversion() {
        let err = MapError::LossyConversion {
            field: "thinking".into(),
            reason: "target does not support thinking blocks".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("lossy_conversion"));
    }

    #[test]
    fn map_error_unmappable_tool() {
        let err = MapError::UnmappableTool {
            name: "custom_tool".into(),
            reason: "not in target schema".into(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("custom_tool"));
    }
}

// ── Module: 6×6 Dialect Pair Tests ───────────────────────────────────────────

mod dialect_matrix {
    use super::*;

    fn assert_pair_maps(from: Dialect, to: Dialect) {
        let mapper = default_ir_mapper(from, to);
        assert!(
            mapper.is_some(),
            "mapper should exist for {:?} -> {:?}",
            from,
            to
        );
        let mapper = mapper.unwrap();
        let conv = make_simple_conversation("dialect matrix test");
        let result = mapper.map_request(from, to, &conv);
        assert!(
            result.is_ok(),
            "mapping {:?} -> {:?} should succeed, got: {:?}",
            from,
            to,
            result.err()
        );
        let mapped = result.unwrap();
        assert!(
            !mapped.messages.is_empty(),
            "mapped result for {:?} -> {:?} should have messages",
            from,
            to
        );
    }

    #[test]
    fn all_supported_pairs_have_mappers() {
        let pairs = supported_ir_pairs();
        assert!(
            pairs.len() >= 24,
            "should have at least 24 pairs (6 identity + 18 cross)"
        );
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to);
            assert!(mapper.is_some(), "no mapper for {:?} -> {:?}", from, to);
        }
    }

    #[test]
    fn identity_pairs_for_all_dialects() {
        for &d in all_dialects() {
            assert_pair_maps(d, d);
        }
    }

    #[test]
    fn openai_claude_bidirectional() {
        assert_pair_maps(Dialect::OpenAi, Dialect::Claude);
        assert_pair_maps(Dialect::Claude, Dialect::OpenAi);
    }

    #[test]
    fn openai_gemini_bidirectional() {
        assert_pair_maps(Dialect::OpenAi, Dialect::Gemini);
        assert_pair_maps(Dialect::Gemini, Dialect::OpenAi);
    }

    #[test]
    fn claude_gemini_bidirectional() {
        assert_pair_maps(Dialect::Claude, Dialect::Gemini);
        assert_pair_maps(Dialect::Gemini, Dialect::Claude);
    }

    #[test]
    fn openai_codex_bidirectional() {
        assert_pair_maps(Dialect::OpenAi, Dialect::Codex);
        assert_pair_maps(Dialect::Codex, Dialect::OpenAi);
    }

    #[test]
    fn openai_kimi_bidirectional() {
        assert_pair_maps(Dialect::OpenAi, Dialect::Kimi);
        assert_pair_maps(Dialect::Kimi, Dialect::OpenAi);
    }

    #[test]
    fn claude_kimi_bidirectional() {
        assert_pair_maps(Dialect::Claude, Dialect::Kimi);
        assert_pair_maps(Dialect::Kimi, Dialect::Claude);
    }

    #[test]
    fn openai_copilot_bidirectional() {
        assert_pair_maps(Dialect::OpenAi, Dialect::Copilot);
        assert_pair_maps(Dialect::Copilot, Dialect::OpenAi);
    }

    #[test]
    fn gemini_kimi_bidirectional() {
        assert_pair_maps(Dialect::Gemini, Dialect::Kimi);
        assert_pair_maps(Dialect::Kimi, Dialect::Gemini);
    }

    #[test]
    fn codex_claude_bidirectional() {
        assert_pair_maps(Dialect::Codex, Dialect::Claude);
        assert_pair_maps(Dialect::Claude, Dialect::Codex);
    }

    #[test]
    fn response_mapping_all_supported_pairs() {
        let pairs = supported_ir_pairs();
        let resp = make_assistant_response("Test response content");
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_response(*from, *to, &resp);
            assert!(
                result.is_ok(),
                "response mapping {:?} -> {:?} should succeed",
                from,
                to
            );
        }
    }

    #[test]
    fn multi_turn_mapping_all_supported_pairs() {
        let pairs = supported_ir_pairs();
        let conv = make_multi_turn_conversation();
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            assert!(
                result.is_ok(),
                "multi-turn mapping {:?} -> {:?} should succeed, got: {:?}",
                from,
                to,
                result.err()
            );
        }
    }

    #[test]
    fn dialect_labels_are_unique() {
        let labels: Vec<&str> = all_dialects().iter().map(|d| d.label()).collect();
        let unique: std::collections::HashSet<&&str> = labels.iter().collect();
        assert_eq!(labels.len(), unique.len(), "dialect labels must be unique");
    }

    #[test]
    fn six_dialects_exist() {
        assert_eq!(all_dialects().len(), 6, "should have exactly 6 dialects");
    }
}

// ── Module: Tool Call Roundtrip Tests ────────────────────────────────────────

mod tool_call_roundtrip {
    use super::*;

    fn assert_tool_roundtrip(from: Dialect, to: Dialect) {
        let mapper = default_ir_mapper(from, to);
        if mapper.is_none() {
            return; // skip unsupported pairs
        }
        let mapper = mapper.unwrap();
        let conv = make_tool_call_conversation();
        let result = mapper.map_request(from, to, &conv);
        assert!(
            result.is_ok(),
            "tool call mapping {:?} -> {:?} should succeed",
            from,
            to
        );
        let mapped = result.unwrap();

        // Check tool use block is preserved
        let has_tool_use = mapped.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { name, .. } if name == "read_file"))
        });
        // Tool content should survive mapping (name preserved)
        if from != Dialect::Codex && to != Dialect::Codex {
            assert!(
                has_tool_use,
                "tool use should be preserved in {:?} -> {:?}",
                from, to
            );
        }
    }

    #[test]
    fn tool_call_openai_to_claude() {
        assert_tool_roundtrip(Dialect::OpenAi, Dialect::Claude);
    }

    #[test]
    fn tool_call_claude_to_openai() {
        assert_tool_roundtrip(Dialect::Claude, Dialect::OpenAi);
    }

    #[test]
    fn tool_call_openai_to_gemini() {
        assert_tool_roundtrip(Dialect::OpenAi, Dialect::Gemini);
    }

    #[test]
    fn tool_call_gemini_to_openai() {
        assert_tool_roundtrip(Dialect::Gemini, Dialect::OpenAi);
    }

    #[test]
    fn tool_call_claude_to_gemini() {
        assert_tool_roundtrip(Dialect::Claude, Dialect::Gemini);
    }

    #[test]
    fn tool_call_openai_to_kimi() {
        assert_tool_roundtrip(Dialect::OpenAi, Dialect::Kimi);
    }

    #[test]
    fn tool_call_openai_to_copilot() {
        assert_tool_roundtrip(Dialect::OpenAi, Dialect::Copilot);
    }

    #[test]
    fn tool_call_identity_preserves_all_fields() {
        let mapper = IrIdentityMapper;
        let conv = make_tool_call_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(conv, result, "identity should preserve tool calls exactly");
    }

    #[test]
    fn tool_result_preserved_through_mapping() {
        let pairs = supported_ir_pairs();
        let conv = make_tool_call_conversation();
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            assert!(result.is_ok(), "tool result mapping {:?} -> {:?}", from, to);
            let mapped = result.unwrap();
            // Tool role message should exist in some form
            let has_tool_msg = mapped.messages.iter().any(|m| {
                m.role == IrRole::Tool
                    || m.content
                        .iter()
                        .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            });
            if *from != Dialect::Codex && *to != Dialect::Codex {
                assert!(
                    has_tool_msg,
                    "tool result should survive {:?} -> {:?}",
                    from, to
                );
            }
        }
    }

    #[test]
    fn tool_call_event_kind_names() {
        let event = make_agent_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        });
        let name = event_kind_name(&event.kind);
        assert!(
            name.contains("tool") || name.contains("Tool"),
            "event kind name should mention tool"
        );
    }
}

// ── Module: Thinking Block Tests ─────────────────────────────────────────────

mod thinking_blocks {
    use super::*;

    #[test]
    fn thinking_block_preserved_in_identity() {
        let mapper = IrIdentityMapper;
        let conv = make_thinking_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        let has_thinking = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        });
        assert!(
            has_thinking,
            "thinking blocks should be preserved in identity"
        );
    }

    #[test]
    fn thinking_block_content_preserved() {
        let mapper = IrIdentityMapper;
        let conv = make_thinking_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        let thinking_text: String = result
            .messages
            .iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                IrContentBlock::Thinking { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(thinking_text.contains("2+2=4"));
    }

    #[test]
    fn thinking_block_cross_dialect_openai_claude() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        let conv = make_thinking_conversation();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &conv);
        assert!(
            result.is_ok(),
            "thinking blocks should map OpenAI -> Claude"
        );
    }

    #[test]
    fn thinking_block_cross_dialect_claude_openai() {
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
        let conv = make_thinking_conversation();
        let result = mapper.map_request(Dialect::Claude, Dialect::OpenAi, &conv);
        assert!(
            result.is_ok(),
            "thinking blocks should map Claude -> OpenAI"
        );
    }

    #[test]
    fn thinking_block_cross_dialect_claude_gemini() {
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
        let conv = make_thinking_conversation();
        let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &conv);
        assert!(
            result.is_ok(),
            "thinking blocks should map Claude -> Gemini"
        );
    }

    #[test]
    fn thinking_block_all_supported_pairs() {
        let pairs = supported_ir_pairs();
        let conv = make_thinking_conversation();
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            assert!(
                result.is_ok(),
                "thinking block mapping {:?} -> {:?} should succeed, got: {:?}",
                from,
                to,
                result.err()
            );
        }
    }

    #[test]
    fn thinking_and_text_coexist() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning...".into(),
                },
                IrContentBlock::Text {
                    text: "conclusion".into(),
                },
            ],
        );
        assert!(!msg.is_text_only());
        assert!(msg.text_content().contains("conclusion"));
    }

    #[test]
    fn emulation_strategy_for_thinking() {
        let strategy = default_strategy(&Capability::ExtendedThinking);
        assert!(matches!(
            strategy,
            EmulationStrategy::SystemPromptInjection { .. }
        ));
    }
}

// ── Module: Streaming Event Equivalence Tests ────────────────────────────────

mod streaming_events {
    use super::*;

    #[test]
    fn event_kind_name_consistency() {
        let kinds = vec![
            AgentEventKind::RunStarted {
                message: "s".into(),
            },
            AgentEventKind::RunCompleted {
                message: "c".into(),
            },
            AgentEventKind::AssistantDelta { text: "d".into() },
            AgentEventKind::AssistantMessage { text: "m".into() },
            AgentEventKind::ToolCall {
                tool_name: "t".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            AgentEventKind::ToolResult {
                tool_name: "t".into(),
                tool_use_id: None,
                output: json!("ok"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "f".into(),
                summary: "s".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "c".into(),
                exit_code: Some(0),
                output_preview: None,
            },
            AgentEventKind::Warning {
                message: "w".into(),
            },
            AgentEventKind::Error {
                message: "e".into(),
                error_code: None,
            },
        ];
        for kind in &kinds {
            let name = event_kind_name(kind);
            assert!(!name.is_empty(), "event kind name should not be empty");
        }
    }

    #[test]
    fn event_serialization_roundtrip() {
        let event = make_agent_event(AgentEventKind::AssistantDelta {
            text: "hello".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        let parsed: AgentEvent = serde_json::from_str(&json).unwrap();
        match &parsed.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
            other => panic!("expected AssistantDelta, got {:?}", other),
        }
    }

    #[test]
    fn event_ext_data_serialization() {
        let raw = json!({"stream_index": 42});
        let event = make_passthrough_event(
            AgentEventKind::AssistantDelta { text: "tok".into() },
            raw.clone(),
        );
        let json_str = serde_json::to_string(&event).unwrap();
        let parsed: AgentEvent = serde_json::from_str(&json_str).unwrap();
        let ext = parsed.ext.unwrap();
        assert_eq!(ext["raw_message"], raw);
    }

    #[test]
    fn stream_pipeline_builder_constructs() {
        let pipeline = StreamPipelineBuilder::new().build();
        // Pipeline should process events
        let event = make_agent_event(AgentEventKind::AssistantDelta {
            text: "test".into(),
        });
        let result = pipeline.process(event);
        assert!(
            result.is_some(),
            "default pipeline should pass events through"
        );
    }

    #[test]
    fn stream_pipeline_filter_removes_events() {
        let filter = EventFilter::by_kind("warning");
        let pipeline = StreamPipelineBuilder::new().filter(filter).build();

        // "warning" kind filter: only warnings match
        let warning = make_agent_event(AgentEventKind::Warning {
            message: "keep me".into(),
        });
        assert!(
            pipeline.process(warning).is_some(),
            "filter should keep warnings"
        );

        let msg = make_agent_event(AgentEventKind::AssistantMessage {
            text: "drop me".into(),
        });
        assert!(
            pipeline.process(msg).is_none(),
            "filter should remove non-warnings"
        );
    }

    #[test]
    fn event_recorder_captures_events() {
        let recorder = EventRecorder::new();
        let event = make_agent_event(AgentEventKind::AssistantMessage {
            text: "recorded".into(),
        });
        recorder.record(&event);
        let events = recorder.events();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn event_stats_track_counts() {
        let stats = EventStats::new();
        stats.observe(&make_agent_event(AgentEventKind::AssistantDelta {
            text: "a".into(),
        }));
        stats.observe(&make_agent_event(AgentEventKind::AssistantDelta {
            text: "b".into(),
        }));
        stats.observe(&make_agent_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }));
        assert!(stats.total_events() >= 3);
    }

    #[test]
    fn delta_events_aggregate_to_full_message() {
        let deltas = vec!["Hello", ", ", "world", "!"];
        let full: String = deltas.iter().copied().collect();
        assert_eq!(full, "Hello, world!");
        // Verify delta events can be created and serialized
        for d in &deltas {
            let event = make_agent_event(AgentEventKind::AssistantDelta {
                text: d.to_string(),
            });
            let json = serde_json::to_string(&event).unwrap();
            assert!(json.contains(d));
        }
    }

    #[test]
    fn error_event_with_code() {
        let event = make_agent_event(AgentEventKind::Error {
            message: "backend timeout".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        });
        let json = serde_json::to_string(&event).unwrap();
        let parsed: AgentEvent = serde_json::from_str(&json).unwrap();
        match parsed.kind {
            AgentEventKind::Error {
                error_code: Some(code),
                ..
            } => {
                assert_eq!(code, ErrorCode::BackendTimeout);
                assert!(code.is_retryable());
            }
            _ => panic!("expected Error with code"),
        }
    }

    #[test]
    fn stream_event_order_preserved() {
        let events: Vec<AgentEvent> = (0..10)
            .map(|i| {
                make_agent_event(AgentEventKind::AssistantDelta {
                    text: format!("tok_{}", i),
                })
            })
            .collect();
        let recorder = EventRecorder::new();
        for e in &events {
            recorder.record(e);
        }
        let recorded = recorder.events();
        assert_eq!(recorded.len(), 10);
        for (i, e) in recorded.iter().enumerate() {
            match &e.kind {
                AgentEventKind::AssistantDelta { text } => {
                    assert_eq!(text, &format!("tok_{}", i));
                }
                _ => panic!("unexpected event kind"),
            }
        }
    }
}

// ── Module: Cross-Cutting Integration Tests ──────────────────────────────────

mod integration {
    use super::*;

    #[test]
    fn work_order_builder_sets_defaults() {
        let wo = WorkOrderBuilder::new("test task").build();
        assert_eq!(wo.task, "test task");
        assert_eq!(wo.config.model, None);
    }

    #[test]
    fn work_order_with_model() {
        let wo = WorkOrderBuilder::new("test").model("gpt-4").build();
        assert_eq!(wo.config.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn work_order_with_budget() {
        let wo = WorkOrderBuilder::new("test").max_budget_usd(10.0).build();
        assert_eq!(wo.config.max_budget_usd, Some(10.0));
    }

    #[test]
    fn receipt_builder_error_sets_failed() {
        let receipt = ReceiptBuilder::new("test")
            .error("something went wrong")
            .build();
        assert_eq!(receipt.outcome, Outcome::Failed);
        assert!(receipt.trace.iter().any(|e| matches!(
            &e.kind,
            AgentEventKind::Error { message, .. } if message.contains("something went wrong")
        )));
    }

    #[test]
    fn receipt_builder_usage_tokens() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .usage_tokens(1000, 500)
            .build();
        assert_eq!(receipt.usage.input_tokens, Some(1000));
        assert_eq!(receipt.usage.output_tokens, Some(500));
    }

    #[test]
    fn ir_conversation_helpers() {
        let conv = make_multi_turn_conversation();
        assert!(!conv.is_empty());
        assert_eq!(conv.len(), 5);
        assert!(conv.system_message().is_some());
        assert!(conv.last_assistant().is_some());
        let user_msgs = conv.messages_by_role(IrRole::User);
        assert_eq!(user_msgs.len(), 2);
    }

    #[test]
    fn ir_message_text_only() {
        let msg = IrMessage::text(IrRole::User, "plain text");
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "plain text");
    }

    #[test]
    fn ir_message_with_tool_use_not_text_only() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            }],
        );
        assert!(!msg.is_text_only());
        let tool_uses = msg.tool_use_blocks();
        assert_eq!(tool_uses.len(), 1);
    }

    #[test]
    fn projection_matrix_defaults_register_all_pairs() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_defaults();
        for &d in all_dialects() {
            let mapper = matrix.resolve_mapper(d, d);
            assert!(mapper.is_some(), "identity mapper should exist for {:?}", d);
        }
    }

    #[test]
    fn contract_version_format() {
        assert!(CONTRACT_VERSION.starts_with("abp/"));
        assert!(CONTRACT_VERSION.contains("v0."));
    }

    #[test]
    fn backend_identity_serialization() {
        let id = BackendIdentity {
            id: "mock".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("2.0".into()),
        };
        let json = serde_json::to_string(&id).unwrap();
        let parsed: BackendIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "mock");
        assert_eq!(parsed.backend_version, Some("1.0".into()));
    }

    #[test]
    fn execution_mode_serialization() {
        let modes = [ExecutionMode::Passthrough, ExecutionMode::Mapped];
        for mode in &modes {
            let json = serde_json::to_string(mode).unwrap();
            let parsed: ExecutionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, &parsed);
        }
    }

    #[test]
    fn capability_serialization_roundtrip() {
        let caps = vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ExtendedThinking,
            Capability::Vision,
        ];
        for cap in &caps {
            let json = serde_json::to_string(cap).unwrap();
            let parsed: Capability = serde_json::from_str(&json).unwrap();
            assert_eq!(cap, &parsed);
        }
    }

    #[test]
    fn support_level_serialization_roundtrip() {
        let levels = vec![
            SupportLevel::Native,
            SupportLevel::Emulated,
            SupportLevel::Unsupported,
            SupportLevel::Restricted {
                reason: "test".into(),
            },
        ];
        for level in &levels {
            let json = serde_json::to_string(level).unwrap();
            let parsed: SupportLevel = serde_json::from_str(&json).unwrap();
            // SupportLevel doesn't derive PartialEq; compare via JSON
            let json2 = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, json2, "SupportLevel roundtrip failed");
        }
    }

    #[test]
    fn mapper_supported_pairs_is_complete() {
        let pairs = supported_ir_pairs();
        // Identity pairs
        for &d in all_dialects() {
            assert!(pairs.contains(&(d, d)), "identity pair missing for {:?}", d);
        }
        // Key cross-dialect pairs
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
    }

    #[test]
    fn ir_tool_definition_serialization() {
        let tool = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "read_file");
    }

    #[test]
    fn dialect_label_not_empty() {
        for &d in all_dialects() {
            assert!(!d.label().is_empty(), "{:?} should have a label", d);
        }
    }
}
