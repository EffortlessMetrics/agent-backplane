#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive API surface stability tests.
//!
//! These tests verify that public types, constructors, methods, trait
//! implementations, enum variants, constants, and re-exports across all
//! primary crates remain accessible and stable.
//!
//! Categories:
//! 1. Trait implementations (30+ tests)
//! 2. Type construction and defaults (25+ tests)
//! 3. Method signatures (25+ tests)
//! 4. Enum exhaustiveness (20+ tests)
//! 5. Re-exports and module structure (15+ tests)
//! 6. Version and contract stability (15+ tests)

// ==========================================================================
// 1. Trait implementations — Clone, Debug, Send, Sync, Serialize, etc.
// ==========================================================================

mod trait_impls {
    use std::collections::BTreeMap;
    use std::fmt::Debug;

    use abp_core::*;
    use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
    use abp_protocol::{Envelope, JsonlCodec, ProtocolError};
    use serde::{Deserialize, Serialize};

    fn assert_clone<T: Clone>() {}
    fn assert_debug<T: Debug>() {}
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_serialize<T: Serialize>() {}
    fn assert_deserialize<'a, T: Deserialize<'a>>() {}
    fn assert_json_schema<T: schemars::JsonSchema>() {}
    fn assert_display<T: std::fmt::Display>() {}
    fn assert_error<T: std::error::Error>() {}

    // ── Clone ──────────────────────────────────────────────────────────

    #[test]
    fn work_order_is_clone() {
        assert_clone::<WorkOrder>();
    }

    #[test]
    fn receipt_is_clone() {
        assert_clone::<Receipt>();
    }

    #[test]
    fn agent_event_is_clone() {
        assert_clone::<AgentEvent>();
    }

    #[test]
    fn agent_event_kind_is_clone() {
        assert_clone::<AgentEventKind>();
    }

    #[test]
    fn execution_lane_is_clone() {
        assert_clone::<ExecutionLane>();
    }

    #[test]
    fn outcome_is_clone() {
        assert_clone::<Outcome>();
    }

    #[test]
    fn capability_is_clone() {
        assert_clone::<Capability>();
    }

    #[test]
    fn support_level_is_clone() {
        assert_clone::<SupportLevel>();
    }

    #[test]
    fn execution_mode_is_clone() {
        assert_clone::<ExecutionMode>();
    }

    #[test]
    fn backend_identity_is_clone() {
        assert_clone::<BackendIdentity>();
    }

    #[test]
    fn envelope_is_clone() {
        assert_clone::<Envelope>();
    }

    #[test]
    fn error_code_is_clone() {
        assert_clone::<ErrorCode>();
    }

    #[test]
    fn error_info_is_clone() {
        assert_clone::<ErrorInfo>();
    }

    // ── Debug ──────────────────────────────────────────────────────────

    #[test]
    fn work_order_is_debug() {
        assert_debug::<WorkOrder>();
    }

    #[test]
    fn receipt_is_debug() {
        assert_debug::<Receipt>();
    }

    #[test]
    fn agent_event_is_debug() {
        assert_debug::<AgentEvent>();
    }

    #[test]
    fn outcome_is_debug() {
        assert_debug::<Outcome>();
    }

    #[test]
    fn error_code_is_debug() {
        assert_debug::<ErrorCode>();
    }

    #[test]
    fn abp_error_is_debug() {
        assert_debug::<AbpError>();
    }

    #[test]
    fn envelope_is_debug() {
        assert_debug::<Envelope>();
    }

    #[test]
    fn protocol_error_is_debug() {
        assert_debug::<ProtocolError>();
    }

    #[test]
    fn contract_error_is_debug() {
        assert_debug::<ContractError>();
    }

    #[test]
    fn jsonl_codec_is_debug() {
        assert_debug::<JsonlCodec>();
    }

    // ── Send + Sync ────────────────────────────────────────────────────

    #[test]
    fn work_order_is_send_sync() {
        assert_send_sync::<WorkOrder>();
    }

    #[test]
    fn receipt_is_send_sync() {
        assert_send_sync::<Receipt>();
    }

    #[test]
    fn agent_event_is_send_sync() {
        assert_send_sync::<AgentEvent>();
    }

    #[test]
    fn envelope_is_send_sync() {
        assert_send_sync::<Envelope>();
    }

    #[test]
    fn error_code_is_send_sync() {
        assert_send_sync::<ErrorCode>();
    }

    #[test]
    fn error_info_is_send_sync() {
        assert_send_sync::<ErrorInfo>();
    }

    // ── std::error::Error ──────────────────────────────────────────────

    #[test]
    fn abp_error_implements_std_error() {
        assert_error::<AbpError>();
    }

    #[test]
    fn protocol_error_implements_std_error() {
        assert_error::<ProtocolError>();
    }

    #[test]
    fn contract_error_implements_std_error() {
        assert_error::<ContractError>();
    }

    #[test]
    fn config_error_implements_std_error() {
        assert_error::<abp_config::ConfigError>();
    }

    // ── Serialize + Deserialize ────────────────────────────────────────

