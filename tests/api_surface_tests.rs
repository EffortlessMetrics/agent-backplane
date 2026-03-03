//! Semver compliance tests — verifying the public API surface of all crates.
//!
//! These tests will break if someone accidentally changes a public type,
//! removes a field, renames a variant, or drops a trait implementation.

// ═══════════════════════════════════════════════════════════════════════
// 1. abp-core: struct construction & field access
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_work_order_fields() {
    use abp_core::{
        CapabilityRequirements, ContextPacket, ExecutionLane, PolicyProfile, RuntimeConfig,
        WorkOrder, WorkspaceMode, WorkspaceSpec,
    };
    use uuid::Uuid;

    let wo = WorkOrder {
        id: Uuid::nil(),
        task: "t".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::Staged,
            include: vec![],
            exclude: vec![],
        },
        context: ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: CapabilityRequirements::default(),
        config: RuntimeConfig::default(),
    };
    assert_eq!(wo.task, "t");
}

#[test]
fn core_receipt_fields() {
    use abp_core::{
        ArtifactRef, BackendIdentity, CapabilityManifest, ExecutionMode, Outcome, Receipt,
        RunMetadata, UsageNormalized, VerificationReport,
    };
    use chrono::Utc;
    use uuid::Uuid;

    let receipt = Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: abp_core::CONTRACT_VERSION.to_string(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![ArtifactRef {
            kind: "log".into(),
            path: "out.log".into(),
        }],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    assert_eq!(receipt.backend.id, "mock");
}

#[test]
fn core_context_packet_and_snippet() {
    use abp_core::{ContextPacket, ContextSnippet};
    let ctx = ContextPacket {
        files: vec!["a.rs".into()],
        snippets: vec![ContextSnippet {
            name: "n".into(),
            content: "c".into(),
        }],
    };
    assert_eq!(ctx.snippets[0].name, "n");
}

#[test]
fn core_agent_event_fields() {
    use abp_core::{AgentEvent, AgentEventKind};
    use chrono::Utc;

    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    assert!(matches!(ev.kind, AgentEventKind::AssistantMessage { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. abp-core: enum variant exhaustiveness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_execution_lane_variants() {
    use abp_core::ExecutionLane;
    let _p = ExecutionLane::PatchFirst;
    let _w = ExecutionLane::WorkspaceFirst;
}

#[test]
fn core_workspace_mode_variants() {
    use abp_core::WorkspaceMode;
    let _p = WorkspaceMode::PassThrough;
    let _s = WorkspaceMode::Staged;
}

#[test]
fn core_execution_mode_variants_and_default() {
    use abp_core::ExecutionMode;
    let _p = ExecutionMode::Passthrough;
    let _m = ExecutionMode::Mapped;
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn core_outcome_variants() {
    use abp_core::Outcome;
    let _c = Outcome::Complete;
    let _p = Outcome::Partial;
    let _f = Outcome::Failed;
}

#[test]
fn core_min_support_variants() {
    use abp_core::MinSupport;
    let _n = MinSupport::Native;
    let _e = MinSupport::Emulated;
}

#[test]
fn core_support_level_variants() {
    use abp_core::SupportLevel;
    let _n = SupportLevel::Native;
    let _e = SupportLevel::Emulated;
    let _u = SupportLevel::Unsupported;
    let _r = SupportLevel::Restricted { reason: "x".into() };
}

#[test]
fn core_capability_all_variants() {
    use abp_core::Capability;
    let caps = [
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
    assert_eq!(caps.len(), 26);
}

#[test]
fn core_agent_event_kind_variants() {
    use abp_core::AgentEventKind;
    let _v: Vec<AgentEventKind> = vec![
        AgentEventKind::RunStarted { message: "".into() },
        AgentEventKind::RunCompleted { message: "".into() },
        AgentEventKind::AssistantDelta { text: "".into() },
        AgentEventKind::AssistantMessage { text: "".into() },
        AgentEventKind::ToolCall {
            tool_name: "".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::Value::Null,
        },
        AgentEventKind::ToolResult {
            tool_name: "".into(),
            tool_use_id: None,
            output: serde_json::Value::Null,
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "".into(),
            summary: "".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "".into(),
            exit_code: None,
            output_preview: None,
        },
        AgentEventKind::Warning { message: "".into() },
        AgentEventKind::Error {
            message: "".into(),
            error_code: None,
        },
    ];
    assert_eq!(_v.len(), 10);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Trait implementations: Clone, Debug, Serialize, Deserialize, PartialEq
// ═══════════════════════════════════════════════════════════════════════

fn assert_clone<T: Clone>() {}
fn assert_debug<T: std::fmt::Debug>() {}
fn assert_serialize<T: serde::Serialize>() {}
fn assert_deserialize<'de, T: serde::Deserialize<'de>>() {}
fn assert_partial_eq<T: PartialEq>() {}
fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn core_types_implement_clone() {
    assert_clone::<abp_core::WorkOrder>();
    assert_clone::<abp_core::Receipt>();
    assert_clone::<abp_core::AgentEvent>();
    assert_clone::<abp_core::AgentEventKind>();
    assert_clone::<abp_core::Capability>();
    assert_clone::<abp_core::SupportLevel>();
    assert_clone::<abp_core::ExecutionMode>();
    assert_clone::<abp_core::Outcome>();
    assert_clone::<abp_core::BackendIdentity>();
    assert_clone::<abp_core::RuntimeConfig>();
    assert_clone::<abp_core::PolicyProfile>();
    assert_clone::<abp_core::UsageNormalized>();
    assert_clone::<abp_core::VerificationReport>();
    assert_clone::<abp_core::ContextPacket>();
}

#[test]
fn core_types_implement_debug() {
    assert_debug::<abp_core::WorkOrder>();
    assert_debug::<abp_core::Receipt>();
    assert_debug::<abp_core::AgentEvent>();
    assert_debug::<abp_core::WorkOrderBuilder>();
    assert_debug::<abp_core::ReceiptBuilder>();
    assert_debug::<abp_core::ContractError>();
    assert_debug::<abp_core::ExecutionLane>();
    assert_debug::<abp_core::WorkspaceMode>();
}

#[test]
fn core_types_implement_serialize() {
    assert_serialize::<abp_core::WorkOrder>();
    assert_serialize::<abp_core::Receipt>();
    assert_serialize::<abp_core::AgentEvent>();
    assert_serialize::<abp_core::Capability>();
    assert_serialize::<abp_core::Outcome>();
    assert_serialize::<abp_core::ExecutionMode>();
    assert_serialize::<abp_core::SupportLevel>();
}

#[test]
fn core_types_implement_deserialize() {
    assert_deserialize::<abp_core::WorkOrder>();
    assert_deserialize::<abp_core::Receipt>();
    assert_deserialize::<abp_core::AgentEvent>();
    assert_deserialize::<abp_core::Capability>();
    assert_deserialize::<abp_core::Outcome>();
    assert_deserialize::<abp_core::ExecutionMode>();
    assert_deserialize::<abp_core::SupportLevel>();
}

#[test]
fn core_types_implement_partial_eq() {
    assert_partial_eq::<abp_core::Outcome>();
    assert_partial_eq::<abp_core::ExecutionMode>();
    assert_partial_eq::<abp_core::Capability>();
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Send + Sync bounds
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_types_are_send() {
    assert_send::<abp_core::WorkOrder>();
    assert_send::<abp_core::Receipt>();
    assert_send::<abp_core::AgentEvent>();
    assert_send::<abp_core::Capability>();
    assert_send::<abp_core::ContractError>();
}

#[test]
fn core_types_are_sync() {
    assert_sync::<abp_core::WorkOrder>();
    assert_sync::<abp_core::Receipt>();
    assert_sync::<abp_core::AgentEvent>();
    assert_sync::<abp_core::Capability>();
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Default implementations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_default_impls() {
    let _: abp_core::ContextPacket = Default::default();
    let _: abp_core::RuntimeConfig = Default::default();
    let _: abp_core::PolicyProfile = Default::default();
    let _: abp_core::CapabilityRequirements = Default::default();
    let _: abp_core::UsageNormalized = Default::default();
    let _: abp_core::VerificationReport = Default::default();
    let _: abp_core::ExecutionMode = Default::default();
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Constants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_contract_version_stable() {
    assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Builder patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_work_order_builder_chainable() {
    use abp_core::{ExecutionLane, WorkOrderBuilder, WorkspaceMode};

    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/**".into()])
        .model("gpt-4")
        .max_budget_usd(10.0)
        .max_turns(5)
        .build();

    assert_eq!(wo.task, "task");
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    assert_eq!(wo.config.max_turns, Some(5));
    assert_eq!(wo.workspace.root, "/tmp");
}

#[test]
fn core_receipt_builder_chainable() {
    use abp_core::{ExecutionMode, Outcome, ReceiptBuilder};

    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Failed)
        .mode(ExecutionMode::Passthrough)
        .backend_version("1.0")
        .adapter_version("0.1")
        .build();

    assert_eq!(receipt.backend.id, "test-backend");
    assert_eq!(receipt.outcome, Outcome::Failed);
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn core_receipt_builder_with_hash() {
    use abp_core::{Outcome, ReceiptBuilder};

    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .expect("hashing should succeed");

    assert!(receipt.receipt_sha256.is_some());
    assert_eq!(receipt.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Public functions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_canonical_json_accessible() {
    let val = serde_json::json!({"b": 2, "a": 1});
    let json = abp_core::canonical_json(&val).unwrap();
    assert!(json.contains("\"a\""));
}

#[test]
fn core_sha256_hex_accessible() {
    let hex = abp_core::sha256_hex(b"hello");
    assert_eq!(hex.len(), 64);
}

#[test]
fn core_receipt_hash_accessible() {
    use abp_core::{Outcome, ReceiptBuilder};
    let receipt = ReceiptBuilder::new("m").outcome(Outcome::Complete).build();
    let hash = abp_core::receipt_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. SupportLevel::satisfies
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_support_level_satisfies_api() {
    use abp_core::{MinSupport, SupportLevel};
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Type alias re-export
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_capability_manifest_type_alias() {
    use abp_core::{Capability, CapabilityManifest, SupportLevel};
    let mut m = CapabilityManifest::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    assert!(m.contains_key(&Capability::Streaming));
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Error types implement Error + Display
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_contract_error_is_error_and_display() {
    fn assert_error<T: std::error::Error + std::fmt::Display>() {}
    assert_error::<abp_core::ContractError>();
}

#[test]
fn error_abp_error_is_error_and_display() {
    fn assert_error<T: std::error::Error + std::fmt::Display>() {}
    assert_error::<abp_error::AbpError>();
}

#[test]
fn protocol_error_is_error_and_display() {
    fn assert_error<T: std::error::Error + std::fmt::Display>() {}
    assert_error::<abp_protocol::ProtocolError>();
}

#[test]
fn mapping_error_is_error_and_display() {
    fn assert_error<T: std::error::Error + std::fmt::Display>() {}
    assert_error::<abp_mapping::MappingError>();
}

#[test]
fn config_error_is_error_and_display() {
    fn assert_error<T: std::error::Error + std::fmt::Display>() {}
    assert_error::<abp_config::ConfigError>();
}

// ═══════════════════════════════════════════════════════════════════════
// 12. abp-error public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_category_variants() {
    use abp_error::ErrorCategory;
    let cats = [
        ErrorCategory::Protocol,
        ErrorCategory::Backend,
        ErrorCategory::Capability,
        ErrorCategory::Policy,
        ErrorCategory::Workspace,
        ErrorCategory::Ir,
        ErrorCategory::Receipt,
        ErrorCategory::Dialect,
        ErrorCategory::Config,
        ErrorCategory::Internal,
    ];
    assert_eq!(cats.len(), 10);
}

#[test]
fn error_code_variants_and_category() {
    use abp_error::ErrorCode;
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::ProtocolUnexpectedMessage,
        ErrorCode::ProtocolVersionMismatch,
        ErrorCode::BackendNotFound,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
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
    assert_eq!(codes.len(), 20);
    // Every code must have a category
    for code in &codes {
        let _ = code.category();
    }
}

#[test]
fn error_code_as_str_stable() {
    use abp_error::ErrorCode;
    assert_eq!(ErrorCode::BackendTimeout.as_str(), "BACKEND_TIMEOUT");
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.as_str(),
        "PROTOCOL_INVALID_ENVELOPE"
    );
    assert_eq!(ErrorCode::Internal.as_str(), "INTERNAL");
}

#[test]
fn error_code_display_matches_as_str() {
    use abp_error::ErrorCode;
    let code = ErrorCode::PolicyDenied;
    assert_eq!(code.to_string(), code.as_str());
}

#[test]
fn abp_error_builder_pattern() {
    use abp_error::{AbpError, ErrorCode};
    let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
        .with_context("backend", "openai")
        .with_context("timeout_ms", 30_000);
    assert_eq!(err.code, ErrorCode::BackendTimeout);
    assert_eq!(err.context.len(), 2);
    assert_eq!(err.category(), abp_error::ErrorCategory::Backend);
}

#[test]
fn abp_error_dto_fields() {
    use abp_error::{AbpError, AbpErrorDto, ErrorCode};
    let err = AbpError::new(ErrorCode::Internal, "oops");
    let dto: AbpErrorDto = (&err).into();
    assert_eq!(dto.code, ErrorCode::Internal);
    assert_eq!(dto.message, "oops");
}

#[test]
fn error_code_serde_roundtrip() {
    use abp_error::ErrorCode;
    let code = ErrorCode::BackendTimeout;
    let json = serde_json::to_string(&code).unwrap();
    let back: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(code, back);
}

#[test]
fn error_category_traits() {
    assert_clone::<abp_error::ErrorCategory>();
    assert_debug::<abp_error::ErrorCategory>();
    assert_serialize::<abp_error::ErrorCategory>();
    assert_deserialize::<abp_error::ErrorCategory>();
    assert_partial_eq::<abp_error::ErrorCategory>();
}

#[test]
fn error_code_traits() {
    assert_clone::<abp_error::ErrorCode>();
    assert_debug::<abp_error::ErrorCode>();
    assert_serialize::<abp_error::ErrorCode>();
    assert_deserialize::<abp_error::ErrorCode>();
    assert_partial_eq::<abp_error::ErrorCode>();
}

// ═══════════════════════════════════════════════════════════════════════
// 13. abp-protocol public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_envelope_variants() {
    use abp_core::{BackendIdentity, CapabilityManifest};
    use abp_protocol::Envelope;

    // Hello
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    assert!(matches!(hello, Envelope::Hello { .. }));

    // Fatal with code
    let fatal =
        Envelope::fatal_with_code(Some("ref".into()), "boom", abp_error::ErrorCode::Internal);
    assert_eq!(fatal.error_code(), Some(abp_error::ErrorCode::Internal));
}

#[test]
fn protocol_envelope_serde_tag() {
    use abp_core::{BackendIdentity, CapabilityManifest};
    use abp_protocol::{Envelope, JsonlCodec};

    let hello = Envelope::hello(
        BackendIdentity {
            id: "s".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let line = JsonlCodec::encode(&hello).unwrap();
    // Discriminator is "t", not "type"
    assert!(line.contains("\"t\":\"hello\""));
}

#[test]
fn protocol_jsonl_codec_roundtrip() {
    use abp_core::{BackendIdentity, CapabilityManifest};
    use abp_protocol::{Envelope, JsonlCodec};

    let orig = Envelope::hello(
        BackendIdentity {
            id: "rt".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let encoded = JsonlCodec::encode(&orig).unwrap();
    assert!(encoded.ends_with('\n'));
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn protocol_error_variants() {
    use abp_protocol::ProtocolError;
    // Json variant
    let err = abp_protocol::JsonlCodec::decode("not json").unwrap_err();
    assert!(matches!(err, ProtocolError::Json(_)));
}

#[test]
fn protocol_envelope_traits() {
    assert_clone::<abp_protocol::Envelope>();
    assert_debug::<abp_protocol::Envelope>();
    assert_serialize::<abp_protocol::Envelope>();
    assert_deserialize::<abp_protocol::Envelope>();
}

#[test]
fn protocol_jsonl_codec_traits() {
    assert_debug::<abp_protocol::JsonlCodec>();
    assert_clone::<abp_protocol::JsonlCodec>();
}

// ═══════════════════════════════════════════════════════════════════════
// 14. abp-glob public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn glob_match_decision_variants() {
    use abp_glob::MatchDecision;
    assert!(MatchDecision::Allowed.is_allowed());
    assert!(!MatchDecision::DeniedByExclude.is_allowed());
    assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
}

#[test]
fn glob_include_exclude_construction() {
    use abp_glob::IncludeExcludeGlobs;
    let globs = IncludeExcludeGlobs::new(&["*.rs".to_string()], &["test_*".to_string()]).unwrap();
    assert!(globs.decide_str("main.rs").is_allowed());
    assert!(!globs.decide_str("test_foo.rs").is_allowed());
    assert!(!globs.decide_str("readme.md").is_allowed());
}

#[test]
fn glob_build_globset_accessible() {
    let result = abp_glob::build_globset(&[]).unwrap();
    assert!(result.is_none());
    let result = abp_glob::build_globset(&["*.rs".to_string()]).unwrap();
    assert!(result.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 15. abp-policy public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn policy_engine_construction() {
    use abp_core::PolicyProfile;
    use abp_policy::PolicyEngine;
    let policy = PolicyProfile {
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let engine = PolicyEngine::new(&policy).unwrap();
    let decision = engine.can_use_tool("bash");
    assert!(!decision.allowed);
}

#[test]
fn policy_decision_api() {
    use abp_policy::Decision;
    let allow = Decision::allow();
    assert!(allow.allowed);
    assert!(allow.reason.is_none());

    let deny = Decision::deny("forbidden");
    assert!(!deny.allowed);
    assert_eq!(deny.reason.as_deref(), Some("forbidden"));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. abp-dialect public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_enum_variants() {
    use abp_dialect::Dialect;
    let all = Dialect::all();
    assert!(all.contains(&Dialect::OpenAi));
    assert!(all.contains(&Dialect::Claude));
    assert!(all.contains(&Dialect::Gemini));
    assert!(all.contains(&Dialect::Codex));
    assert!(all.contains(&Dialect::Kimi));
    assert!(all.contains(&Dialect::Copilot));
    assert_eq!(all.len(), 6);
}

#[test]
fn dialect_label_stable() {
    use abp_dialect::Dialect;
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
}

#[test]
fn dialect_display_matches_label() {
    use abp_dialect::Dialect;
    assert_eq!(Dialect::OpenAi.to_string(), "OpenAI");
}

#[test]
fn dialect_traits() {
    assert_clone::<abp_dialect::Dialect>();
    assert_debug::<abp_dialect::Dialect>();
    assert_serialize::<abp_dialect::Dialect>();
    assert_deserialize::<abp_dialect::Dialect>();
    assert_partial_eq::<abp_dialect::Dialect>();
}

#[test]
fn dialect_detection_result_fields() {
    use abp_dialect::{DetectionResult, Dialect};
    let r = DetectionResult {
        dialect: Dialect::Claude,
        confidence: 0.9,
        evidence: vec!["has model field".into()],
    };
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn dialect_detector_accessible() {
    use abp_dialect::DialectDetector;
    let d = DialectDetector::new();
    let _ = d.detect(&serde_json::json!({}));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. abp-mapping public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_variants() {
    use abp_dialect::Dialect;
    use abp_mapping::MappingError;
    let _ = MappingError::FeatureUnsupported {
        feature: "f".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let _ = MappingError::FidelityLoss {
        feature: "f".into(),
        warning: "w".into(),
    };
    let _ = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Kimi,
    };
    let _ = MappingError::InvalidInput {
        reason: "bad".into(),
    };
}

#[test]
fn mapping_fidelity_api() {
    use abp_mapping::Fidelity;
    assert!(Fidelity::Lossless.is_lossless());
    assert!(!Fidelity::Lossless.is_unsupported());
    let u = Fidelity::Unsupported { reason: "x".into() };
    assert!(u.is_unsupported());
    assert!(!u.is_lossless());
}

#[test]
fn mapping_registry_accessible() {
    use abp_mapping::MappingRegistry;
    let reg = MappingRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 18. abp-config public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn config_backplane_config_fields() {
    use abp_config::BackplaneConfig;
    let cfg = BackplaneConfig::default();
    assert!(cfg.default_backend.is_none());
    assert!(cfg.workspace_dir.is_none());
    assert_eq!(cfg.log_level.as_deref(), Some("info"));
    assert!(cfg.receipts_dir.is_none());
    assert!(cfg.backends.is_empty());
}

#[test]
fn config_backend_entry_variants() {
    use abp_config::BackendEntry;
    let _ = BackendEntry::Mock {};
    let _ = BackendEntry::Sidecar {
        command: "node".into(),
        args: vec![],
        timeout_secs: Some(30),
    };
}

#[test]
fn config_warning_variants() {
    use abp_config::ConfigWarning;
    let _ = ConfigWarning::DeprecatedField {
        field: "f".into(),
        suggestion: None,
    };
    let _ = ConfigWarning::MissingOptionalField {
        field: "f".into(),
        hint: "h".into(),
    };
    let _ = ConfigWarning::LargeTimeout {
        backend: "b".into(),
        secs: 9999,
    };
}

#[test]
fn config_error_variants() {
    use abp_config::ConfigError;
    let _ = ConfigError::FileNotFound { path: "x".into() };
    let _ = ConfigError::ParseError { reason: "r".into() };
    let _ = ConfigError::ValidationError {
        reasons: vec!["r".into()],
    };
    let _ = ConfigError::MergeConflict { reason: "c".into() };
}

#[test]
fn config_public_functions_accessible() {
    let cfg = abp_config::load_config(None).unwrap();
    assert!(cfg.backends.is_empty());

    let cfg2 = abp_config::parse_toml("").unwrap();
    let _ = abp_config::validate_config(&cfg2);

    let merged = abp_config::merge_configs(cfg, cfg2);
    assert!(merged.backends.is_empty());
}

#[test]
fn config_traits() {
    assert_clone::<abp_config::BackplaneConfig>();
    assert_debug::<abp_config::BackplaneConfig>();
    assert_serialize::<abp_config::BackplaneConfig>();
    assert_deserialize::<abp_config::BackplaneConfig>();
    assert_partial_eq::<abp_config::BackplaneConfig>();
}

// ═══════════════════════════════════════════════════════════════════════
// 19. abp-core IR types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_variants() {
    use abp_core::ir::{IrMessage, IrRole};
    let _ = IrRole::System;
    let _ = IrRole::User;
    let _ = IrRole::Assistant;
    let _ = IrRole::Tool;
    // Construction via helper
    let msg = IrMessage::text(IrRole::User, "hello");
    assert!(msg.is_text_only());
    assert_eq!(msg.text_content(), "hello");
}

#[test]
fn ir_content_block_variants() {
    use abp_core::ir::IrContentBlock;
    let _ = IrContentBlock::Text { text: "t".into() };
    let _ = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "abc".into(),
    };
    let _ = IrContentBlock::ToolUse {
        id: "1".into(),
        name: "bash".into(),
        input: serde_json::json!({}),
    };
    let _ = IrContentBlock::ToolResult {
        tool_use_id: "1".into(),
        content: vec![],
        is_error: false,
    };
    let _ = IrContentBlock::Thinking { text: "hmm".into() };
}

#[test]
fn ir_conversation_api() {
    use abp_core::ir::{IrConversation, IrMessage, IrRole};
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"));
    assert_eq!(conv.len(), 2);
    assert!(!conv.is_empty());
    assert!(conv.system_message().is_some());
}

#[test]
fn ir_tool_definition_fields() {
    use abp_core::ir::IrToolDefinition;
    let td = IrToolDefinition {
        name: "read".into(),
        description: "read a file".into(),
        parameters: serde_json::json!({}),
    };
    assert_eq!(td.name, "read");
}

#[test]
fn ir_usage_construction() {
    use abp_core::ir::IrUsage;
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);

    let d = IrUsage::default();
    assert_eq!(d.total_tokens, 0);
}

#[test]
fn ir_types_traits() {
    assert_clone::<abp_core::ir::IrRole>();
    assert_debug::<abp_core::ir::IrRole>();
    assert_serialize::<abp_core::ir::IrRole>();
    assert_deserialize::<abp_core::ir::IrRole>();
    assert_partial_eq::<abp_core::ir::IrRole>();
    assert_clone::<abp_core::ir::IrMessage>();
    assert_clone::<abp_core::ir::IrContentBlock>();
    assert_clone::<abp_core::ir::IrConversation>();
    assert_clone::<abp_core::ir::IrToolDefinition>();
    assert_clone::<abp_core::ir::IrUsage>();
}

// ═══════════════════════════════════════════════════════════════════════
// 20. JsonSchema derive verification
// ═══════════════════════════════════════════════════════════════════════

fn assert_json_schema<T: schemars::JsonSchema>() {}

#[test]
fn json_schema_derives_on_core_types() {
    assert_json_schema::<abp_core::WorkOrder>();
    assert_json_schema::<abp_core::Receipt>();
    assert_json_schema::<abp_core::AgentEvent>();
    assert_json_schema::<abp_core::AgentEventKind>();
    assert_json_schema::<abp_core::Capability>();
    assert_json_schema::<abp_core::SupportLevel>();
    assert_json_schema::<abp_core::ExecutionMode>();
    assert_json_schema::<abp_core::Outcome>();
    assert_json_schema::<abp_core::RuntimeConfig>();
    assert_json_schema::<abp_core::PolicyProfile>();
    assert_json_schema::<abp_core::UsageNormalized>();
    assert_json_schema::<abp_core::VerificationReport>();
    assert_json_schema::<abp_core::BackendIdentity>();
    assert_json_schema::<abp_core::RunMetadata>();
    assert_json_schema::<abp_core::ArtifactRef>();
    assert_json_schema::<abp_core::ContextPacket>();
    assert_json_schema::<abp_core::ContextSnippet>();
    assert_json_schema::<abp_core::WorkspaceSpec>();
    assert_json_schema::<abp_core::CapabilityRequirement>();
    assert_json_schema::<abp_core::CapabilityRequirements>();
    assert_json_schema::<abp_core::ExecutionLane>();
    assert_json_schema::<abp_core::WorkspaceMode>();
    assert_json_schema::<abp_core::MinSupport>();
}

#[test]
fn json_schema_derives_on_ir_types() {
    assert_json_schema::<abp_core::ir::IrRole>();
    assert_json_schema::<abp_core::ir::IrContentBlock>();
    assert_json_schema::<abp_core::ir::IrMessage>();
    assert_json_schema::<abp_core::ir::IrToolDefinition>();
    assert_json_schema::<abp_core::ir::IrConversation>();
    assert_json_schema::<abp_core::ir::IrUsage>();
}

#[test]
fn json_schema_on_error_code() {
    assert_json_schema::<abp_error::ErrorCode>();
}

#[test]
fn json_schema_on_config_types() {
    assert_json_schema::<abp_config::BackplaneConfig>();
    assert_json_schema::<abp_config::BackendEntry>();
}

// ═══════════════════════════════════════════════════════════════════════
// 21. abp-emulation public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn emulation_strategy_variants() {
    use abp_emulation::EmulationStrategy;
    let _ = EmulationStrategy::SystemPromptInjection { prompt: "p".into() };
    let _ = EmulationStrategy::PostProcessing { detail: "d".into() };
    let _ = EmulationStrategy::Disabled { reason: "r".into() };
}

#[test]
fn emulation_config_and_engine() {
    use abp_emulation::{EmulationConfig, EmulationEngine};
    let config = EmulationConfig::new();
    assert!(config.strategies.is_empty());

    let engine = EmulationEngine::new(config);
    let _ = engine.resolve_strategy(&abp_core::Capability::Streaming);
}

#[test]
fn emulation_public_functions() {
    let _ = abp_emulation::emulate_structured_output();
    let _ = abp_emulation::emulate_code_execution();
    let _ = abp_emulation::emulate_extended_thinking();
    let _ = abp_emulation::emulate_image_input();
    let _ = abp_emulation::emulate_stop_sequences();
    let _ = abp_emulation::default_strategy(&abp_core::Capability::Streaming);
    let _ = abp_emulation::can_emulate(&abp_core::Capability::Streaming);
}

#[test]
fn emulation_report_api() {
    use abp_emulation::EmulationReport;
    let r = EmulationReport::default();
    assert!(r.is_empty());
    assert!(!r.has_unemulatable());
}

#[test]
fn emulation_traits() {
    assert_clone::<abp_emulation::EmulationStrategy>();
    assert_debug::<abp_emulation::EmulationStrategy>();
    assert_serialize::<abp_emulation::EmulationStrategy>();
    assert_deserialize::<abp_emulation::EmulationStrategy>();
    assert_partial_eq::<abp_emulation::EmulationStrategy>();
    assert_clone::<abp_emulation::EmulationConfig>();
    assert_clone::<abp_emulation::EmulationReport>();
}

// ═══════════════════════════════════════════════════════════════════════
// 22. abp-telemetry public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn telemetry_run_metrics_fields() {
    use abp_telemetry::RunMetrics;
    let m = RunMetrics {
        backend_name: "mock".into(),
        dialect: "openai".into(),
        duration_ms: 100,
        events_count: 5,
        tokens_in: 10,
        tokens_out: 20,
        tool_calls_count: 1,
        errors_count: 0,
        emulations_applied: 0,
    };
    assert_eq!(m.backend_name, "mock");
}

#[test]
fn telemetry_collector_api() {
    use abp_telemetry::{MetricsCollector, RunMetrics};
    let c = MetricsCollector::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    c.record(RunMetrics::default());
    assert_eq!(c.len(), 1);
    let s = c.summary();
    assert_eq!(s.count, 1);
    c.clear();
    assert!(c.is_empty());
}

#[test]
fn telemetry_span_api() {
    use abp_telemetry::TelemetrySpan;
    let span = TelemetrySpan::new("test").with_attribute("key", "value");
    assert_eq!(span.name, "test");
    assert_eq!(span.attributes.get("key").unwrap(), "value");
}

#[test]
fn telemetry_traits() {
    assert_clone::<abp_telemetry::RunMetrics>();
    assert_debug::<abp_telemetry::RunMetrics>();
    assert_serialize::<abp_telemetry::RunMetrics>();
    assert_deserialize::<abp_telemetry::RunMetrics>();
    assert_partial_eq::<abp_telemetry::RunMetrics>();
    assert_send::<abp_telemetry::RunMetrics>();
    assert_sync::<abp_telemetry::RunMetrics>();
}

// ═══════════════════════════════════════════════════════════════════════
// 23. abp-receipt public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_crate_re_exports() {
    // abp_receipt re-exports Outcome and Receipt from abp_core
    let _: abp_receipt::Receipt = abp_core::ReceiptBuilder::new("m")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let _ = abp_receipt::Outcome::Complete;
}

#[test]
fn receipt_hash_functions() {
    let receipt = abp_core::ReceiptBuilder::new("m")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let canonical = abp_receipt::canonicalize(&receipt).unwrap();
    assert!(!canonical.is_empty());
    let hash = abp_receipt::compute_hash(&receipt).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn receipt_chain_api() {
    use abp_receipt::ReceiptChain;
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn receipt_diff_api() {
    let a = abp_core::ReceiptBuilder::new("a")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let b = abp_core::ReceiptBuilder::new("b")
        .outcome(abp_core::Outcome::Failed)
        .build();
    let diff = abp_receipt::diff_receipts(&a, &b);
    assert!(!diff.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 24. abp-capability public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_negotiation_functions() {
    use abp_capability::{check_capability, generate_report, negotiate};
    use abp_core::{Capability, CapabilityManifest, CapabilityRequirements, SupportLevel};

    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::Streaming, SupportLevel::Native);

    let level = check_capability(&manifest, &Capability::Streaming);
    assert!(matches!(level, abp_capability::SupportLevel::Native));

    let result = negotiate(&manifest, &CapabilityRequirements::default());
    assert!(result.is_compatible());

    let report = generate_report(&result);
    assert!(report.compatible);
}

#[test]
fn capability_support_level_variants() {
    use abp_capability::SupportLevel;
    let _ = SupportLevel::Native;
    let _ = SupportLevel::Emulated {
        method: "prompt".into(),
    };
    let _ = SupportLevel::Unsupported {
        reason: "unsupported".into(),
    };
}

// ═══════════════════════════════════════════════════════════════════════
// 25. abp-stream public API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_filter_api() {
    use abp_stream::EventFilter;
    let _ = EventFilter::errors_only();
    let _ = EventFilter::exclude_errors();
    let _ = EventFilter::by_kind("tool_call");
}

#[test]
fn stream_event_recorder_api() {
    use abp_stream::EventRecorder;
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    r.clear();
}

#[test]
fn stream_pipeline_builder() {
    use abp_stream::{EventFilter, StreamPipelineBuilder};
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .record()
        .build();
    assert!(pipeline.recorder().is_some());
}

#[test]
fn stream_event_stats_api() {
    use abp_stream::EventStats;
    let s = EventStats::new();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.error_count(), 0);
    s.reset();
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Serde round-trip for core contract types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_work_order() {
    let wo = abp_core::WorkOrderBuilder::new("task").build();
    let json = serde_json::to_string(&wo).unwrap();
    let back: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(back.task, "task");
}

#[test]
fn serde_roundtrip_receipt() {
    let r = abp_core::ReceiptBuilder::new("m")
        .outcome(abp_core::Outcome::Complete)
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let back: abp_core::Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(back.backend.id, "m");
    assert_eq!(back.outcome, abp_core::Outcome::Complete);
}

#[test]
fn serde_roundtrip_agent_event() {
    use abp_core::{AgentEvent, AgentEventKind};
    let ev = AgentEvent {
        ts: chrono::Utc::now(),
        kind: AgentEventKind::Warning {
            message: "w".into(),
        },
        ext: None,
    };
    let json = serde_json::to_string(&ev).unwrap();
    let back: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back.kind, AgentEventKind::Warning { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 27. No unintentional public leakage — private modules stay private
// ═══════════════════════════════════════════════════════════════════════

/// Compile-time check: these modules are public and reachable.
#[test]
fn core_public_modules_accessible() {
    let _ = std::any::type_name::<abp_core::aggregate::EventAggregator>();
    let _ = std::any::type_name::<abp_core::chain::ReceiptChain>();
    let _ = std::any::type_name::<abp_core::config::ConfigValidator>();
    let _ = std::any::type_name::<abp_core::error::ErrorCode>();
    let _ = std::any::type_name::<dyn abp_core::ext::WorkOrderExt>();
    let _ = std::any::type_name::<abp_core::filter::EventFilter>();
    let _ = std::any::type_name::<abp_core::ir::IrRole>();
    let _ = std::any::type_name::<abp_core::negotiate::NegotiationResult>();
    let _ = std::any::type_name::<abp_core::stream::EventStream>();
    let _ = std::any::type_name::<abp_core::validate::ValidationError>();
    let _ = std::any::type_name::<abp_core::verify::ReceiptVerifier>();
}

#[test]
fn protocol_public_modules_accessible() {
    let _ = std::any::type_name::<abp_protocol::batch::BatchRequest>();
    let _ = std::any::type_name::<abp_protocol::builder::HelloBuilder>();
    let _ = std::any::type_name::<abp_protocol::codec::StreamingCodec>();
    let _ = std::any::type_name::<abp_protocol::compress::MessageCompressor>();
    let _ = std::any::type_name::<abp_protocol::router::MessageRouter>();
    let _ = std::any::type_name::<abp_protocol::stream::StreamParser>();
    let _ = std::any::type_name::<abp_protocol::validate::EnvelopeValidator>();
    let _ = std::any::type_name::<abp_protocol::version::ProtocolVersion>();
}

// ═══════════════════════════════════════════════════════════════════════
// 28. Re-exports from expected paths
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn core_error_module_re_exports_error_code() {
    // ErrorCode is available via abp_core::error
    let _: abp_core::error::ErrorCode = abp_core::error::ErrorCode::InvalidContractVersion;
}

// ═══════════════════════════════════════════════════════════════════════
// 29. Extension traits exist and have expected methods
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ext_work_order_ext_methods() {
    use abp_core::ext::WorkOrderExt;
    let wo = abp_core::WorkOrderBuilder::new("code task").build();
    let _ = wo.has_capability(&abp_core::Capability::Streaming);
    let _ = wo.tool_budget_remaining();
    let _ = wo.is_code_task();
    let _ = wo.task_summary(20);
    let _ = wo.required_capabilities();
    let _ = wo.vendor_config("x");
}

#[test]
fn ext_receipt_ext_methods() {
    use abp_core::ext::ReceiptExt;
    let r = abp_core::ReceiptBuilder::new("m")
        .outcome(abp_core::Outcome::Complete)
        .build();
    assert!(r.is_success());
    assert!(!r.is_failure());
    let _ = r.event_count_by_kind();
    let _ = r.tool_calls();
    let _ = r.assistant_messages();
    let _ = r.total_tool_calls();
    let _ = r.has_errors();
    let _ = r.duration_secs();
}

// ═══════════════════════════════════════════════════════════════════════
// 30. Protocol version API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_version_api() {
    use abp_protocol::version::{ProtocolVersion, VersionError, VersionRange};

    let v = ProtocolVersion { major: 0, minor: 1 };
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);

    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 0 },
        max: ProtocolVersion { major: 0, minor: 2 },
    };
    assert!(range.min < range.max);

    // VersionError variants
    let _ = VersionError::InvalidFormat;
    let _ = VersionError::InvalidMajor;
    let _ = VersionError::InvalidMinor;
    let _ = VersionError::Incompatible {
        local: ProtocolVersion { major: 0, minor: 1 },
        remote: ProtocolVersion { major: 1, minor: 0 },
    };
}

#[test]
fn protocol_version_traits() {
    assert_clone::<abp_protocol::version::ProtocolVersion>();
    assert_debug::<abp_protocol::version::ProtocolVersion>();
    assert_serialize::<abp_protocol::version::ProtocolVersion>();
    assert_deserialize::<abp_protocol::version::ProtocolVersion>();
    assert_partial_eq::<abp_protocol::version::ProtocolVersion>();
}

// ═══════════════════════════════════════════════════════════════════════
// 31. Protocol batch API
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn protocol_batch_types() {
    use abp_protocol::batch::{BatchItemStatus, BatchValidationError, MAX_BATCH_SIZE};
    assert_ne!(MAX_BATCH_SIZE, 0);
    let _ = BatchItemStatus::Success;
    let _ = BatchItemStatus::Failed { error: "x".into() };
    let _ = BatchItemStatus::Skipped { reason: "y".into() };
    let _ = BatchValidationError::EmptyBatch;
    let _ = BatchValidationError::TooManyItems {
        count: 999,
        max: MAX_BATCH_SIZE,
    };
}

// ═══════════════════════════════════════════════════════════════════════
// 32. Capability ordering — BTreeMap key stability
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_ord_for_btreemap() {
    use abp_core::{Capability, CapabilityManifest, SupportLevel};
    fn assert_ord<T: Ord>() {}
    assert_ord::<Capability>();

    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    assert_eq!(m.len(), 2);
}

#[test]
fn capability_hash() {
    fn assert_hash<T: std::hash::Hash>() {}
    assert_hash::<abp_core::Capability>();
}
