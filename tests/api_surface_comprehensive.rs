// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive API surface stability tests.
//!
//! These tests verify that public types, constructors, methods, trait
//! implementations, enum variants, constants, and re-exports across all
//! primary crates remain accessible and stable.

// ==========================================================================
// 1. abp-core — public types are accessible (type position)
// ==========================================================================

mod core_types_accessible {
    use abp_core::{
        AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
        CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet,
        ContractError, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
        ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel, UsageNormalized,
        VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    };

    #[test]
    fn work_order_is_nameable() {
        let _: Option<WorkOrder> = None;
    }

    #[test]
    fn receipt_is_nameable() {
        let _: Option<Receipt> = None;
    }

    #[test]
    fn agent_event_is_nameable() {
        let _: Option<AgentEvent> = None;
    }

    #[test]
    fn agent_event_kind_is_nameable() {
        let _: Option<AgentEventKind> = None;
    }

    #[test]
    fn backend_identity_is_nameable() {
        let _: Option<BackendIdentity> = None;
    }

    #[test]
    fn capability_is_nameable() {
        let _: Option<Capability> = None;
    }

    #[test]
    fn capability_manifest_is_nameable() {
        let _: Option<CapabilityManifest> = None;
    }

    #[test]
    fn capability_requirements_is_nameable() {
        let _: Option<CapabilityRequirements> = None;
    }

    #[test]
    fn capability_requirement_is_nameable() {
        let _: Option<CapabilityRequirement> = None;
    }

    #[test]
    fn context_packet_is_nameable() {
        let _: Option<ContextPacket> = None;
    }

    #[test]
    fn context_snippet_is_nameable() {
        let _: Option<ContextSnippet> = None;
    }

    #[test]
    fn execution_lane_is_nameable() {
        let _: Option<ExecutionLane> = None;
    }

    #[test]
    fn execution_mode_is_nameable() {
        let _: Option<ExecutionMode> = None;
    }

    #[test]
    fn min_support_is_nameable() {
        let _: Option<MinSupport> = None;
    }

    #[test]
    fn outcome_is_nameable() {
        let _: Option<Outcome> = None;
    }

    #[test]
    fn policy_profile_is_nameable() {
        let _: Option<PolicyProfile> = None;
    }

    #[test]
    fn run_metadata_is_nameable() {
        let _: Option<RunMetadata> = None;
    }

    #[test]
    fn runtime_config_is_nameable() {
        let _: Option<RuntimeConfig> = None;
    }

    #[test]
    fn support_level_is_nameable() {
        let _: Option<SupportLevel> = None;
    }

    #[test]
    fn usage_normalized_is_nameable() {
        let _: Option<UsageNormalized> = None;
    }

    #[test]
    fn verification_report_is_nameable() {
        let _: Option<VerificationReport> = None;
    }

    #[test]
    fn workspace_mode_is_nameable() {
        let _: Option<WorkspaceMode> = None;
    }

    #[test]
    fn workspace_spec_is_nameable() {
        let _: Option<WorkspaceSpec> = None;
    }

    #[test]
    fn artifact_ref_is_nameable() {
        let _: Option<ArtifactRef> = None;
    }

    #[test]
    fn contract_error_is_nameable() {
        let _: Option<ContractError> = None;
    }

    #[test]
    fn work_order_builder_is_nameable() {
        let _: Option<WorkOrderBuilder> = None;
    }

    #[test]
    fn receipt_builder_is_nameable() {
        let _: Option<ReceiptBuilder> = None;
    }
}

// ==========================================================================
// 2. abp-core — constants
// ==========================================================================

mod core_constants {
    use abp_core::CONTRACT_VERSION;

    #[test]
    fn contract_version_is_accessible() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }
}

// ==========================================================================
// 3. abp-core — constructors and builders
// ==========================================================================

mod core_constructors {
    use abp_core::{ExecutionLane, Outcome, ReceiptBuilder, WorkOrderBuilder, WorkspaceMode};

    #[test]
    fn work_order_builder_new() {
        let wo = WorkOrderBuilder::new("test task").build();
        assert_eq!(wo.task, "test task");
    }

    #[test]
    fn work_order_builder_lane() {
        let wo = WorkOrderBuilder::new("t")
            .lane(ExecutionLane::WorkspaceFirst)
            .build();
        assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    }

    #[test]
    fn work_order_builder_root() {
        let wo = WorkOrderBuilder::new("t").root("/tmp").build();
        assert_eq!(wo.workspace.root, "/tmp");
    }

    #[test]
    fn work_order_builder_workspace_mode() {
        let wo = WorkOrderBuilder::new("t")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    }

    #[test]
    fn work_order_builder_include_exclude() {
        let wo = WorkOrderBuilder::new("t")
            .include(vec!["src/**".into()])
            .exclude(vec!["*.log".into()])
            .build();
        assert_eq!(wo.workspace.include, vec!["src/**"]);
        assert_eq!(wo.workspace.exclude, vec!["*.log"]);
    }