    #[test]
    fn work_order_is_serializable() {
        assert_serialize::<WorkOrder>();
        assert_deserialize::<WorkOrder>();
    }

    #[test]
    fn receipt_is_serializable() {
        assert_serialize::<Receipt>();
        assert_deserialize::<Receipt>();
    }

    #[test]
    fn agent_event_is_serializable() {
        assert_serialize::<AgentEvent>();
        assert_deserialize::<AgentEvent>();
    }

    #[test]
    fn envelope_is_serializable() {
        assert_serialize::<Envelope>();
        assert_deserialize::<Envelope>();
    }

    #[test]
    fn error_code_is_serializable() {
        assert_serialize::<ErrorCode>();
        assert_deserialize::<ErrorCode>();
    }

    #[test]
    fn outcome_is_serializable() {
        assert_serialize::<Outcome>();
        assert_deserialize::<Outcome>();
    }

    #[test]
    fn capability_is_serializable() {
        assert_serialize::<Capability>();
        assert_deserialize::<Capability>();
    }

    #[test]
    fn execution_mode_is_serializable() {
        assert_serialize::<ExecutionMode>();
        assert_deserialize::<ExecutionMode>();
    }

    // ── JsonSchema ─────────────────────────────────────────────────────

    #[test]
    fn work_order_has_json_schema() {
        assert_json_schema::<WorkOrder>();
    }

    #[test]
    fn receipt_has_json_schema() {
        assert_json_schema::<Receipt>();
    }

    #[test]
    fn agent_event_has_json_schema() {
        assert_json_schema::<AgentEvent>();
    }

    #[test]
    fn outcome_has_json_schema() {
        assert_json_schema::<Outcome>();
    }

    #[test]
    fn capability_has_json_schema() {
        assert_json_schema::<Capability>();
    }

    #[test]
    fn execution_mode_has_json_schema() {
        assert_json_schema::<ExecutionMode>();
    }

    #[test]
    fn error_code_has_json_schema() {
        assert_json_schema::<ErrorCode>();
    }

    // ── Display ────────────────────────────────────────────────────────

    #[test]
    fn error_code_implements_display() {
        assert_display::<ErrorCode>();
    }

    #[test]
    fn abp_error_implements_display() {
        assert_display::<AbpError>();
    }

    #[test]
    fn error_info_implements_display() {
        assert_display::<ErrorInfo>();
    }

    #[test]
    fn error_category_implements_display() {
        assert_display::<ErrorCategory>();
    }

    #[test]
    fn protocol_error_implements_display() {
        assert_display::<ProtocolError>();
    }

    #[test]
    fn contract_error_implements_display() {
        assert_display::<ContractError>();
    }
}

// ==========================================================================
// 2. Type construction and defaults
// ==========================================================================

mod type_construction {
    use abp_core::*;
    use abp_error::{AbpError, ErrorCode, ErrorInfo};
    use std::collections::BTreeMap;

    // ── WorkOrder builder defaults ─────────────────────────────────────

    #[test]
    fn work_order_builder_produces_valid_defaults() {
        let wo = WorkOrderBuilder::new("test task").build();
        assert_eq!(wo.task, "test task");
        assert!(!wo.id.is_nil());
    }