    #[test]
    fn work_order_builder_model() {
        let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn work_order_builder_max_turns() {
        let wo = WorkOrderBuilder::new("t").max_turns(5).build();
        assert_eq!(wo.config.max_turns, Some(5));
    }

    #[test]
    fn work_order_builder_max_budget_usd() {
        let wo = WorkOrderBuilder::new("t").max_budget_usd(1.5).build();
        assert_eq!(wo.config.max_budget_usd, Some(1.5));
    }

    #[test]
    fn receipt_builder_new() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.backend.id, "mock");
    }

    #[test]
    fn receipt_builder_outcome() {
        let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        assert_eq!(r.outcome, Outcome::Failed);
    }

    #[test]
    fn receipt_builder_with_hash() {
        let r = ReceiptBuilder::new("mock").with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_with_hash_method() {
        let r = ReceiptBuilder::new("mock").build().with_hash().unwrap();
        assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
    }
}

// ==========================================================================
// 4. abp-core — trait implementations (Clone, Debug, Serialize, Deserialize)
// ==========================================================================

mod core_traits {
    use abp_core::{ExecutionMode, Outcome, ReceiptBuilder, WorkOrderBuilder};

    #[test]
    fn work_order_clone_debug() {
        let wo = WorkOrderBuilder::new("t").build();
        let cloned = wo.clone();
        let _ = format!("{:?}", cloned);
    }

    #[test]
    fn work_order_serde_roundtrip() {
        let wo = WorkOrderBuilder::new("serde test").build();
        let json = serde_json::to_string(&wo).unwrap();
        let back: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task, "serde test");
    }

    #[test]
    fn receipt_clone_debug() {
        let r = ReceiptBuilder::new("mock").build();
        let cloned = r.clone();
        let _ = format!("{:?}", cloned);
    }

    #[test]
    fn receipt_serde_roundtrip() {
        let r = ReceiptBuilder::new("mock").build();
        let json = serde_json::to_string(&r).unwrap();
        let back: abp_core::Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(back.backend.id, "mock");
    }

    #[test]
    fn execution_mode_default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn outcome_partial_eq() {
        assert_eq!(Outcome::Complete, Outcome::Complete);
        assert_ne!(Outcome::Complete, Outcome::Failed);
    }

    #[test]
    fn execution_mode_clone_copy() {
        let m = ExecutionMode::Passthrough;
        let c = m;
        assert_eq!(m, c);
    }
}

// ==========================================================================
// 5. abp-core — enum variant exhaustiveness
// ==========================================================================

mod core_enum_variants {
    use abp_core::{
        Capability, ExecutionLane, ExecutionMode, MinSupport, Outcome, SupportLevel, WorkspaceMode,
    };

    #[test]
    fn execution_lane_exhaustive() {
        let lanes = [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst];
        for lane in &lanes {
            match lane {
                ExecutionLane::PatchFirst => {}
                ExecutionLane::WorkspaceFirst => {}
            }
        }
    }

    #[test]
    fn workspace_mode_exhaustive() {
        let modes = [WorkspaceMode::PassThrough, WorkspaceMode::Staged];
        for mode in &modes {
            match mode {
                WorkspaceMode::PassThrough => {}
                WorkspaceMode::Staged => {}
            }
        }
    }

    #[test]
    fn execution_mode_exhaustive() {
        let modes = [ExecutionMode::Passthrough, ExecutionMode::Mapped];
        for mode in &modes {
            match mode {
                ExecutionMode::Passthrough => {}
                ExecutionMode::Mapped => {}
            }
        }
    }

    #[test]
    fn outcome_exhaustive() {
        let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
        for o in &outcomes {
            match o {
                Outcome::Complete => {}
                Outcome::Partial => {}
                Outcome::Failed => {}
            }
        }
    }

    #[test]
    fn min_support_exhaustive() {
        let ms = [MinSupport::Native, MinSupport::Emulated];
        for m in &ms {
            match m {
                MinSupport::Native => {}
                MinSupport::Emulated => {}
            }
        }
    }

    #[test]
    fn support_level_exhaustive() {
        let levels = [
            SupportLevel::Native,
            SupportLevel::Emulated,
            SupportLevel::Unsupported,
            SupportLevel::Restricted { reason: "r".into() },
        ];
        for l in &levels {
            match l {
                SupportLevel::Native => {}
                SupportLevel::Emulated => {}
                SupportLevel::Unsupported => {}
                SupportLevel::Restricted { reason: _ } => {}
            }
        }
    }

    #[test]
    fn capability_exhaustive() {
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
        for c in &caps {
            match c {
                Capability::Streaming => {}
                Capability::ToolRead => {}
                Capability::ToolWrite => {}
                Capability::ToolEdit => {}
                Capability::ToolBash => {}
                Capability::ToolGlob => {}
                Capability::ToolGrep => {}
                Capability::ToolWebSearch => {}
                Capability::ToolWebFetch => {}
                Capability::ToolAskUser => {}
                Capability::HooksPreToolUse => {}
                Capability::HooksPostToolUse => {}
                Capability::SessionResume => {}
                Capability::SessionFork => {}
                Capability::Checkpointing => {}
                Capability::StructuredOutputJsonSchema => {}
                Capability::McpClient => {}
                Capability::McpServer => {}
                Capability::ToolUse => {}
                Capability::ExtendedThinking => {}
                Capability::ImageInput => {}
                Capability::PdfInput => {}
                Capability::CodeExecution => {}
                Capability::Logprobs => {}
                Capability::SeedDeterminism => {}
                Capability::StopSequences => {}
            }
        }
    }
}

// ==========================================================================
// 6. abp-core — public free functions
// ==========================================================================

mod core_functions {
    use abp_core::{Outcome, ReceiptBuilder, canonical_json, receipt_hash, sha256_hex};

    #[test]
    fn canonical_json_works() {
        let json = canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
        assert!(json.starts_with(r#"{"a":1"#));
    }

    #[test]
    fn sha256_hex_works() {
        let hex = sha256_hex(b"hello");
        assert_eq!(hex.len(), 64);
    }

    #[test]
    fn receipt_hash_works() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let h = receipt_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn receipt_hash_is_deterministic() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }
}

// ==========================================================================
// 7. abp-core — SupportLevel::satisfies
// ==========================================================================

mod core_support_level {
    use abp_core::{MinSupport, SupportLevel};

    #[test]
    fn native_satisfies_native() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_satisfies_emulated() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn emulated_does_not_satisfy_native() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn emulated_satisfies_emulated() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn unsupported_satisfies_nothing() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn restricted_satisfies_emulated() {
        let r = SupportLevel::Restricted {
            reason: "sandbox".into(),
        };
        assert!(r.satisfies(&MinSupport::Emulated));
        assert!(!r.satisfies(&MinSupport::Native));
    }
}

// ==========================================================================
// 8. abp-core — error type implements std::error::Error
// ==========================================================================

mod core_errors {
    use abp_core::ContractError;

    #[test]
    fn contract_error_is_std_error() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<ContractError>();
    }

    #[test]
    fn contract_error_display() {
        let err =
            ContractError::Json(serde_json::from_str::<serde_json::Value>("invalid").unwrap_err());
        let msg = format!("{err}");
        assert!(!msg.is_empty());
    }
}

// ==========================================================================
// 9. abp-core — AgentEventKind variant exhaustiveness
// ==========================================================================

mod core_agent_event_kind {
    use abp_core::AgentEventKind;

    #[test]
    fn agent_event_kind_exhaustive() {
        let kinds = [
            AgentEventKind::RunStarted {
                message: "s".into(),
            },
            AgentEventKind::RunCompleted {
                message: "c".into(),
            },
            AgentEventKind::AssistantDelta { text: "d".into() },
            AgentEventKind::AssistantMessage { text: "m".into() },
            AgentEventKind::ToolCall {
                tool_name: "Read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            AgentEventKind::ToolResult {
                tool_name: "Read".into(),
                tool_use_id: None,
                output: serde_json::json!({}),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "a.rs".into(),
                summary: "created".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "ls".into(),
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
        for k in &kinds {
            match k {
                AgentEventKind::RunStarted { .. } => {}
                AgentEventKind::RunCompleted { .. } => {}
                AgentEventKind::AssistantDelta { .. } => {}
                AgentEventKind::AssistantMessage { .. } => {}
                AgentEventKind::ToolCall { .. } => {}
                AgentEventKind::ToolResult { .. } => {}
                AgentEventKind::FileChanged { .. } => {}
                AgentEventKind::CommandExecuted { .. } => {}
                AgentEventKind::Warning { .. } => {}
                AgentEventKind::Error { .. } => {}
            }
        }
    }
}

// ==========================================================================
// 10. abp-core — module re-exports
// ==========================================================================

mod core_modules {
    #[test]
    fn aggregate_module_accessible() {
        let _exists = std::any::type_name::<abp_core::aggregate::EventAggregator>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn chain_module_accessible() {
        let _exists = std::any::type_name::<abp_core::chain::ChainError>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn config_module_accessible() {
        let _exists = std::any::type_name::<abp_core::config::WarningSeverity>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn filter_module_accessible() {
        let _exists = std::any::type_name::<abp_core::filter::EventFilter>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn ir_module_accessible() {
        let _exists = std::any::type_name::<abp_core::ir::IrRole>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn validate_module_accessible() {
        let _exists = std::any::type_name::<abp_core::validate::ValidationError>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn verify_module_accessible() {
        let _exists = std::any::type_name::<abp_core::verify::VerificationCheck>();
        assert!(!_exists.is_empty());
    }
}

// ==========================================================================
// 11. abp-protocol — public types accessible
// ==========================================================================

mod protocol_types {
    use abp_protocol::{Envelope, JsonlCodec, ProtocolError};

    #[test]
    fn envelope_is_nameable() {
        let _: Option<Envelope> = None;
    }

    #[test]
    fn jsonl_codec_is_nameable() {
        let _: Option<JsonlCodec> = None;
    }

    #[test]
    fn protocol_error_is_nameable() {
        let _: Option<ProtocolError> = None;
    }
}

// ==========================================================================
// 12. abp-protocol — constructors and methods
// ==========================================================================

mod protocol_constructors {
    use abp_core::{BackendIdentity, CapabilityManifest, ExecutionMode};
    use abp_protocol::{Envelope, JsonlCodec};

    #[test]
    fn envelope_hello() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        assert!(matches!(env, Envelope::Hello { .. }));
    }

    #[test]
    fn envelope_hello_with_mode() {
        let env = Envelope::hello_with_mode(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        assert!(matches!(env, Envelope::Hello { .. }));
    }

    #[test]
    fn envelope_fatal_with_code() {
        let env = Envelope::fatal_with_code(
            Some("run-1".into()),
            "boom",
            abp_error::ErrorCode::ProtocolInvalidEnvelope,
        );
        assert!(env.error_code().is_some());
    }

    #[test]
    fn envelope_error_code_on_non_fatal_is_none() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "x".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        assert!(env.error_code().is_none());
    }

    #[test]
    fn jsonl_codec_encode_decode_roundtrip() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "test".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.ends_with('\n'));
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Fatal { .. }));
    }

    #[test]
    fn jsonl_codec_encode_to_writer() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "test".into(),
            error_code: None,
        };
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        assert!(!buf.is_empty());
    }

    #[test]
    fn jsonl_codec_encode_many_to_writer() {
        let envs = vec![
            Envelope::Fatal {
                ref_id: None,
                error: "a".into(),
                error_code: None,
            },
            Envelope::Fatal {
                ref_id: None,
                error: "b".into(),
                error_code: None,
            },
        ];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn jsonl_codec_decode_stream() {
        use std::io::BufReader;
        let input = r#"{"t":"fatal","ref_id":null,"error":"x"}
{"t":"fatal","ref_id":null,"error":"y"}
"#;
        let reader = BufReader::new(input.as_bytes());
        let envs: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envs.len(), 2);
    }
}

// ==========================================================================
// 13. abp-protocol — Envelope variant exhaustiveness
// ==========================================================================

mod protocol_enum_variants {
    use abp_protocol::Envelope;