    #[test]
    fn work_order_builder_default_lane_is_patch_first() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    }

    #[test]
    fn work_order_builder_default_root_is_dot() {
        let wo = WorkOrderBuilder::new("t").build();
        assert_eq!(wo.workspace.root, ".");
    }

    #[test]
    fn work_order_builder_default_workspace_mode_is_staged() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    }

    #[test]
    fn work_order_builder_default_include_is_empty() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.workspace.include.is_empty());
    }

    #[test]
    fn work_order_builder_default_exclude_is_empty() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.workspace.exclude.is_empty());
    }

    #[test]
    fn work_order_builder_default_config_model_is_none() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.config.model.is_none());
    }

    #[test]
    fn work_order_builder_default_max_turns_is_none() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.config.max_turns.is_none());
    }

    #[test]
    fn work_order_builder_default_max_budget_is_none() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.config.max_budget_usd.is_none());
    }

    #[test]
    fn work_order_builder_default_vendor_is_empty() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.config.vendor.is_empty());
    }

    #[test]
    fn work_order_builder_default_env_is_empty() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.config.env.is_empty());
    }

    // ── Receipt builder defaults ───────────────────────────────────────

    #[test]
    fn receipt_builder_default_outcome_is_complete() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_builder_default_hash_is_none() {
        let r = ReceiptBuilder::new("mock").build();
        assert!(r.receipt_sha256.is_none());
    }

    #[test]
    fn receipt_builder_default_trace_is_empty() {
        let r = ReceiptBuilder::new("mock").build();
        assert!(r.trace.is_empty());
    }

    #[test]
    fn receipt_builder_default_artifacts_is_empty() {
        let r = ReceiptBuilder::new("mock").build();
        assert!(r.artifacts.is_empty());
    }

    #[test]
    fn receipt_builder_default_capabilities_is_empty() {
        let r = ReceiptBuilder::new("mock").build();
        assert!(r.capabilities.is_empty());
    }

    #[test]
    fn receipt_builder_default_mode_is_mapped() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn receipt_builder_sets_backend_id() {
        let r = ReceiptBuilder::new("my-backend").build();
        assert_eq!(r.backend.id, "my-backend");
    }

    #[test]
    fn receipt_builder_sets_contract_version() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    // ── Default trait impls ────────────────────────────────────────────

    #[test]
    fn runtime_config_defaults_all_none_or_empty() {
        let c = RuntimeConfig::default();
        assert!(c.model.is_none());
        assert!(c.vendor.is_empty());
        assert!(c.env.is_empty());
        assert!(c.max_budget_usd.is_none());
        assert!(c.max_turns.is_none());
    }

    #[test]
    fn policy_profile_defaults_all_empty() {
        let p = PolicyProfile::default();
        assert!(p.allowed_tools.is_empty());
        assert!(p.disallowed_tools.is_empty());
        assert!(p.deny_read.is_empty());
        assert!(p.deny_write.is_empty());
        assert!(p.allow_network.is_empty());
        assert!(p.deny_network.is_empty());
        assert!(p.require_approval_for.is_empty());
    }

    #[test]
    fn capability_requirements_defaults_empty() {
        let cr = CapabilityRequirements::default();
        assert!(cr.required.is_empty());
    }

    #[test]
    fn context_packet_defaults_empty() {
        let cp = ContextPacket::default();
        assert!(cp.files.is_empty());
        assert!(cp.snippets.is_empty());
    }

    #[test]
    fn usage_normalized_defaults_all_none() {
        let u = UsageNormalized::default();
        assert!(u.input_tokens.is_none());
        assert!(u.output_tokens.is_none());
        assert!(u.cache_read_tokens.is_none());
        assert!(u.cache_write_tokens.is_none());
        assert!(u.request_units.is_none());
        assert!(u.estimated_cost_usd.is_none());
    }

    #[test]
    fn verification_report_defaults_sensible() {
        let v = VerificationReport::default();
        assert!(v.git_diff.is_none());
        assert!(v.git_status.is_none());
        assert_eq!(v.harness_ok, false);
    }

    #[test]
    fn execution_mode_default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn backplane_config_defaults_sensible() {
        let c = abp_config::BackplaneConfig::default();
        assert!(c.default_backend.is_none());
        assert!(c.workspace_dir.is_none());
        assert_eq!(c.log_level.as_deref(), Some("info"));
        assert!(c.backends.is_empty());
        assert!(c.policy_profiles.is_empty());
    }
}

// ==========================================================================
// 3. Method signatures
// ==========================================================================

mod method_signatures {
    use abp_core::*;
    use abp_error::{AbpError, ErrorCategory, ErrorCode, ErrorInfo};
    use abp_protocol::{Envelope, JsonlCodec};
    use std::collections::BTreeMap;

    // ── Receipt hashing ────────────────────────────────────────────────

    #[test]
    fn receipt_with_hash_returns_non_none_hash() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        assert!(r.receipt_sha256.is_some());
        assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
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

    #[test]
    fn receipt_hash_length_is_64_hex() {
        let r = ReceiptBuilder::new("mock").build();
        let h = receipt_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_verify_hash_returns_true_for_valid() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        assert!(abp_receipt::verify_hash(&r));
    }

    #[test]
    fn receipt_verify_hash_returns_false_for_tampered() {
        let mut r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        r.receipt_sha256 = Some("tampered".into());
        assert!(!abp_receipt::verify_hash(&r));
    }

    #[test]
    fn receipt_verify_hash_returns_true_when_no_hash() {
        let r = ReceiptBuilder::new("mock").build();
        assert!(abp_receipt::verify_hash(&r));
    }

    // ── Envelope encode/decode ─────────────────────────────────────────