    #[test]
    fn envelope_exhaustive() {
        let fatal = Envelope::Fatal {
            ref_id: None,
            error: "e".into(),
            error_code: None,
        };
        match &fatal {
            Envelope::Hello { .. } => {}
            Envelope::Run { .. } => {}
            Envelope::Event { .. } => {}
            Envelope::Final { .. } => {}
            Envelope::Fatal { .. } => {}
        }
    }
}

// ==========================================================================
// 14. abp-protocol — ProtocolError variant exhaustiveness and std::error::Error
// ==========================================================================

mod protocol_errors {
    use abp_protocol::ProtocolError;

    #[test]
    fn protocol_error_is_std_error() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<ProtocolError>();
    }

    #[test]
    fn protocol_error_json_variant() {
        let err = abp_protocol::JsonlCodec::decode("not json").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn protocol_error_violation_variant() {
        let err = ProtocolError::Violation("test".into());
        assert!(err.to_string().contains("test"));
        assert!(err.error_code().is_some());
    }

    #[test]
    fn protocol_error_unexpected_message_variant() {
        let err = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "fatal".into(),
        };
        assert!(err.to_string().contains("hello"));
        assert!(err.error_code().is_some());
    }

    #[test]
    fn protocol_error_error_code_method() {
        let err = ProtocolError::Violation("v".into());
        assert!(err.error_code().is_some());
    }
}

// ==========================================================================
// 15. abp-protocol — free functions
// ==========================================================================

mod protocol_functions {
    use abp_protocol::{is_compatible_version, parse_version};

    #[test]
    fn parse_version_valid() {
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    }

    #[test]
    fn parse_version_invalid() {
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version("abp/v"), None);
    }

    #[test]
    fn is_compatible_version_same_major() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    }

    #[test]
    fn is_compatible_version_different_major() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    }
}

// ==========================================================================
// 16. abp-protocol — trait implementations
// ==========================================================================

mod protocol_traits {
    use abp_protocol::{Envelope, JsonlCodec};

    #[test]
    fn envelope_clone_debug_serde() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "e".into(),
            error_code: None,
        };
        let cloned = env.clone();
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&cloned).unwrap();
        let _: Envelope = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn jsonl_codec_debug_clone_copy() {
        let c = JsonlCodec;
        let c2 = c;
        let _ = format!("{:?}", c2);
    }
}

// ==========================================================================
// 17. abp-glob — public types accessible
// ==========================================================================

mod glob_types {
    use abp_glob::{IncludeExcludeGlobs, MatchDecision};

    #[test]
    fn include_exclude_globs_is_nameable() {
        let _: Option<IncludeExcludeGlobs> = None;
    }

    #[test]
    fn match_decision_is_nameable() {
        let _: Option<MatchDecision> = None;
    }
}

// ==========================================================================
// 18. abp-glob — constructors and methods
// ==========================================================================

mod glob_constructors {
    use abp_glob::{IncludeExcludeGlobs, MatchDecision, build_globset};
    use std::path::Path;

    #[test]
    fn new_empty() {
        let g = IncludeExcludeGlobs::new(&[], &[]).unwrap();
        assert_eq!(g.decide_str("anything"), MatchDecision::Allowed);
    }

    #[test]
    fn decide_str_include() {
        let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
        assert_eq!(g.decide_str("src/lib.rs"), MatchDecision::Allowed);
        assert_eq!(
            g.decide_str("README.md"),
            MatchDecision::DeniedByMissingInclude
        );
    }

    #[test]
    fn decide_str_exclude() {
        let g = IncludeExcludeGlobs::new(&[], &["*.log".into()]).unwrap();
        assert_eq!(g.decide_str("app.log"), MatchDecision::DeniedByExclude);
        assert_eq!(g.decide_str("main.rs"), MatchDecision::Allowed);
    }