    #[test]
    fn envelope_encode_produces_valid_jsonl() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.ends_with('\n'));
        assert!(line.contains("\"t\":\"fatal\""));
    }

    #[test]
    fn envelope_decode_accepts_valid_jsonl() {
        let line = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
        let env = JsonlCodec::decode(line).unwrap();
        assert!(matches!(env, Envelope::Fatal { error, .. } if error == "boom"));
    }

    #[test]
    fn envelope_roundtrip_fatal() {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "test error".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Fatal { error, .. } if error == "test error"));
    }

    #[test]
    fn envelope_hello_roundtrip() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    // ── ErrorCode methods ──────────────────────────────────────────────

    #[test]
    fn error_code_as_str_returns_snake_case() {
        let code = ErrorCode::BackendTimeout;
        let s = code.as_str();
        assert_eq!(s, "backend_timeout");
        assert!(!s.contains('-'));
        assert!(!s.chars().any(|c| c.is_uppercase()));
    }

    #[test]
    fn error_code_as_str_all_are_snake_case() {
        let codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::BackendNotFound,
            ErrorCode::PolicyDenied,
            ErrorCode::Internal,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::WorkspaceStagingFailed,
        ];
        for code in &codes {
            let s = code.as_str();
            assert!(
                s.chars()
                    .all(|c| c.is_lowercase() || c == '_' || c.is_ascii_digit()),
                "ErrorCode::{:?}.as_str() = {:?} is not snake_case",
                code,
                s
            );
        }
    }

    #[test]
    fn error_code_category_returns_valid_category() {
        assert_eq!(ErrorCode::BackendTimeout.category(), ErrorCategory::Backend);
        assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
        assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.category(),
            ErrorCategory::Protocol
        );
    }

    #[test]
    fn error_code_message_returns_nonempty_string() {
        let code = ErrorCode::BackendTimeout;
        let msg = code.message();
        assert!(!msg.is_empty());
    }

    #[test]
    fn error_code_is_retryable_for_transient_errors() {
        assert!(ErrorCode::BackendTimeout.is_retryable());
        assert!(ErrorCode::BackendUnavailable.is_retryable());
        assert!(ErrorCode::BackendRateLimited.is_retryable());
        assert!(!ErrorCode::PolicyDenied.is_retryable());
        assert!(!ErrorCode::Internal.is_retryable());
    }

    // ── AbpError methods ───────────────────────────────────────────────

    #[test]
    fn abp_error_new_creates_valid_error() {
        let err = AbpError::new(ErrorCode::Internal, "something broke");
        assert_eq!(err.code, ErrorCode::Internal);
        assert_eq!(err.message, "something broke");
        assert!(err.source.is_none());
        assert!(err.context.is_empty());
    }

    #[test]
    fn abp_error_with_context_adds_context() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out")
            .with_context("backend", "openai")
            .with_context("timeout_ms", 30_000);
        assert_eq!(err.context.len(), 2);
        assert!(err.context.contains_key("backend"));
        assert!(err.context.contains_key("timeout_ms"));
    }

    #[test]
    fn abp_error_with_source_adds_source_chain() {
        let inner = std::io::Error::new(std::io::ErrorKind::TimedOut, "network timeout");
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out").with_source(inner);
        assert!(err.source.is_some());
        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }

    #[test]
    fn abp_error_to_info_preserves_fields() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("k", "v");
        let info = err.to_info();
        assert_eq!(info.code, ErrorCode::BackendTimeout);
        assert_eq!(info.message, "timed out");
        assert_eq!(info.details.len(), 1);
        assert!(info.is_retryable);
    }

    // ── canonical_json / sha256_hex ────────────────────────────────────

    #[test]
    fn canonical_json_sorts_keys() {
        let json = canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
        assert!(json.starts_with(r#"{"a":1"#));
    }

    #[test]
    fn sha256_hex_returns_64_char_hex() {
        let hex = sha256_hex(b"hello");
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── SupportLevel ───────────────────────────────────────────────────

    #[test]
    fn support_level_satisfies_logic() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    // ── Protocol version parsing ───────────────────────────────────────

    #[test]
    fn parse_version_valid() {
        assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(abp_protocol::parse_version("abp/v2.3"), Some((2, 3)));
    }

    #[test]
    fn parse_version_invalid_returns_none() {
        assert_eq!(abp_protocol::parse_version("invalid"), None);
        assert_eq!(abp_protocol::parse_version(""), None);
    }

    #[test]
    fn is_compatible_version_same_major() {
        assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
    }
}

// ==========================================================================
// 4. Enum exhaustiveness — verify all expected variants exist
// ==========================================================================

mod enum_exhaustiveness {
    use abp_core::*;
    use abp_error::{ErrorCategory, ErrorCode};

    #[test]
    fn agent_event_kind_has_run_started() {
        let _ = AgentEventKind::RunStarted {
            message: String::new(),
        };
    }

    #[test]
    fn agent_event_kind_has_run_completed() {
        let _ = AgentEventKind::RunCompleted {
            message: String::new(),
        };
    }

    #[test]
    fn agent_event_kind_has_assistant_delta() {
        let _ = AgentEventKind::AssistantDelta {
            text: String::new(),
        };
    }

    #[test]
    fn agent_event_kind_has_assistant_message() {
        let _ = AgentEventKind::AssistantMessage {
            text: String::new(),
        };
    }

    #[test]
    fn agent_event_kind_has_tool_call() {
        let _ = AgentEventKind::ToolCall {
            tool_name: String::new(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        };
    }

    #[test]
    fn agent_event_kind_has_tool_result() {
        let _ = AgentEventKind::ToolResult {
            tool_name: String::new(),
            tool_use_id: None,
            output: serde_json::json!({}),
            is_error: false,
        };
    }

    #[test]
    fn agent_event_kind_has_file_changed() {
        let _ = AgentEventKind::FileChanged {
            path: String::new(),
            summary: String::new(),
        };
    }

    #[test]
    fn agent_event_kind_has_command_executed() {
        let _ = AgentEventKind::CommandExecuted {
            command: String::new(),
            exit_code: None,
            output_preview: None,
        };
    }

    #[test]
    fn agent_event_kind_has_warning() {
        let _ = AgentEventKind::Warning {
            message: String::new(),
        };
    }

    #[test]
    fn agent_event_kind_has_error() {
        let _ = AgentEventKind::Error {
            message: String::new(),
            error_code: None,
        };
    }

    #[test]
    fn outcome_has_all_expected_variants() {
        let _ = Outcome::Complete;
        let _ = Outcome::Partial;
        let _ = Outcome::Failed;
    }

    #[test]
    fn execution_lane_has_all_expected_variants() {
        let _ = ExecutionLane::PatchFirst;
        let _ = ExecutionLane::WorkspaceFirst;
    }

    #[test]
    fn execution_mode_has_all_expected_variants() {
        let _ = ExecutionMode::Passthrough;
        let _ = ExecutionMode::Mapped;
    }

    #[test]
    fn support_level_has_all_expected_variants() {
        let _ = SupportLevel::Native;
        let _ = SupportLevel::Emulated;
        let _ = SupportLevel::Unsupported;
        let _ = SupportLevel::Restricted {
            reason: String::new(),
        };
    }

    #[test]
    fn workspace_mode_has_all_expected_variants() {
        let _ = WorkspaceMode::PassThrough;
        let _ = WorkspaceMode::Staged;
    }

    #[test]
    fn min_support_has_all_expected_variants() {
        let _ = MinSupport::Native;
        let _ = MinSupport::Emulated;
    }

    #[test]
    fn capability_has_streaming_and_tool_variants() {
        let _ = Capability::Streaming;
        let _ = Capability::ToolRead;
        let _ = Capability::ToolWrite;
        let _ = Capability::ToolEdit;
        let _ = Capability::ToolBash;
        let _ = Capability::ToolGlob;
        let _ = Capability::ToolGrep;
        let _ = Capability::ToolWebSearch;
        let _ = Capability::ToolWebFetch;
        let _ = Capability::ToolAskUser;
    }

    #[test]
    fn capability_has_hooks_and_session_variants() {
        let _ = Capability::HooksPreToolUse;
        let _ = Capability::HooksPostToolUse;
        let _ = Capability::SessionResume;
        let _ = Capability::SessionFork;
        let _ = Capability::Checkpointing;
    }

    #[test]
    fn capability_has_mcp_and_generic_variants() {
        let _ = Capability::McpClient;
        let _ = Capability::McpServer;
        let _ = Capability::ToolUse;
        let _ = Capability::ExtendedThinking;
        let _ = Capability::ImageInput;
        let _ = Capability::PdfInput;
        let _ = Capability::CodeExecution;
    }

    #[test]
    fn capability_has_params_variants() {
        let _ = Capability::Temperature;
        let _ = Capability::TopP;
        let _ = Capability::TopK;
        let _ = Capability::MaxTokens;
        let _ = Capability::FrequencyPenalty;
        let _ = Capability::PresencePenalty;
    }

    #[test]
    fn capability_has_legacy_and_advanced_variants() {
        let _ = Capability::FunctionCalling;
        let _ = Capability::Vision;
        let _ = Capability::Audio;
        let _ = Capability::JsonMode;
        let _ = Capability::SystemMessage;
        let _ = Capability::CacheControl;
        let _ = Capability::BatchMode;
        let _ = Capability::Embeddings;
        let _ = Capability::ImageGeneration;
        let _ = Capability::Logprobs;
        let _ = Capability::SeedDeterminism;
        let _ = Capability::StopSequences;
        let _ = Capability::StructuredOutputJsonSchema;
    }

    // ── ErrorCode variants ─────────────────────────────────────────────

    #[test]
    fn error_code_has_protocol_variants() {
        let _ = ErrorCode::ProtocolInvalidEnvelope;
        let _ = ErrorCode::ProtocolHandshakeFailed;
        let _ = ErrorCode::ProtocolMissingRefId;
        let _ = ErrorCode::ProtocolUnexpectedMessage;
        let _ = ErrorCode::ProtocolVersionMismatch;
    }

    #[test]
    fn error_code_has_mapping_variants() {
        let _ = ErrorCode::MappingUnsupportedCapability;
        let _ = ErrorCode::MappingDialectMismatch;
        let _ = ErrorCode::MappingLossyConversion;
        let _ = ErrorCode::MappingUnmappableTool;
    }

    #[test]
    fn error_code_has_backend_variants() {
        let _ = ErrorCode::BackendNotFound;
        let _ = ErrorCode::BackendUnavailable;
        let _ = ErrorCode::BackendTimeout;
        let _ = ErrorCode::BackendRateLimited;
        let _ = ErrorCode::BackendAuthFailed;
        let _ = ErrorCode::BackendModelNotFound;
        let _ = ErrorCode::BackendCrashed;
    }

    #[test]
    fn error_code_has_execution_variants() {
        let _ = ErrorCode::ExecutionToolFailed;
        let _ = ErrorCode::ExecutionWorkspaceError;
        let _ = ErrorCode::ExecutionPermissionDenied;
    }

    #[test]
    fn error_code_has_contract_variants() {
        let _ = ErrorCode::ContractVersionMismatch;
        let _ = ErrorCode::ContractSchemaViolation;
        let _ = ErrorCode::ContractInvalidReceipt;
    }

    #[test]
    fn error_code_has_capability_policy_workspace_variants() {
        let _ = ErrorCode::CapabilityUnsupported;
        let _ = ErrorCode::CapabilityEmulationFailed;
        let _ = ErrorCode::PolicyDenied;
        let _ = ErrorCode::PolicyInvalid;
        let _ = ErrorCode::WorkspaceInitFailed;
        let _ = ErrorCode::WorkspaceStagingFailed;
    }

    #[test]
    fn error_code_has_ir_receipt_dialect_config_internal() {
        let _ = ErrorCode::IrLoweringFailed;
        let _ = ErrorCode::IrInvalid;
        let _ = ErrorCode::ReceiptHashMismatch;
        let _ = ErrorCode::ReceiptChainBroken;
        let _ = ErrorCode::DialectUnknown;
        let _ = ErrorCode::DialectMappingFailed;
        let _ = ErrorCode::ConfigInvalid;
        let _ = ErrorCode::Internal;
    }

    #[test]
    fn error_category_has_all_expected_variants() {
        let _ = ErrorCategory::Protocol;
        let _ = ErrorCategory::Backend;
        let _ = ErrorCategory::Capability;
        let _ = ErrorCategory::Policy;
        let _ = ErrorCategory::Workspace;
        let _ = ErrorCategory::Ir;
        let _ = ErrorCategory::Receipt;
        let _ = ErrorCategory::Dialect;
        let _ = ErrorCategory::Config;
        let _ = ErrorCategory::Mapping;
        let _ = ErrorCategory::Execution;
        let _ = ErrorCategory::Contract;
        let _ = ErrorCategory::Internal;
    }

    #[test]
    fn dialect_has_all_expected_variants() {
        let _ = abp_dialect::Dialect::OpenAi;
        let _ = abp_dialect::Dialect::Claude;
        let _ = abp_dialect::Dialect::Gemini;
        let _ = abp_dialect::Dialect::Codex;
        let _ = abp_dialect::Dialect::Kimi;
        let _ = abp_dialect::Dialect::Copilot;
    }

    #[test]
    fn ir_role_has_all_expected_variants() {
        use abp_core::ir::*;
        let _ = IrRole::System;
        let _ = IrRole::User;
        let _ = IrRole::Assistant;
        let _ = IrRole::Tool;
    }

    #[test]
    fn ir_content_block_has_all_expected_variants() {
        use abp_core::ir::*;
        let _ = IrContentBlock::Text {
            text: String::new(),
        };
        let _ = IrContentBlock::Image {
            media_type: String::new(),
            data: String::new(),
        };
        let _ = IrContentBlock::ToolUse {
            id: String::new(),
            name: String::new(),
            input: serde_json::json!({}),
        };
        let _ = IrContentBlock::ToolResult {
            tool_use_id: String::new(),
            content: vec![],
            is_error: false,
        };
        let _ = IrContentBlock::Thinking {
            text: String::new(),
        };
    }
}

// ==========================================================================
// 5. Re-exports and module structure
// ==========================================================================

mod reexports {
    #[test]
    fn abp_core_exports_contract_version() {
        let _ = abp_core::CONTRACT_VERSION;
    }

    #[test]
    fn abp_core_exports_work_order() {
        let _: Option<abp_core::WorkOrder> = None;
    }

    #[test]
    fn abp_core_exports_receipt() {
        let _: Option<abp_core::Receipt> = None;
    }

    #[test]
    fn abp_core_exports_agent_event() {
        let _: Option<abp_core::AgentEvent> = None;
    }

    #[test]
    fn abp_core_exports_all_builders() {
        let _: Option<abp_core::WorkOrderBuilder> = None;
        let _: Option<abp_core::ReceiptBuilder> = None;
    }

    #[test]
    fn abp_core_exports_all_enums() {
        let _: Option<abp_core::ExecutionLane> = None;
        let _: Option<abp_core::Outcome> = None;
        let _: Option<abp_core::Capability> = None;
        let _: Option<abp_core::SupportLevel> = None;
        let _: Option<abp_core::ExecutionMode> = None;
        let _: Option<abp_core::MinSupport> = None;
        let _: Option<abp_core::WorkspaceMode> = None;
    }

    #[test]
    fn abp_core_exports_all_structs() {
        let _: Option<abp_core::BackendIdentity> = None;
        let _: Option<abp_core::RuntimeConfig> = None;
        let _: Option<abp_core::PolicyProfile> = None;
        let _: Option<abp_core::CapabilityRequirements> = None;
        let _: Option<abp_core::ContextPacket> = None;
        let _: Option<abp_core::WorkspaceSpec> = None;
        let _: Option<abp_core::RunMetadata> = None;
        let _: Option<abp_core::UsageNormalized> = None;
        let _: Option<abp_core::VerificationReport> = None;
        let _: Option<abp_core::ArtifactRef> = None;
        let _: Option<abp_core::ContextSnippet> = None;
        let _: Option<abp_core::CapabilityRequirement> = None;
    }

    #[test]
    fn abp_core_exports_capability_manifest_type_alias() {
        let _: abp_core::CapabilityManifest = std::collections::BTreeMap::new();
    }

    #[test]
    fn abp_core_exports_functions() {
        let _ = abp_core::canonical_json as fn(&serde_json::Value) -> _;
        let _ = abp_core::sha256_hex as fn(&[u8]) -> String;
        let _ = abp_core::receipt_hash as fn(&abp_core::Receipt) -> _;
    }

    #[test]
    fn abp_core_exports_ir_module() {
        let _: Option<abp_core::ir::IrRole> = None;
        let _: Option<abp_core::ir::IrContentBlock> = None;
        let _: Option<abp_core::ir::IrMessage> = None;
        let _: Option<abp_core::ir::IrToolDefinition> = None;
    }

    #[test]
    fn abp_protocol_exports_envelope() {
        let _: Option<abp_protocol::Envelope> = None;
    }

    #[test]
    fn abp_protocol_exports_jsonl_codec() {
        let _: Option<abp_protocol::JsonlCodec> = None;
    }

    #[test]
    fn abp_protocol_exports_protocol_error() {
        let _: Option<abp_protocol::ProtocolError> = None;
    }

    #[test]
    fn abp_protocol_exports_version_functions() {
        let _ = abp_protocol::parse_version as fn(&str) -> Option<(u32, u32)>;
        let _ = abp_protocol::is_compatible_version as fn(&str, &str) -> bool;
    }

    #[test]
    fn abp_integrations_exports_backend_trait() {
        fn assert_backend_trait_exists<T: abp_integrations::Backend>() {}
    }

    #[test]
    fn abp_integrations_exports_mock_backend() {
        let _: Option<abp_integrations::MockBackend> = None;
    }

    #[test]
    fn abp_integrations_exports_sidecar_backend() {
        let _: Option<abp_integrations::SidecarBackend> = None;
    }

    #[test]
    fn abp_error_exports_core_types() {
        let _: Option<abp_error::AbpError> = None;
        let _: Option<abp_error::ErrorCode> = None;
        let _: Option<abp_error::ErrorInfo> = None;
        let _: Option<abp_error::ErrorCategory> = None;
    }

    #[test]
    fn abp_receipt_reexports_core_receipt() {
        let _: Option<abp_receipt::Receipt> = None;
        let _: Option<abp_receipt::Outcome> = None;
        let _: Option<abp_receipt::ReceiptBuilder> = None;
    }

    #[test]
    fn abp_receipt_exports_own_functions() {
        let _ = abp_receipt::canonicalize as fn(&abp_core::Receipt) -> _;
        let _ = abp_receipt::compute_hash as fn(&abp_core::Receipt) -> _;
        let _ = abp_receipt::verify_hash as fn(&abp_core::Receipt) -> bool;
    }

    #[test]
    fn abp_ir_reexports_core_ir_types() {
        let _: Option<abp_ir::IrRole> = None;
        let _: Option<abp_ir::IrContentBlock> = None;
        let _: Option<abp_ir::IrMessage> = None;
    }

    #[test]
    fn abp_dialect_exports_dialect_enum() {
        let _: Option<abp_dialect::Dialect> = None;
    }

    #[test]
    fn abp_policy_exports_engine_and_decision() {
        let _: Option<abp_policy::PolicyEngine> = None;
        let _: Option<abp_policy::Decision> = None;
    }

    #[test]
    fn abp_glob_exports_match_types() {
        let _: Option<abp_glob::IncludeExcludeGlobs> = None;
        let _: Option<abp_glob::MatchDecision> = None;
    }

    #[test]
    fn abp_config_exports_config_types() {
        let _: Option<abp_config::BackplaneConfig> = None;
        let _: Option<abp_config::BackendEntry> = None;
        let _: Option<abp_config::ConfigError> = None;
        let _: Option<abp_config::ConfigWarning> = None;
    }

    #[test]
    fn abp_capability_exports_negotiation_types() {
        let _: Option<abp_capability::NegotiationResult> = None;
        let _: Option<abp_capability::CompatibilityReport> = None;
        let _: Option<abp_capability::SupportLevel> = None;
        let _: Option<abp_capability::EmulationStrategy> = None;
        let _: Option<abp_capability::CapabilityRegistry> = None;
    }

    #[test]
    fn abp_validate_crate_has_validator_types() {
        // abp-validate exports validation types; verified via abp_core::validate module
        let _: Option<abp_core::ContractError> = None;
    }

    #[test]
    fn abp_mapping_exports_error_and_fidelity() {
        let _: Option<abp_mapping::MappingError> = None;
        let _: Option<abp_mapping::Fidelity> = None;
    }

    #[test]
    fn abp_projection_exports_matrix_types() {
        let _: Option<abp_projection::ProjectionError> = None;
        let _: Option<abp_projection::BackendEntry> = None;
        let _: Option<abp_projection::ProjectionScore> = None;
    }
}

// ==========================================================================
// 6. Version and contract stability
// ==========================================================================

mod version_and_contract {
    use abp_core::*;
    use abp_protocol::{Envelope, JsonlCodec};

    #[test]
    fn contract_version_is_abp_v01() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn contract_version_starts_with_abp() {
        assert!(CONTRACT_VERSION.starts_with("abp/"));
    }

    #[test]
    fn envelope_tag_uses_t_discriminator() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "test".into(),
            error_code: None,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(
            json.contains("\"t\":"),
            "Envelope should use 't' as tag discriminator, got: {json}"
        );
    }

    #[test]
    fn agent_event_kind_uses_type_discriminator() {
        let kind = AgentEventKind::Warning {
            message: "test".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(
            json.contains("\"type\":"),
            "AgentEventKind should use 'type' as tag, got: {json}"
        );
    }

    #[test]
    fn outcome_serializes_as_snake_case() {
        let json = serde_json::to_string(&Outcome::Complete).unwrap();
        assert_eq!(json, "\"complete\"");
        let json = serde_json::to_string(&Outcome::Partial).unwrap();
        assert_eq!(json, "\"partial\"");
        let json = serde_json::to_string(&Outcome::Failed).unwrap();
        assert_eq!(json, "\"failed\"");
    }

    #[test]
    fn execution_lane_serializes_as_snake_case() {
        let json = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
        assert_eq!(json, "\"patch_first\"");
        let json = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
        assert_eq!(json, "\"workspace_first\"");
    }

    #[test]
    fn execution_mode_serializes_as_snake_case() {
        let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
        assert_eq!(json, "\"passthrough\"");
        let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
        assert_eq!(json, "\"mapped\"");
    }

    #[test]
    fn capability_serializes_as_snake_case() {
        let json = serde_json::to_string(&Capability::ToolRead).unwrap();
        assert_eq!(json, "\"tool_read\"");
        let json = serde_json::to_string(&Capability::ExtendedThinking).unwrap();
        assert_eq!(json, "\"extended_thinking\"");
    }

    #[test]
    fn support_level_serializes_as_snake_case() {
        let json = serde_json::to_string(&SupportLevel::Native).unwrap();
        assert_eq!(json, "\"native\"");
        let json = serde_json::to_string(&SupportLevel::Emulated).unwrap();
        assert_eq!(json, "\"emulated\"");
    }

    #[test]
    fn agent_event_kind_variant_names_are_snake_case() {
        let kind = AgentEventKind::RunStarted {
            message: "hi".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("\"type\":\"run_started\""), "got: {json}");

        let kind = AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("\"type\":\"tool_call\""), "got: {json}");
    }

    #[test]
    fn error_code_serializes_as_snake_case() {
        use abp_error::ErrorCode;
        let json = serde_json::to_string(&ErrorCode::BackendTimeout).unwrap();
        assert_eq!(json, "\"backend_timeout\"");
        let json = serde_json::to_string(&ErrorCode::ProtocolInvalidEnvelope).unwrap();
        assert_eq!(json, "\"protocol_invalid_envelope\"");
    }

    #[test]
    fn json_schema_generation_works_for_work_order() {
        let schema = schemars::schema_for!(WorkOrder);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("WorkOrder") || json.contains("properties"));
    }

    #[test]
    fn json_schema_generation_works_for_receipt() {
        let schema = schemars::schema_for!(Receipt);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(!json.is_empty());
    }

    #[test]
    fn json_schema_generation_works_for_agent_event() {
        let schema = schemars::schema_for!(AgentEvent);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(!json.is_empty());
    }

    #[test]
    fn work_order_serde_roundtrip() {
        let wo = WorkOrderBuilder::new("test task")
            .lane(ExecutionLane::WorkspaceFirst)
            .model("gpt-4")
            .max_turns(10)
            .build();
        let json = serde_json::to_string(&wo).unwrap();
        let deserialized: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task, "test task");
        assert_eq!(deserialized.config.model.as_deref(), Some("gpt-4"));
        assert_eq!(deserialized.config.max_turns, Some(10));
    }

    #[test]
    fn receipt_serde_roundtrip() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Failed)
            .build()
            .with_hash()
            .unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.outcome, Outcome::Failed);
        assert!(deserialized.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_builder_contract_version_matches_constant() {
        let r = ReceiptBuilder::new("test").build();
        assert_eq!(r.meta.contract_version, "abp/v0.1");
    }

    #[test]
    fn hello_envelope_carries_contract_version() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("abp/v0.1"));
    }

    #[test]
    fn dialect_serializes_as_snake_case() {
        use abp_dialect::Dialect;
        let json = serde_json::to_string(&Dialect::OpenAi).unwrap();
        assert_eq!(json, "\"open_ai\"");
        let json = serde_json::to_string(&Dialect::Claude).unwrap();
        assert_eq!(json, "\"claude\"");
    }

    #[test]
    fn ir_role_serializes_as_snake_case() {
        use abp_core::ir::IrRole;
        let json = serde_json::to_string(&IrRole::System).unwrap();
        assert_eq!(json, "\"system\"");
        let json = serde_json::to_string(&IrRole::Assistant).unwrap();
        assert_eq!(json, "\"assistant\"");
    }
}