    #[test]
    fn decide_path_works() {
        let g = IncludeExcludeGlobs::new(&["src/**".into()], &[]).unwrap();
        assert_eq!(
            g.decide_path(Path::new("src/main.rs")),
            MatchDecision::Allowed
        );
    }

    #[test]
    fn build_globset_empty_returns_none() {
        assert!(build_globset(&[]).unwrap().is_none());
    }

    #[test]
    fn build_globset_non_empty_returns_some() {
        let set = build_globset(&["*.rs".into()]).unwrap();
        assert!(set.is_some());
    }
}

// ==========================================================================
// 19. abp-glob — enum variant exhaustiveness and traits
// ==========================================================================

mod glob_enum_variants {
    use abp_glob::MatchDecision;

    #[test]
    fn match_decision_exhaustive() {
        let decisions = [
            MatchDecision::Allowed,
            MatchDecision::DeniedByExclude,
            MatchDecision::DeniedByMissingInclude,
        ];
        for d in &decisions {
            match d {
                MatchDecision::Allowed => {}
                MatchDecision::DeniedByExclude => {}
                MatchDecision::DeniedByMissingInclude => {}
            }
        }
    }

    #[test]
    fn match_decision_is_allowed() {
        assert!(MatchDecision::Allowed.is_allowed());
        assert!(!MatchDecision::DeniedByExclude.is_allowed());
        assert!(!MatchDecision::DeniedByMissingInclude.is_allowed());
    }

    #[test]
    fn match_decision_partial_eq() {
        assert_eq!(MatchDecision::Allowed, MatchDecision::Allowed);
        assert_ne!(MatchDecision::Allowed, MatchDecision::DeniedByExclude);
    }

    #[test]
    fn match_decision_clone_copy_debug() {
        let d = MatchDecision::Allowed;
        let d2 = d;
        let _ = format!("{:?}", d2);
    }
}

// ==========================================================================
// 20. abp-policy — public types accessible
// ==========================================================================

mod policy_types {
    use abp_policy::{Decision, PolicyEngine};

    #[test]
    fn decision_is_nameable() {
        let _: Option<Decision> = None;
    }

    #[test]
    fn policy_engine_is_nameable() {
        let _: Option<PolicyEngine> = None;
    }
}

// ==========================================================================
// 21. abp-policy — constructors and methods
// ==========================================================================

mod policy_constructors {
    use abp_core::PolicyProfile;
    use abp_policy::{Decision, PolicyEngine};
    use std::path::Path;

    #[test]
    fn decision_allow() {
        let d = Decision::allow();
        assert!(d.allowed);
        assert!(d.reason.is_none());
    }

    #[test]
    fn decision_deny() {
        let d = Decision::deny("nope");
        assert!(!d.allowed);
        assert_eq!(d.reason.as_deref(), Some("nope"));
    }

    #[test]
    fn policy_engine_new_default_policy() {
        let engine = PolicyEngine::new(&PolicyProfile::default()).unwrap();
        assert!(engine.can_use_tool("Bash").allowed);
    }

    #[test]
    fn policy_engine_can_use_tool() {
        let policy = PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_use_tool("Bash").allowed);
        assert!(engine.can_use_tool("Read").allowed);
    }

    #[test]
    fn policy_engine_can_read_path() {
        let policy = PolicyProfile {
            deny_read: vec!["secret*".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_read_path(Path::new("secret.txt")).allowed);
        assert!(engine.can_read_path(Path::new("public.txt")).allowed);
    }

    #[test]
    fn policy_engine_can_write_path() {
        let policy = PolicyProfile {
            deny_write: vec!["locked*".into()],
            ..PolicyProfile::default()
        };
        let engine = PolicyEngine::new(&policy).unwrap();
        assert!(!engine.can_write_path(Path::new("locked.md")).allowed);
        assert!(engine.can_write_path(Path::new("open.md")).allowed);
    }
}

// ==========================================================================
// 22. abp-policy — trait implementations
// ==========================================================================

mod policy_traits {
    use abp_policy::Decision;

    #[test]
    fn decision_clone_debug_serde() {
        let d = Decision::deny("test");
        let cloned = d.clone();
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&cloned).unwrap();
        let back: Decision = serde_json::from_str(&json).unwrap();
        assert!(!back.allowed);
    }

    #[test]
    fn policy_engine_clone_debug() {
        let engine = abp_policy::PolicyEngine::new(&abp_core::PolicyProfile::default()).unwrap();
        let cloned = engine.clone();
        let _ = format!("{:?}", cloned);
    }
}

// ==========================================================================
// 23. abp-dialect — public types accessible
// ==========================================================================

mod dialect_types {
    use abp_dialect::{
        DetectionResult, Dialect, DialectDetector, DialectValidator, ValidationError,
        ValidationResult,
    };

    #[test]
    fn dialect_is_nameable() {
        let _: Option<Dialect> = None;
    }

    #[test]
    fn detection_result_is_nameable() {
        let _: Option<DetectionResult> = None;
    }

    #[test]
    fn dialect_detector_is_nameable() {
        let _: Option<DialectDetector> = None;
    }

    #[test]
    fn dialect_validator_is_nameable() {
        let _: Option<DialectValidator> = None;
    }

    #[test]
    fn validation_error_is_nameable() {
        let _: Option<ValidationError> = None;
    }

    #[test]
    fn validation_result_is_nameable() {
        let _: Option<ValidationResult> = None;
    }
}

// ==========================================================================
// 24. abp-dialect — enum variant exhaustiveness
// ==========================================================================

mod dialect_enum_variants {
    use abp_dialect::Dialect;

    #[test]
    fn dialect_exhaustive() {
        let dialects = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Kimi,
            Dialect::Copilot,
        ];
        for d in &dialects {
            match d {
                Dialect::OpenAi => {}
                Dialect::Claude => {}
                Dialect::Gemini => {}
                Dialect::Codex => {}
                Dialect::Kimi => {}
                Dialect::Copilot => {}
            }
        }
    }
}

// ==========================================================================
// 25. abp-dialect — constructors and methods
// ==========================================================================

mod dialect_constructors {
    use abp_dialect::{Dialect, DialectDetector, DialectValidator};

    #[test]
    fn dialect_label() {
        assert_eq!(Dialect::OpenAi.label(), "OpenAI");
        assert_eq!(Dialect::Claude.label(), "Claude");
        assert_eq!(Dialect::Gemini.label(), "Gemini");
        assert_eq!(Dialect::Codex.label(), "Codex");
        assert_eq!(Dialect::Kimi.label(), "Kimi");
        assert_eq!(Dialect::Copilot.label(), "Copilot");
    }

    #[test]
    fn dialect_all() {
        let all = Dialect::all();
        assert_eq!(all.len(), 6);
    }

    #[test]
    fn dialect_display() {
        let s = format!("{}", Dialect::OpenAi);
        assert_eq!(s, "OpenAI");
    }

    #[test]
    fn dialect_detector_new() {
        let d = DialectDetector::new();
        let _ = format!("{:?}", d);
    }

    #[test]
    fn dialect_detector_detect_openai() {
        let detector = DialectDetector::new();
        let val = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}],
            "choices": []
        });
        let result = detector.detect(&val);
        assert!(result.is_some());
    }

    #[test]
    fn dialect_detector_detect_none_for_non_object() {
        let detector = DialectDetector::new();
        assert!(detector.detect(&serde_json::json!("string")).is_none());
    }

    #[test]
    fn dialect_detector_detect_all() {
        let detector = DialectDetector::new();
        let val =
            serde_json::json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let results = detector.detect_all(&val);
        assert!(!results.is_empty());
    }

    #[test]
    fn dialect_validator_new() {
        let v = DialectValidator::new();
        let _ = format!("{:?}", v);
    }

    #[test]
    fn dialect_validator_validate() {
        let v = DialectValidator::new();
        let val =
            serde_json::json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
        let result = v.validate(&val, abp_dialect::Dialect::OpenAi);
        assert!(result.valid);
    }

    #[test]
    fn dialect_validator_validate_invalid() {
        let v = DialectValidator::new();
        let result = v.validate(
            &serde_json::json!("not an object"),
            abp_dialect::Dialect::OpenAi,
        );
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }
}

// ==========================================================================
// 26. abp-dialect — trait implementations
// ==========================================================================

mod dialect_traits {
    use abp_dialect::{Dialect, ValidationError};

    #[test]
    fn dialect_clone_copy_debug_hash_eq_serde() {
        let d = Dialect::OpenAi;
        let d2 = d;
        assert_eq!(d, d2);
        let _ = format!("{:?}", d2);
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn dialect_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Dialect::OpenAi);
        set.insert(Dialect::Claude);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn validation_error_is_std_error() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<ValidationError>();
    }

    #[test]
    fn validation_error_display() {
        let e = ValidationError {
            path: "/model".into(),
            message: "missing".into(),
        };
        let s = format!("{e}");
        assert!(s.contains("/model"));
        assert!(s.contains("missing"));
    }
}

// ==========================================================================
// 27. abp-mapping — public types accessible
// ==========================================================================

mod mapping_types {
    use abp_mapping::{
        Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    };

    #[test]
    fn fidelity_is_nameable() {
        let _: Option<Fidelity> = None;
    }

    #[test]
    fn mapping_error_is_nameable() {
        let _: Option<MappingError> = None;
    }

    #[test]
    fn mapping_registry_is_nameable() {
        let _: Option<MappingRegistry> = None;
    }

    #[test]
    fn mapping_rule_is_nameable() {
        let _: Option<MappingRule> = None;
    }

    #[test]
    fn mapping_matrix_is_nameable() {
        let _: Option<MappingMatrix> = None;
    }

    #[test]
    fn mapping_validation_is_nameable() {
        let _: Option<MappingValidation> = None;
    }
}

// ==========================================================================
// 28. abp-mapping — constructors and methods
// ==========================================================================

mod mapping_constructors {
    use abp_dialect::Dialect;
    use abp_mapping::{
        Fidelity, MappingMatrix, MappingRegistry, MappingRule, known_rules, validate_mapping,
    };

    #[test]
    fn mapping_registry_new() {
        let reg = MappingRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn mapping_registry_insert_and_lookup() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
        assert!(rule.is_some());
    }

    #[test]
    fn mapping_registry_iter() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "f".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.iter().count(), 1);
    }

    #[test]
    fn mapping_registry_rank_targets() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
        assert!(!ranked.is_empty());
    }

    #[test]
    fn mapping_matrix_new() {
        let m = MappingMatrix::new();
        assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn mapping_matrix_set_get() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(true));
        assert_eq!(m.get(Dialect::Claude, Dialect::OpenAi), None);
    }

    #[test]
    fn mapping_matrix_from_registry() {
        let reg = known_rules();
        let matrix = MappingMatrix::from_registry(&reg);
        assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn known_rules_non_empty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn validate_mapping_returns_results() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["tool_use".into(), "streaming".into()],
        );
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn fidelity_is_lossless() {
        assert!(Fidelity::Lossless.is_lossless());
        assert!(!Fidelity::Unsupported { reason: "x".into() }.is_lossless());
    }

    #[test]
    fn fidelity_is_unsupported() {
        assert!(Fidelity::Unsupported { reason: "x".into() }.is_unsupported());
        assert!(!Fidelity::Lossless.is_unsupported());
    }
}

// ==========================================================================
// 29. abp-mapping — enum variant exhaustiveness
// ==========================================================================

mod mapping_enum_variants {
    use abp_dialect::Dialect;
    use abp_mapping::{Fidelity, MappingError};

    #[test]
    fn fidelity_exhaustive() {
        let variants = [
            Fidelity::Lossless,
            Fidelity::LossyLabeled {
                warning: "w".into(),
            },
            Fidelity::Unsupported { reason: "r".into() },
        ];
        for v in &variants {
            match v {
                Fidelity::Lossless => {}
                Fidelity::LossyLabeled { .. } => {}
                Fidelity::Unsupported { .. } => {}
            }
        }
    }

    #[test]
    fn mapping_error_exhaustive() {
        let errs = [
            MappingError::FeatureUnsupported {
                feature: "f".into(),
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MappingError::FidelityLoss {
                feature: "f".into(),
                warning: "w".into(),
            },
            MappingError::DialectMismatch {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MappingError::InvalidInput { reason: "r".into() },
        ];
        for e in &errs {
            match e {
                MappingError::FeatureUnsupported { .. } => {}
                MappingError::FidelityLoss { .. } => {}
                MappingError::DialectMismatch { .. } => {}
                MappingError::InvalidInput { .. } => {}
            }
        }
    }
}

// ==========================================================================
// 30. abp-mapping — error type implements std::error::Error
// ==========================================================================

mod mapping_errors {
    use abp_mapping::MappingError;

    #[test]
    fn mapping_error_is_std_error() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<MappingError>();
    }

    #[test]
    fn mapping_error_display() {
        let err = abp_mapping::MappingError::InvalidInput {
            reason: "bad".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("bad"));
    }
}

// ==========================================================================
// 31. abp-mapping — trait implementations
// ==========================================================================

mod mapping_traits {
    use abp_dialect::Dialect;
    use abp_mapping::{Fidelity, MappingError, MappingRegistry, MappingRule};

    #[test]
    fn fidelity_clone_debug_eq_serde() {
        let f = Fidelity::Lossless;
        let cloned = f.clone();
        assert_eq!(f, cloned);
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&f).unwrap();
        let back: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn mapping_error_clone_debug_eq_serde() {
        let e = MappingError::InvalidInput { reason: "x".into() };
        let cloned = e.clone();
        assert_eq!(e, cloned);
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&e).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn mapping_rule_clone_debug_eq_serde() {
        let r = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "f".into(),
            fidelity: Fidelity::Lossless,
        };
        let cloned = r.clone();
        assert_eq!(r, cloned);
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&r).unwrap();
        let back: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn mapping_registry_clone_debug_default() {
        let reg = MappingRegistry::default();
        let cloned = reg.clone();
        let _ = format!("{:?}", cloned);
    }

    #[test]
    fn mapping_matrix_clone_debug_default() {
        let m = abp_mapping::MappingMatrix::default();
        let cloned = m.clone();
        let _ = format!("{:?}", cloned);
    }
}

// ==========================================================================
// 32. abp-mapping — features module constants
// ==========================================================================

mod mapping_features {
    use abp_mapping::features;

    #[test]
    fn feature_constants_accessible() {
        assert_eq!(features::TOOL_USE, "tool_use");
        assert_eq!(features::STREAMING, "streaming");
        assert_eq!(features::THINKING, "thinking");
        assert_eq!(features::IMAGE_INPUT, "image_input");
        assert_eq!(features::CODE_EXEC, "code_exec");
    }
}

// ==========================================================================
// 33. abp-capability — public types accessible
// ==========================================================================

mod capability_types {
    use abp_capability::{CompatibilityReport, NegotiationResult, SupportLevel};

    #[test]
    fn support_level_is_nameable() {
        let _: Option<SupportLevel> = None;
    }

    #[test]
    fn negotiation_result_is_nameable() {
        let _: Option<NegotiationResult> = None;
    }

    #[test]
    fn compatibility_report_is_nameable() {
        let _: Option<CompatibilityReport> = None;
    }
}

// ==========================================================================
// 34. abp-capability — constructors and methods
// ==========================================================================

mod capability_constructors {
    use abp_capability::{
        NegotiationResult, SupportLevel, check_capability, generate_report, negotiate,
    };
    use abp_core::{
        Capability, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, MinSupport,
        SupportLevel as CoreSupportLevel,
    };

    #[test]
    fn check_capability_native() {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, CoreSupportLevel::Native);
        assert_eq!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Native
        );
    }

    #[test]
    fn check_capability_missing() {
        let m = CapabilityManifest::new();
        assert_eq!(
            check_capability(&m, &Capability::Streaming),
            SupportLevel::Unsupported
        );
    }

    #[test]
    fn negotiate_returns_result() {
        let mut m = CapabilityManifest::new();
        m.insert(Capability::Streaming, CoreSupportLevel::Native);
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let result = negotiate(&m, &reqs);
        assert!(result.is_compatible());
        assert_eq!(result.total(), 1);
    }

    #[test]
    fn negotiation_result_is_compatible() {
        let r = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        assert!(r.is_compatible());
    }

    #[test]
    fn negotiation_result_total() {
        let r = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![Capability::ToolRead],
            unsupported: vec![Capability::Logprobs],
        };
        assert_eq!(r.total(), 3);
    }

    #[test]
    fn generate_report_compatible() {
        let r = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&r);
        assert!(report.compatible);
        assert_eq!(report.native_count, 1);
        assert!(report.summary.contains("fully compatible"));
    }

    #[test]
    fn generate_report_incompatible() {
        let r = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![Capability::Streaming],
        };
        let report = generate_report(&r);
        assert!(!report.compatible);
        assert!(report.summary.contains("incompatible"));
    }
}

// ==========================================================================
// 35. abp-capability — enum variant exhaustiveness
// ==========================================================================

mod capability_enum_variants {
    use abp_capability::SupportLevel;

    #[test]
    fn support_level_exhaustive() {
        let levels = [
            SupportLevel::Native,
            SupportLevel::Emulated {
                strategy: "s".into(),
            },
            SupportLevel::Unsupported,
        ];
        for l in &levels {
            match l {
                SupportLevel::Native => {}
                SupportLevel::Emulated { .. } => {}
                SupportLevel::Unsupported => {}
            }
        }
    }
}

// ==========================================================================
// 36. abp-capability — trait implementations
// ==========================================================================

mod capability_traits {
    use abp_capability::{NegotiationResult, SupportLevel, generate_report};
    use abp_core::Capability;

    #[test]
    fn support_level_clone_debug_eq_serde() {
        let l = SupportLevel::Native;
        let cloned = l.clone();
        assert_eq!(l, cloned);
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&l).unwrap();
        let back: SupportLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, l);
    }

    #[test]
    fn negotiation_result_clone_debug_eq_serde() {
        let r = NegotiationResult {
            native: vec![Capability::Streaming],
            emulated: vec![],
            unsupported: vec![],
        };
        let cloned = r.clone();
        assert_eq!(r, cloned);
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&r).unwrap();
        let back: NegotiationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn compatibility_report_clone_debug_eq_serde() {
        let r = NegotiationResult {
            native: vec![],
            emulated: vec![],
            unsupported: vec![],
        };
        let report = generate_report(&r);
        let cloned = report.clone();
        assert_eq!(report, cloned);
        let _ = format!("{:?}", cloned);
        let json = serde_json::to_string(&report).unwrap();
        let back: abp_capability::CompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
    }
}

// ==========================================================================
// 37. abp-policy — module re-exports
// ==========================================================================

mod policy_modules {
    #[test]
    fn audit_module_accessible() {
        let _exists = std::any::type_name::<abp_policy::audit::PolicyDecision>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn compose_module_accessible() {
        let _exists = std::any::type_name::<abp_policy::compose::PolicyDecision>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn rules_module_accessible() {
        let _exists = std::any::type_name::<abp_policy::rules::RuleCondition>();
        assert!(!_exists.is_empty());
    }
}

// ==========================================================================
// 38. abp-protocol — module re-exports
// ==========================================================================

mod protocol_modules {
    #[test]
    fn batch_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::batch::BatchRequest>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn builder_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::builder::BuilderError>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn codec_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::codec::StreamingCodec>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn compress_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::compress::CompressionAlgorithm>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn router_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::router::MessageRoute>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn stream_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::stream::StreamParser>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn validate_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::validate::ValidationError>();
        assert!(!_exists.is_empty());
    }

    #[test]
    fn version_module_accessible() {
        let _exists = std::any::type_name::<abp_protocol::version::VersionError>();
        assert!(!_exists.is_empty());
    }
}

// ==========================================================================
// 39. Cross-crate integration: PolicyProfile from core works with PolicyEngine
// ==========================================================================

mod cross_crate_integration {
    use abp_core::PolicyProfile;
    use abp_policy::PolicyEngine;

    #[test]
    fn core_policy_profile_works_with_policy_engine() {
        let profile = PolicyProfile {
            allowed_tools: vec!["Read".into()],
            disallowed_tools: vec!["Bash".into()],
            deny_read: vec![],
            deny_write: vec!["**/.git/**".into()],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        };
        let engine = PolicyEngine::new(&profile).unwrap();
        assert!(engine.can_use_tool("Read").allowed);
        assert!(!engine.can_use_tool("Bash").allowed);
    }
}
