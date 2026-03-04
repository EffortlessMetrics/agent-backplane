#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

// ---------------------------------------------------------------------------
// Helpers shared across all modules
// ---------------------------------------------------------------------------
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ContractError, ExecutionLane, ExecutionMode, MinSupport, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    canonical_json, receipt_hash, sha256_hex,
};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::BufReader;
use uuid::Uuid;

/// Build a minimal receipt with deterministic fields for hashing tests.
fn deterministic_receipt(backend_id: &str) -> Receipt {
    let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: backend_id.into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn make_hello() -> Envelope {
    Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    )
}

fn make_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

// =========================================================================
// Module: receipt_hash_determinism
// =========================================================================
mod receipt_hash_determinism {
    use super::*;

    #[test]
    fn same_receipt_produces_same_hash() {
        let r = deterministic_receipt("mock");
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_is_64_hex_chars() {
        let r = deterministic_receipt("mock");
        let h = receipt_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_backend_different_hash() {
        let r1 = deterministic_receipt("backend-a");
        let r2 = deterministic_receipt("backend-b");
        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn different_outcome_different_hash() {
        let mut r1 = deterministic_receipt("mock");
        let mut r2 = deterministic_receipt("mock");
        r1.outcome = Outcome::Complete;
        r2.outcome = Outcome::Failed;
        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn with_hash_attaches_hash() {
        let r = deterministic_receipt("mock");
        assert!(r.receipt_sha256.is_none());
        let r = r.with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
    }

    #[test]
    fn with_hash_returns_64_hex() {
        let r = deterministic_receipt("mock").with_hash().unwrap();
        let h = r.receipt_sha256.unwrap();
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn builder_with_hash_produces_hash() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(r.receipt_sha256.is_some());
    }

    #[test]
    fn hash_deterministic_across_with_hash_calls() {
        let r1 = deterministic_receipt("mock").with_hash().unwrap();
        let r2 = deterministic_receipt("mock").with_hash().unwrap();
        assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn sha256_hex_basic() {
        let h = sha256_hex(b"hello");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sha256_hex_deterministic() {
        assert_eq!(sha256_hex(b"test"), sha256_hex(b"test"));
    }

    #[test]
    fn sha256_hex_different_input_different_hash() {
        assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
    }

    #[test]
    fn sha256_hex_empty() {
        let h = sha256_hex(b"");
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn receipt_with_trace_hashes_deterministically() {
        let mut r = deterministic_receipt("mock");
        r.trace.push(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }));
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn receipt_with_artifacts_hashes_deterministically() {
        let mut r = deterministic_receipt("mock");
        r.artifacts.push(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        });
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_trace_different_hash() {
        let mut r1 = deterministic_receipt("mock");
        let mut r2 = deterministic_receipt("mock");
        r1.trace.push(make_event(AgentEventKind::RunStarted {
            message: "a".into(),
        }));
        r2.trace.push(make_event(AgentEventKind::RunStarted {
            message: "b".into(),
        }));
        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn receipt_mode_affects_hash() {
        let mut r1 = deterministic_receipt("mock");
        let mut r2 = deterministic_receipt("mock");
        r1.mode = ExecutionMode::Mapped;
        r2.mode = ExecutionMode::Passthrough;
        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }
}

// =========================================================================
// Module: receipt_self_referential_prevention
// =========================================================================
mod receipt_self_referential_prevention {
    use super::*;

    #[test]
    fn hash_ignores_existing_receipt_sha256() {
        let mut r = deterministic_receipt("mock");
        let h1 = receipt_hash(&r).unwrap();
        r.receipt_sha256 = Some("deadbeef".to_string());
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(
            h1, h2,
            "existing receipt_sha256 must be nulled before hashing"
        );
    }

    #[test]
    fn hash_same_whether_none_or_some() {
        let r_none = deterministic_receipt("mock");
        let mut r_some = deterministic_receipt("mock");
        r_some.receipt_sha256 = Some("anything".into());
        assert_eq!(
            receipt_hash(&r_none).unwrap(),
            receipt_hash(&r_some).unwrap()
        );
    }

    #[test]
    fn with_hash_idempotent_on_value() {
        let r1 = deterministic_receipt("mock").with_hash().unwrap();
        let mut r2 = deterministic_receipt("mock");
        r2.receipt_sha256 = Some("wrong".into());
        let r2 = r2.with_hash().unwrap();
        assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn canonical_json_of_receipt_nulls_sha256() {
        let mut r = deterministic_receipt("mock");
        r.receipt_sha256 = Some("pre-existing".into());
        let mut v = serde_json::to_value(&r).unwrap();
        if let serde_json::Value::Object(map) = &mut v {
            map.insert("receipt_sha256".into(), serde_json::Value::Null);
        }
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("\"receipt_sha256\":null"));
    }

    #[test]
    fn receipt_hash_consistent_with_manual_null() {
        let r = deterministic_receipt("mock");
        let auto_hash = receipt_hash(&r).unwrap();

        let mut v = serde_json::to_value(&r).unwrap();
        if let serde_json::Value::Object(map) = &mut v {
            map.insert("receipt_sha256".into(), serde_json::Value::Null);
        }
        let manual_json = serde_json::to_string(&v).unwrap();
        let manual_hash = sha256_hex(manual_json.as_bytes());
        assert_eq!(auto_hash, manual_hash);
    }

    #[test]
    fn hash_with_large_trace_still_nulls_sha256() {
        let mut r = deterministic_receipt("mock");
        for i in 0..100 {
            r.trace.push(make_event(AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            }));
        }
        r.receipt_sha256 = Some("should-be-ignored".into());
        let h1 = receipt_hash(&r).unwrap();
        r.receipt_sha256 = None;
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_with_populated_usage_ignores_sha256() {
        let mut r = deterministic_receipt("mock");
        r.usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        };
        let h1 = receipt_hash(&r).unwrap();
        r.receipt_sha256 = Some("ignored".into());
        let h2 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }
}

// =========================================================================
// Module: contract_version
// =========================================================================
mod contract_version {
    use super::*;

    #[test]
    fn contract_version_format() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn contract_version_parseable() {
        let parsed = parse_version(CONTRACT_VERSION);
        assert_eq!(parsed, Some((0, 1)));
    }

    #[test]
    fn receipt_builder_embeds_contract_version() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn hello_envelope_embeds_contract_version() {
        let env = make_hello();
        if let Envelope::Hello {
            contract_version, ..
        } = &env
        {
            assert_eq!(contract_version, CONTRACT_VERSION);
        } else {
            panic!("expected Hello");
        }
    }

    #[test]
    fn work_order_builder_builds_valid_task() {
        let wo = WorkOrderBuilder::new("test").build();
        assert_eq!(wo.task, "test");
    }

    #[test]
    fn deterministic_receipt_has_contract_version() {
        let r = deterministic_receipt("mock");
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn parse_version_valid() {
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
        assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
    }

    #[test]
    fn parse_version_invalid() {
        assert_eq!(parse_version("invalid"), None);
        assert_eq!(parse_version("abp/v"), None);
        assert_eq!(parse_version("abp/v0"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compatible_same_major() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    }

    #[test]
    fn incompatible_different_major() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
    }

    #[test]
    fn incompatible_invalid_strings() {
        assert!(!is_compatible_version("invalid", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "garbage"));
    }

    #[test]
    fn protocol_version_struct_parse() {
        let v = abp_protocol::version::ProtocolVersion::parse("abp/v0.1").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
    }

    #[test]
    fn protocol_version_current() {
        let v = abp_protocol::version::ProtocolVersion::current();
        assert_eq!(v.to_string(), CONTRACT_VERSION);
    }

    #[test]
    fn protocol_version_invalid() {
        assert!(abp_protocol::version::ProtocolVersion::parse("nope").is_err());
    }

    #[test]
    fn protocol_version_compatibility() {
        let v01 = abp_protocol::version::ProtocolVersion::parse("abp/v0.1").unwrap();
        let v02 = abp_protocol::version::ProtocolVersion::parse("abp/v0.2").unwrap();
        assert!(v01.is_compatible(&v02));
    }

    #[test]
    fn protocol_version_incompatibility() {
        let v01 = abp_protocol::version::ProtocolVersion::parse("abp/v0.1").unwrap();
        let v10 = abp_protocol::version::ProtocolVersion::parse("abp/v1.0").unwrap();
        assert!(!v01.is_compatible(&v10));
    }

    #[test]
    fn negotiate_version_same_major() {
        let v01 = abp_protocol::version::ProtocolVersion::parse("abp/v0.1").unwrap();
        let v02 = abp_protocol::version::ProtocolVersion::parse("abp/v0.2").unwrap();
        let result = abp_protocol::version::negotiate_version(&v01, &v02).unwrap();
        assert_eq!(result, v01);
    }

    #[test]
    fn negotiate_version_different_major_fails() {
        let v01 = abp_protocol::version::ProtocolVersion::parse("abp/v0.1").unwrap();
        let v10 = abp_protocol::version::ProtocolVersion::parse("abp/v1.0").unwrap();
        assert!(abp_protocol::version::negotiate_version(&v01, &v10).is_err());
    }
}

// =========================================================================
// Module: envelope_structure
// =========================================================================
mod envelope_structure {
    use super::*;

    #[test]
    fn hello_uses_t_tag() {
        let env = make_hello();
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"t\":\"hello\""));
    }

    #[test]
    fn run_uses_t_tag() {
        let wo = make_work_order();
        let env = Envelope::Run {
            id: "run-1".into(),
            work_order: wo,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"t\":\"run\""));
    }

    #[test]
    fn event_uses_t_tag() {
        let env = Envelope::Event {
            ref_id: "run-1".into(),
            event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"t\":\"event\""));
    }

    #[test]
    fn final_uses_t_tag() {
        let env = Envelope::Final {
            ref_id: "run-1".into(),
            receipt: deterministic_receipt("mock"),
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"t\":\"final\""));
    }

    #[test]
    fn fatal_uses_t_tag() {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "boom".into(),
            error_code: None,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"t\":\"fatal\""));
    }

    #[test]
    fn fatal_without_ref_id() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "early crash".into(),
            error_code: None,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"ref_id\":null"));
    }

    #[test]
    fn hello_contains_contract_version_field() {
        let env = make_hello();
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"contract_version\":\"abp/v0.1\""));
    }

    #[test]
    fn hello_contains_backend_field() {
        let env = make_hello();
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"backend\""));
    }

    #[test]
    fn hello_contains_capabilities_field() {
        let env = make_hello();
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"capabilities\""));
    }

    #[test]
    fn run_contains_id_field() {
        let wo = make_work_order();
        let env = Envelope::Run {
            id: "my-run".into(),
            work_order: wo,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"id\":\"my-run\""));
    }

    #[test]
    fn event_contains_ref_id() {
        let env = Envelope::Event {
            ref_id: "ref-123".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"ref_id\":\"ref-123\""));
    }

    #[test]
    fn final_contains_ref_id() {
        let env = Envelope::Final {
            ref_id: "ref-456".into(),
            receipt: deterministic_receipt("mock"),
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"ref_id\":\"ref-456\""));
    }

    #[test]
    fn envelope_not_using_type_tag() {
        let env = make_hello();
        let json = serde_json::to_string(&env).unwrap();
        // Envelope uses "t", NOT "type"
        assert!(!json.contains("\"type\":\"hello\""));
    }

    #[test]
    fn fatal_with_error_code_serializes() {
        let env = Envelope::fatal_with_code(
            Some("run-1".into()),
            "timeout",
            abp_error::ErrorCode::BackendTimeout,
        );
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"error_code\""));
        assert!(json.contains("backend_timeout"));
    }

    #[test]
    fn fatal_error_code_accessor() {
        let env =
            Envelope::fatal_with_code(None, "err", abp_error::ErrorCode::ProtocolInvalidEnvelope);
        assert_eq!(
            env.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    }

    #[test]
    fn non_fatal_error_code_is_none() {
        let env = make_hello();
        assert!(env.error_code().is_none());
    }

    #[test]
    fn hello_with_mode_passthrough() {
        let env = Envelope::hello_with_mode(
            BackendIdentity {
                id: "s".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
            ExecutionMode::Passthrough,
        );
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"mode\":\"passthrough\""));
    }

    #[test]
    fn hello_default_mode_is_mapped() {
        let env = make_hello();
        if let Envelope::Hello { mode, .. } = &env {
            assert_eq!(*mode, ExecutionMode::Mapped);
        } else {
            panic!("expected Hello");
        }
    }
}

// =========================================================================
// Module: agent_event_serde
// =========================================================================
mod agent_event_serde {
    use super::*;

    fn roundtrip(kind: AgentEventKind) {
        let event = make_event(kind);
        let json = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event.ts, back.ts);
    }

    #[test]
    fn run_started_roundtrip() {
        roundtrip(AgentEventKind::RunStarted {
            message: "go".into(),
        });
    }

    #[test]
    fn run_completed_roundtrip() {
        roundtrip(AgentEventKind::RunCompleted {
            message: "done".into(),
        });
    }

    #[test]
    fn assistant_delta_roundtrip() {
        roundtrip(AgentEventKind::AssistantDelta { text: "tok".into() });
    }

    #[test]
    fn assistant_message_roundtrip() {
        roundtrip(AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        });
    }

    #[test]
    fn tool_call_roundtrip() {
        roundtrip(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "/etc/passwd"}),
        });
    }

    #[test]
    fn tool_result_roundtrip() {
        roundtrip(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tu-1".into()),
            output: json!({"content": "data"}),
            is_error: false,
        });
    }

    #[test]
    fn tool_result_error_roundtrip() {
        roundtrip(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: None,
            output: json!("exit code 1"),
            is_error: true,
        });
    }

    #[test]
    fn file_changed_roundtrip() {
        roundtrip(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        });
    }

    #[test]
    fn command_executed_roundtrip() {
        roundtrip(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        });
    }

    #[test]
    fn warning_roundtrip() {
        roundtrip(AgentEventKind::Warning {
            message: "slow".into(),
        });
    }

    #[test]
    fn error_roundtrip() {
        roundtrip(AgentEventKind::Error {
            message: "crash".into(),
            error_code: None,
        });
    }

    #[test]
    fn error_with_code_roundtrip() {
        roundtrip(AgentEventKind::Error {
            message: "timeout".into(),
            error_code: Some(abp_error::ErrorCode::BackendTimeout),
        });
    }

    #[test]
    fn event_uses_type_tag_not_t() {
        let event = make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"run_started\""));
        // Events use "type", not "t"
    }

    #[test]
    fn assistant_delta_type_tag() {
        let event = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"assistant_delta\""));
    }

    #[test]
    fn tool_call_type_tag() {
        let event = make_event(AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"tool_call\""));
    }

    #[test]
    fn tool_result_type_tag() {
        let event = make_event(AgentEventKind::ToolResult {
            tool_name: "t".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
    }

    #[test]
    fn file_changed_type_tag() {
        let event = make_event(AgentEventKind::FileChanged {
            path: "a".into(),
            summary: "b".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"file_changed\""));
    }

    #[test]
    fn command_executed_type_tag() {
        let event = make_event(AgentEventKind::CommandExecuted {
            command: "ls".into(),
            exit_code: None,
            output_preview: None,
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"command_executed\""));
    }

    #[test]
    fn warning_type_tag() {
        let event = make_event(AgentEventKind::Warning {
            message: "w".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"warning\""));
    }

    #[test]
    fn error_type_tag() {
        let event = make_event(AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        });
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"error\""));
    }

    #[test]
    fn event_with_ext_roundtrip() {
        let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut ext = BTreeMap::new();
        ext.insert("raw_message".into(), json!({"vendor": "data"}));
        let event = AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage { text: "msg".into() },
            ext: Some(ext),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json).unwrap();
        assert!(back.ext.is_some());
        assert!(back.ext.unwrap().contains_key("raw_message"));
    }

    #[test]
    fn event_ext_none_omitted_in_json() {
        let event = make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        // ext: None uses skip_serializing_if = "Option::is_none"
        assert!(!json.contains("\"ext\""));
    }

    #[test]
    fn tool_call_with_parent_roundtrip() {
        roundtrip(AgentEventKind::ToolCall {
            tool_name: "inner".into(),
            tool_use_id: Some("tu-2".into()),
            parent_tool_use_id: Some("tu-1".into()),
            input: json!({}),
        });
    }

    #[test]
    fn command_executed_no_optionals_roundtrip() {
        roundtrip(AgentEventKind::CommandExecuted {
            command: "echo hi".into(),
            exit_code: None,
            output_preview: None,
        });
    }
}

// =========================================================================
// Module: jsonl_codec
// =========================================================================
mod jsonl_codec {
    use super::*;

    #[test]
    fn encode_ends_with_newline() {
        let env = make_hello();
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn encode_single_line() {
        let env = make_hello();
        let line = JsonlCodec::encode(&env).unwrap();
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn decode_roundtrip_hello() {
        let env = make_hello();
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn decode_roundtrip_fatal() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Fatal { error, .. } = decoded {
            assert_eq!(error, "boom");
        } else {
            panic!("expected Fatal");
        }
    }

    #[test]
    fn decode_invalid_json() {
        let err = JsonlCodec::decode("not json").unwrap_err();
        assert!(matches!(err, ProtocolError::Json(_)));
    }

    #[test]
    fn decode_stream_multiple_lines() {
        let e1 = Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        };
        let e2 = Envelope::Fatal {
            ref_id: None,
            error: "b".into(),
            error_code: None,
        };
        let mut buf = String::new();
        buf.push_str(&JsonlCodec::encode(&e1).unwrap());
        buf.push_str(&JsonlCodec::encode(&e2).unwrap());

        let reader = BufReader::new(buf.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn decode_stream_skips_blank_lines() {
        let e1 = JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        })
        .unwrap();
        let input = format!("{e1}\n\n{e1}");
        let reader = BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn encode_to_writer_works() {
        let env = make_hello();
        let mut buf = Vec::new();
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.ends_with('\n'));
        assert!(s.contains("\"t\":\"hello\""));
    }

    #[test]
    fn encode_many_to_writer_works() {
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
        let s = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn decode_roundtrip_event_envelope() {
        let env = Envelope::Event {
            ref_id: "r1".into(),
            event: make_event(AgentEventKind::AssistantMessage { text: "yo".into() }),
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let back = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(back, Envelope::Event { .. }));
    }

    #[test]
    fn decode_roundtrip_run_envelope() {
        let wo = make_work_order();
        let env = Envelope::Run {
            id: "run-x".into(),
            work_order: wo,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let back = JsonlCodec::decode(line.trim()).unwrap();
        if let Envelope::Run { id, .. } = back {
            assert_eq!(id, "run-x");
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn decode_roundtrip_final_envelope() {
        let env = Envelope::Final {
            ref_id: "r2".into(),
            receipt: deterministic_receipt("mock"),
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let back = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(back, Envelope::Final { .. }));
    }
}

// =========================================================================
// Module: work_order_validation
// =========================================================================
mod work_order_validation {
    use super::*;

    #[test]
    fn builder_creates_valid_work_order() {
        let wo = WorkOrderBuilder::new("do something").build();
        assert_eq!(wo.task, "do something");
        assert!(!wo.id.is_nil());
    }

    #[test]
    fn builder_default_lane_is_patch_first() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    }

    #[test]
    fn builder_default_workspace_mode_is_staged() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    }

    #[test]
    fn builder_default_root_is_dot() {
        let wo = WorkOrderBuilder::new("t").build();
        assert_eq!(wo.workspace.root, ".");
    }

    #[test]
    fn builder_set_lane() {
        let wo = WorkOrderBuilder::new("t")
            .lane(ExecutionLane::WorkspaceFirst)
            .build();
        assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    }

    #[test]
    fn builder_set_root() {
        let wo = WorkOrderBuilder::new("t").root("/tmp/ws").build();
        assert_eq!(wo.workspace.root, "/tmp/ws");
    }

    #[test]
    fn builder_set_workspace_mode() {
        let wo = WorkOrderBuilder::new("t")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    }

    #[test]
    fn builder_set_model() {
        let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn builder_set_max_turns() {
        let wo = WorkOrderBuilder::new("t").max_turns(5).build();
        assert_eq!(wo.config.max_turns, Some(5));
    }

    #[test]
    fn builder_set_max_budget() {
        let wo = WorkOrderBuilder::new("t").max_budget_usd(1.5).build();
        assert_eq!(wo.config.max_budget_usd, Some(1.5));
    }

    #[test]
    fn builder_set_include_exclude() {
        let wo = WorkOrderBuilder::new("t")
            .include(vec!["*.rs".into()])
            .exclude(vec!["target/".into()])
            .build();
        assert_eq!(wo.workspace.include, vec!["*.rs"]);
        assert_eq!(wo.workspace.exclude, vec!["target/"]);
    }

    #[test]
    fn builder_set_context() {
        let ctx = ContextPacket {
            files: vec!["main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "look here".into(),
            }],
        };
        let wo = WorkOrderBuilder::new("t").context(ctx).build();
        assert_eq!(wo.context.files.len(), 1);
        assert_eq!(wo.context.snippets.len(), 1);
    }

    #[test]
    fn builder_set_policy() {
        let policy = PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec![],
            deny_write: vec!["*.key".into()],
            allow_network: vec![],
            deny_network: vec!["*.evil.com".into()],
            require_approval_for: vec!["write".into()],
        };
        let wo = WorkOrderBuilder::new("t").policy(policy).build();
        assert_eq!(wo.policy.allowed_tools.len(), 1);
        assert_eq!(wo.policy.disallowed_tools.len(), 1);
    }

    #[test]
    fn builder_set_requirements() {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
        assert_eq!(wo.requirements.required.len(), 1);
    }

    #[test]
    fn builder_set_config() {
        let config = RuntimeConfig {
            model: Some("claude-4".into()),
            max_turns: Some(20),
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("t").config(config).build();
        assert_eq!(wo.config.model.as_deref(), Some("claude-4"));
        assert_eq!(wo.config.max_turns, Some(20));
    }

    #[test]
    fn work_order_serde_roundtrip() {
        let wo = WorkOrderBuilder::new("roundtrip test").build();
        let json = serde_json::to_string(&wo).unwrap();
        let back: WorkOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task, "roundtrip test");
        assert_eq!(back.id, wo.id);
    }

    #[test]
    fn work_order_unique_ids() {
        let wo1 = WorkOrderBuilder::new("a").build();
        let wo2 = WorkOrderBuilder::new("b").build();
        assert_ne!(wo1.id, wo2.id);
    }

    #[test]
    fn work_order_empty_task_serializes() {
        let wo = WorkOrderBuilder::new("").build();
        let json = serde_json::to_string(&wo).unwrap();
        assert!(json.contains("\"task\":\"\""));
    }
}

// =========================================================================
// Module: capability_consistency
// =========================================================================
mod capability_consistency {
    use super::*;

    #[test]
    fn capability_manifest_is_btreemap() {
        let manifest: CapabilityManifest = BTreeMap::new();
        assert!(manifest.is_empty());
    }

    #[test]
    fn capability_insert_and_retrieve() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        assert!(manifest.contains_key(&Capability::Streaming));
    }

    #[test]
    fn support_level_native_satisfies_native() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_level_native_satisfies_emulated() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_emulated_does_not_satisfy_native() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }

    #[test]
    fn support_level_emulated_satisfies_emulated() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_unsupported_satisfies_nothing() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_restricted_satisfies_emulated() {
        let restricted = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(restricted.satisfies(&MinSupport::Emulated));
    }

    #[test]
    fn support_level_restricted_not_satisfies_native() {
        let restricted = SupportLevel::Restricted {
            reason: "policy".into(),
        };
        assert!(!restricted.satisfies(&MinSupport::Native));
    }

    #[test]
    fn all_capabilities_serialize() {
        let caps = vec![
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
        for cap in &caps {
            let json = serde_json::to_string(cap).unwrap();
            let back: Capability = serde_json::from_str(&json).unwrap();
            assert_eq!(cap, &back);
        }
    }

    #[test]
    fn capability_serde_uses_snake_case() {
        let json = serde_json::to_string(&Capability::ToolRead).unwrap();
        assert_eq!(json, "\"tool_read\"");
    }

    #[test]
    fn capability_manifest_serde_roundtrip() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        manifest.insert(Capability::ToolBash, SupportLevel::Emulated);
        let json = serde_json::to_string(&manifest).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn capability_manifest_ordered_keys() {
        // BTreeMap uses derived Ord on Capability enum (variant declaration order):
        // Streaming (0) < ToolRead (1) < ToolWrite (2) < ... < ToolBash (4)
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::ToolWrite, SupportLevel::Native);
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        manifest.insert(Capability::ToolBash, SupportLevel::Native);
        let json = serde_json::to_string(&manifest).unwrap();
        let streaming_pos = json.find("streaming").unwrap();
        let write_pos = json.find("tool_write").unwrap();
        let bash_pos = json.find("tool_bash").unwrap();
        assert!(streaming_pos < write_pos);
        assert!(write_pos < bash_pos);
    }

    #[test]
    fn support_level_serde_roundtrip() {
        for sl in [
            SupportLevel::Native,
            SupportLevel::Emulated,
            SupportLevel::Unsupported,
            SupportLevel::Restricted {
                reason: "test".into(),
            },
        ] {
            let json = serde_json::to_string(&sl).unwrap();
            let _back: SupportLevel = serde_json::from_str(&json).unwrap();
        }
    }
}

// =========================================================================
// Module: sidecar_handshake
// =========================================================================
mod sidecar_handshake {
    use super::*;
    use abp_protocol::validate::{EnvelopeValidator, SequenceError};

    #[test]
    fn valid_sequence_no_errors() {
        let hello = make_hello();
        let wo = make_work_order();
        let run_id = wo.id.to_string();
        let run = Envelope::Run {
            id: run_id.clone(),
            work_order: wo,
        };
        let event = Envelope::Event {
            ref_id: run_id.clone(),
            event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        };
        let final_env = Envelope::Final {
            ref_id: run_id,
            receipt: deterministic_receipt("mock"),
        };
        let seq = vec![hello, run, event, final_env];
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&seq);
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn missing_hello_detected() {
        let run = Envelope::Run {
            id: "r".into(),
            work_order: make_work_order(),
        };
        let final_env = Envelope::Final {
            ref_id: "r".into(),
            receipt: deterministic_receipt("mock"),
        };
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[run, final_env]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingHello))
        );
    }

    #[test]
    fn hello_not_first_detected() {
        let run = Envelope::Run {
            id: "r".into(),
            work_order: make_work_order(),
        };
        let hello = make_hello();
        let final_env = Envelope::Final {
            ref_id: "r".into(),
            receipt: deterministic_receipt("mock"),
        };
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[run, hello, final_env]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::HelloNotFirst { .. }))
        );
    }

    #[test]
    fn missing_terminal_detected() {
        let hello = make_hello();
        let run = Envelope::Run {
            id: "r".into(),
            work_order: make_work_order(),
        };
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[hello, run]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingTerminal))
        );
    }

    #[test]
    fn empty_sequence_errors() {
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingHello))
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::MissingTerminal))
        );
    }

    #[test]
    fn ref_id_mismatch_detected() {
        let hello = make_hello();
        let run = Envelope::Run {
            id: "run-1".into(),
            work_order: make_work_order(),
        };
        let event = Envelope::Event {
            ref_id: "wrong-id".into(),
            event: make_event(AgentEventKind::AssistantMessage { text: "hi".into() }),
        };
        let final_env = Envelope::Final {
            ref_id: "run-1".into(),
            receipt: deterministic_receipt("mock"),
        };
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[hello, run, event, final_env]);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, SequenceError::RefIdMismatch { .. }))
        );
    }

    #[test]
    fn fatal_as_terminal_is_valid() {
        let hello = make_hello();
        let run = Envelope::Run {
            id: "r".into(),
            work_order: make_work_order(),
        };
        let fatal = Envelope::Fatal {
            ref_id: Some("r".into()),
            error: "crash".into(),
            error_code: None,
        };
        let validator = EnvelopeValidator::new();
        let errors = validator.validate_sequence(&[hello, run, fatal]);
        let relevant: Vec<_> = errors
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    SequenceError::MissingTerminal | SequenceError::MissingHello
                )
            })
            .collect();
        assert!(relevant.is_empty());
    }

    #[test]
    fn validate_hello_envelope_valid() {
        let hello = make_hello();
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&hello);
        assert!(result.valid);
    }

    #[test]
    fn validate_hello_empty_backend_id() {
        let env = Envelope::Hello {
            contract_version: CONTRACT_VERSION.to_string(),
            backend: BackendIdentity {
                id: "".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn validate_hello_invalid_version() {
        let env = Envelope::Hello {
            contract_version: "invalid".to_string(),
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn validate_run_empty_id() {
        let env = Envelope::Run {
            id: "".into(),
            work_order: make_work_order(),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn validate_fatal_empty_error() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "".into(),
            error_code: None,
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn validate_event_empty_ref_id() {
        let env = Envelope::Event {
            ref_id: "".into(),
            event: make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
    }

    #[test]
    fn validate_final_empty_ref_id() {
        let env = Envelope::Final {
            ref_id: "".into(),
            receipt: deterministic_receipt("mock"),
        };
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
    }
}

// =========================================================================
// Module: error_taxonomy
// =========================================================================
mod error_taxonomy {
    use super::*;
    use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};

    #[test]
    fn all_protocol_codes_map_to_protocol_category() {
        let codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::ProtocolUnexpectedMessage,
            ErrorCode::ProtocolVersionMismatch,
        ];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Protocol);
        }
    }

    #[test]
    fn all_backend_codes_map_to_backend_category() {
        let codes = [
            ErrorCode::BackendNotFound,
            ErrorCode::BackendTimeout,
            ErrorCode::BackendCrashed,
        ];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Backend);
        }
    }

    #[test]
    fn all_capability_codes_map_to_capability_category() {
        let codes = [
            ErrorCode::CapabilityUnsupported,
            ErrorCode::CapabilityEmulationFailed,
        ];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Capability);
        }
    }

    #[test]
    fn all_policy_codes_map_to_policy_category() {
        let codes = [ErrorCode::PolicyDenied, ErrorCode::PolicyInvalid];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Policy);
        }
    }

    #[test]
    fn all_workspace_codes_map_to_workspace_category() {
        let codes = [
            ErrorCode::WorkspaceInitFailed,
            ErrorCode::WorkspaceStagingFailed,
        ];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Workspace);
        }
    }

    #[test]
    fn all_ir_codes_map_to_ir_category() {
        let codes = [ErrorCode::IrLoweringFailed, ErrorCode::IrInvalid];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Ir);
        }
    }

    #[test]
    fn all_receipt_codes_map_to_receipt_category() {
        let codes = [
            ErrorCode::ReceiptHashMismatch,
            ErrorCode::ReceiptChainBroken,
        ];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Receipt);
        }
    }

    #[test]
    fn all_dialect_codes_map_to_dialect_category() {
        let codes = [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed];
        for code in &codes {
            assert_eq!(code.category(), ErrorCategory::Dialect);
        }
    }

    #[test]
    fn config_code_maps_to_config_category() {
        assert_eq!(ErrorCode::ConfigInvalid.category(), ErrorCategory::Config);
    }

    #[test]
    fn internal_code_maps_to_internal_category() {
        assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
    }

    #[test]
    fn error_code_as_str_screaming_snake() {
        assert_eq!(ErrorCode::BackendTimeout.as_str(), "backend_timeout");
        assert_eq!(
            ErrorCode::ProtocolInvalidEnvelope.as_str(),
            "protocol_invalid_envelope"
        );
    }

    #[test]
    fn error_code_display_matches_as_str() {
        let code = ErrorCode::PolicyDenied;
        assert_eq!(code.to_string(), code.message());
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let code = ErrorCode::BackendCrashed;
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(json, "\"backend_crashed\"");
        let back: ErrorCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, code);
    }

    #[test]
    fn abp_error_construction() {
        let err = AbpError::new(ErrorCode::Internal, "boom");
        assert_eq!(err.code, ErrorCode::Internal);
        assert_eq!(err.message, "boom");
    }

    #[test]
    fn abp_error_with_context() {
        let err = AbpError::new(ErrorCode::BackendTimeout, "timed out").with_context("ms", 5000);
        assert_eq!(err.context.len(), 1);
    }

    #[test]
    fn abp_error_display() {
        let err = AbpError::new(ErrorCode::PolicyDenied, "denied");
        assert_eq!(err.to_string(), "[policy_denied] denied");
    }

    #[test]
    fn abp_error_dto_roundtrip() {
        let err = AbpError::new(ErrorCode::ConfigInvalid, "bad config");
        let dto: AbpErrorDto = (&err).into();
        let json = serde_json::to_string(&dto).unwrap();
        let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
    }

    #[test]
    fn abp_error_category_shorthand() {
        let err = AbpError::new(ErrorCode::IrInvalid, "bad ir");
        assert_eq!(err.category(), ErrorCategory::Ir);
    }

    #[test]
    fn protocol_error_carries_error_code() {
        let pe = ProtocolError::Violation("bad".into());
        assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
    }

    #[test]
    fn protocol_error_unexpected_message_code() {
        let pe = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        };
        assert_eq!(pe.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
    }

    #[test]
    fn protocol_error_json_has_no_code() {
        let pe: ProtocolError = serde_json::from_str::<serde_json::Value>("bad")
            .unwrap_err()
            .into();
        assert!(pe.error_code().is_none());
    }

    #[test]
    fn protocol_error_abp_carries_code() {
        let abp_err = AbpError::new(ErrorCode::BackendNotFound, "not found");
        let pe = ProtocolError::from(abp_err);
        assert_eq!(pe.error_code(), Some(ErrorCode::BackendNotFound));
    }

    #[test]
    fn error_category_display() {
        assert_eq!(ErrorCategory::Protocol.to_string(), "protocol");
        assert_eq!(ErrorCategory::Backend.to_string(), "backend");
        assert_eq!(ErrorCategory::Capability.to_string(), "capability");
        assert_eq!(ErrorCategory::Policy.to_string(), "policy");
        assert_eq!(ErrorCategory::Workspace.to_string(), "workspace");
        assert_eq!(ErrorCategory::Ir.to_string(), "ir");
        assert_eq!(ErrorCategory::Receipt.to_string(), "receipt");
        assert_eq!(ErrorCategory::Dialect.to_string(), "dialect");
        assert_eq!(ErrorCategory::Config.to_string(), "config");
        assert_eq!(ErrorCategory::Internal.to_string(), "internal");
    }
}

// =========================================================================
// Module: btreemap_ordering
// =========================================================================
mod btreemap_ordering {
    use super::*;

    #[test]
    fn canonical_json_sorted_keys() {
        let json = canonical_json(&json!({"z": 1, "a": 2, "m": 3})).unwrap();
        let a_pos = json.find("\"a\"").unwrap();
        let m_pos = json.find("\"m\"").unwrap();
        let z_pos = json.find("\"z\"").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn canonical_json_deterministic() {
        let val = json!({"b": 2, "a": 1});
        let j1 = canonical_json(&val).unwrap();
        let j2 = canonical_json(&val).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn runtime_config_vendor_btreemap_ordered() {
        let mut vendor = BTreeMap::new();
        vendor.insert("z_key".to_string(), json!("z"));
        vendor.insert("a_key".to_string(), json!("a"));
        vendor.insert("m_key".to_string(), json!("m"));
        let config = RuntimeConfig {
            vendor,
            ..RuntimeConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let a_pos = json.find("a_key").unwrap();
        let m_pos = json.find("m_key").unwrap();
        let z_pos = json.find("z_key").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn runtime_config_env_btreemap_ordered() {
        let mut env = BTreeMap::new();
        env.insert("Z_VAR".to_string(), "z".into());
        env.insert("A_VAR".to_string(), "a".into());
        let config = RuntimeConfig {
            env,
            ..RuntimeConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let a_pos = json.find("A_VAR").unwrap();
        let z_pos = json.find("Z_VAR").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn capability_manifest_btreemap_ordering() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::ToolWrite, SupportLevel::Native);
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        let json = serde_json::to_string(&manifest).unwrap();
        let streaming_pos = json.find("streaming").unwrap();
        let write_pos = json.find("tool_write").unwrap();
        assert!(streaming_pos < write_pos);
    }

    #[test]
    fn receipt_capabilities_ordered() {
        let mut r = deterministic_receipt("mock");
        r.capabilities
            .insert(Capability::ToolBash, SupportLevel::Native);
        r.capabilities
            .insert(Capability::Streaming, SupportLevel::Native);
        let json = serde_json::to_string(&r).unwrap();
        let streaming_pos = json.find("streaming").unwrap();
        let bash_pos = json.find("tool_bash").unwrap();
        assert!(streaming_pos < bash_pos);
    }

    #[test]
    fn canonical_json_nested_objects_sorted() {
        let val = json!({"outer": {"z": 1, "a": 2}});
        let json = canonical_json(&val).unwrap();
        let a_pos = json.find("\"a\"").unwrap();
        let z_pos = json.find("\"z\"").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn receipt_hash_depends_on_btreemap_order() {
        let mut r1 = deterministic_receipt("mock");
        let mut r2 = deterministic_receipt("mock");
        r1.capabilities
            .insert(Capability::Streaming, SupportLevel::Native);
        r1.capabilities
            .insert(Capability::ToolRead, SupportLevel::Native);
        r2.capabilities
            .insert(Capability::ToolRead, SupportLevel::Native);
        r2.capabilities
            .insert(Capability::Streaming, SupportLevel::Native);
        // BTreeMap means insertion order doesn't matter
        assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn canonical_json_array_preserved() {
        let val = json!({"arr": [3, 1, 2]});
        let json = canonical_json(&val).unwrap();
        assert!(json.contains("[3,1,2]"));
    }

    #[test]
    fn canonical_json_empty_object() {
        let json = canonical_json(&json!({})).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn canonical_json_null_value() {
        let json = canonical_json(&json!(null)).unwrap();
        assert_eq!(json, "null");
    }
}

// =========================================================================
// Module: receipt_builder
// =========================================================================
mod receipt_builder_tests {
    use super::*;

    #[test]
    fn builder_default_outcome_complete() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn builder_set_outcome() {
        let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        assert_eq!(r.outcome, Outcome::Failed);
    }

    #[test]
    fn builder_set_backend_id() {
        let r = ReceiptBuilder::new("initial")
            .backend_id("override")
            .build();
        assert_eq!(r.backend.id, "override");
    }

    #[test]
    fn builder_set_backend_version() {
        let r = ReceiptBuilder::new("mock").backend_version("2.0").build();
        assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
    }

    #[test]
    fn builder_set_adapter_version() {
        let r = ReceiptBuilder::new("mock").adapter_version("1.0").build();
        assert_eq!(r.backend.adapter_version.as_deref(), Some("1.0"));
    }

    #[test]
    fn builder_set_mode() {
        let r = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(r.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn builder_default_mode_mapped() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn builder_add_trace_event() {
        let r = ReceiptBuilder::new("mock")
            .add_trace_event(make_event(AgentEventKind::RunStarted {
                message: "go".into(),
            }))
            .build();
        assert_eq!(r.trace.len(), 1);
    }

    #[test]
    fn builder_add_artifact() {
        let r = ReceiptBuilder::new("mock")
            .add_artifact(ArtifactRef {
                kind: "log".into(),
                path: "out.log".into(),
            })
            .build();
        assert_eq!(r.artifacts.len(), 1);
    }

    #[test]
    fn builder_set_capabilities() {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        let r = ReceiptBuilder::new("mock").capabilities(caps).build();
        assert_eq!(r.capabilities.len(), 1);
    }

    #[test]
    fn builder_set_work_order_id() {
        let id = Uuid::new_v4();
        let r = ReceiptBuilder::new("mock").work_order_id(id).build();
        assert_eq!(r.meta.work_order_id, id);
    }

    #[test]
    fn builder_set_usage_raw() {
        let r = ReceiptBuilder::new("mock")
            .usage_raw(json!({"tokens": 100}))
            .build();
        assert_eq!(r.usage_raw, json!({"tokens": 100}));
    }

    #[test]
    fn builder_set_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(25),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        };
        let r = ReceiptBuilder::new("mock").usage(usage).build();
        assert_eq!(r.usage.input_tokens, Some(50));
    }

    #[test]
    fn builder_set_verification() {
        let v = VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        };
        let r = ReceiptBuilder::new("mock").verification(v).build();
        assert!(r.verification.harness_ok);
        assert!(r.verification.git_diff.is_some());
    }

    #[test]
    fn builder_receipt_sha256_is_none() {
        let r = ReceiptBuilder::new("mock").build();
        assert!(r.receipt_sha256.is_none());
    }

    #[test]
    fn builder_with_hash_produces_sha256() {
        let r = ReceiptBuilder::new("mock").with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
    }

    #[test]
    fn builder_contract_version_in_meta() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn builder_timestamps_set() {
        let ts = chrono::DateTime::parse_from_rfc3339("2025-06-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let r = ReceiptBuilder::new("mock")
            .started_at(ts)
            .finished_at(ts)
            .build();
        assert_eq!(r.meta.started_at, ts);
        assert_eq!(r.meta.finished_at, ts);
    }

    #[test]
    fn builder_duration_computed() {
        let start = chrono::DateTime::parse_from_rfc3339("2025-06-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = chrono::DateTime::parse_from_rfc3339("2025-06-01T12:00:01Z")
            .unwrap()
            .with_timezone(&Utc);
        let r = ReceiptBuilder::new("mock")
            .started_at(start)
            .finished_at(end)
            .build();
        assert_eq!(r.meta.duration_ms, 1000);
    }
}

// =========================================================================
// Module: outcome_serde
// =========================================================================
mod outcome_serde {
    use super::*;

    #[test]
    fn outcome_complete_serde() {
        let json = serde_json::to_string(&Outcome::Complete).unwrap();
        assert_eq!(json, "\"complete\"");
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Outcome::Complete);
    }

    #[test]
    fn outcome_partial_serde() {
        let json = serde_json::to_string(&Outcome::Partial).unwrap();
        assert_eq!(json, "\"partial\"");
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Outcome::Partial);
    }

    #[test]
    fn outcome_failed_serde() {
        let json = serde_json::to_string(&Outcome::Failed).unwrap();
        assert_eq!(json, "\"failed\"");
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Outcome::Failed);
    }
}

// =========================================================================
// Module: execution_mode
// =========================================================================
mod execution_mode_tests {
    use super::*;

    #[test]
    fn default_is_mapped() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
    }

    #[test]
    fn mapped_serde() {
        let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
        assert_eq!(json, "\"mapped\"");
    }

    #[test]
    fn passthrough_serde() {
        let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
        assert_eq!(json, "\"passthrough\"");
    }

    #[test]
    fn mapped_roundtrip() {
        let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ExecutionMode::Mapped);
    }

    #[test]
    fn passthrough_roundtrip() {
        let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
        let back: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ExecutionMode::Passthrough);
    }
}

// =========================================================================
// Module: stream_parser
// =========================================================================
mod stream_parser {
    use super::*;
    use abp_protocol::stream::StreamParser;

    #[test]
    fn empty_push_returns_nothing() {
        let mut parser = StreamParser::new();
        let results = parser.push(b"");
        assert!(results.is_empty());
    }

    #[test]
    fn partial_line_buffered() {
        let mut parser = StreamParser::new();
        let results = parser.push(b"{\"t\":\"fatal\"");
        assert!(results.is_empty());
        assert!(!parser.is_empty());
    }

    #[test]
    fn complete_line_parsed() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        })
        .unwrap();
        let results = parser.push(line.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn split_across_pushes() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "boom".into(),
            error_code: None,
        })
        .unwrap();
        let bytes = line.as_bytes();
        let (first, second) = bytes.split_at(10);
        assert!(parser.push(first).is_empty());
        let results = parser.push(second);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn finish_flushes_remaining() {
        let mut parser = StreamParser::new();
        // Push a line without trailing newline
        parser.push(b"{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"x\"}");
        assert!(parser.is_empty() || parser.buffered_len() > 0);
        let results = parser.finish();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn reset_clears_buffer() {
        let mut parser = StreamParser::new();
        parser.push(b"partial data");
        parser.reset();
        assert!(parser.is_empty());
        assert_eq!(parser.buffered_len(), 0);
    }

    #[test]
    fn blank_lines_skipped() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "x".into(),
            error_code: None,
        })
        .unwrap();
        let input = format!("\n\n{line}\n\n");
        let results = parser.push(input.as_bytes());
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn multiple_lines_in_one_push() {
        let mut parser = StreamParser::new();
        let line = JsonlCodec::encode(&Envelope::Fatal {
            ref_id: None,
            error: "a".into(),
            error_code: None,
        })
        .unwrap();
        let input = format!("{line}{line}{line}");
        let results = parser.push(input.as_bytes());
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn max_line_len_enforced() {
        let mut parser = StreamParser::with_max_line_len(10);
        let long_line = format!(
            "{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{}\"}}\n",
            "x".repeat(100)
        );
        let results = parser.push(long_line.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }
}

// =========================================================================
// Module: envelope_builder
// =========================================================================
mod envelope_builder_tests {
    use super::*;
    use abp_protocol::builder::EnvelopeBuilder;

    #[test]
    fn hello_builder_minimal() {
        let env = EnvelopeBuilder::hello().backend("test").build().unwrap();
        assert!(matches!(env, Envelope::Hello { .. }));
    }

    #[test]
    fn hello_builder_missing_backend_errors() {
        let err = EnvelopeBuilder::hello().build().unwrap_err();
        assert_eq!(
            err,
            abp_protocol::builder::BuilderError::MissingField("backend")
        );
    }

    #[test]
    fn hello_builder_sets_contract_version() {
        let env = EnvelopeBuilder::hello().backend("s").build().unwrap();
        if let Envelope::Hello {
            contract_version, ..
        } = &env
        {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
    }

    #[test]
    fn run_builder_uses_work_order_id() {
        let wo = make_work_order();
        let expected_id = wo.id.to_string();
        let env = EnvelopeBuilder::run(wo).build().unwrap();
        if let Envelope::Run { id, .. } = &env {
            assert_eq!(id, &expected_id);
        }
    }

    #[test]
    fn run_builder_override_ref_id() {
        let wo = make_work_order();
        let env = EnvelopeBuilder::run(wo)
            .ref_id("custom-id")
            .build()
            .unwrap();
        if let Envelope::Run { id, .. } = &env {
            assert_eq!(id, "custom-id");
        }
    }

    #[test]
    fn event_builder_requires_ref_id() {
        let event = make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        });
        let err = EnvelopeBuilder::event(event).build().unwrap_err();
        assert_eq!(
            err,
            abp_protocol::builder::BuilderError::MissingField("ref_id")
        );
    }

    #[test]
    fn event_builder_with_ref_id() {
        let event = make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        });
        let env = EnvelopeBuilder::event(event).ref_id("r1").build().unwrap();
        assert!(matches!(env, Envelope::Event { .. }));
    }

    #[test]
    fn final_builder_requires_ref_id() {
        let receipt = deterministic_receipt("mock");
        let err = EnvelopeBuilder::final_receipt(receipt).build().unwrap_err();
        assert_eq!(
            err,
            abp_protocol::builder::BuilderError::MissingField("ref_id")
        );
    }

    #[test]
    fn final_builder_with_ref_id() {
        let receipt = deterministic_receipt("mock");
        let env = EnvelopeBuilder::final_receipt(receipt)
            .ref_id("r1")
            .build()
            .unwrap();
        assert!(matches!(env, Envelope::Final { .. }));
    }

    #[test]
    fn fatal_builder_no_ref_id() {
        let env = EnvelopeBuilder::fatal("crash").build().unwrap();
        if let Envelope::Fatal { ref_id, error, .. } = &env {
            assert!(ref_id.is_none());
            assert_eq!(error, "crash");
        }
    }

    #[test]
    fn fatal_builder_with_ref_id() {
        let env = EnvelopeBuilder::fatal("crash")
            .ref_id("r1")
            .build()
            .unwrap();
        if let Envelope::Fatal { ref_id, .. } = &env {
            assert_eq!(ref_id.as_deref(), Some("r1"));
        }
    }
}

// =========================================================================
// Module: misc_conformance
// =========================================================================
mod misc_conformance {
    use super::*;

    #[test]
    fn receipt_serde_roundtrip() {
        let r = deterministic_receipt("mock").with_hash().unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let back: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(back.receipt_sha256, r.receipt_sha256);
        assert_eq!(back.outcome, r.outcome);
        assert_eq!(back.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn backend_identity_serde_roundtrip() {
        let bi = BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.5.0".into()),
        };
        let json = serde_json::to_string(&bi).unwrap();
        let back: BackendIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "sidecar:node");
        assert_eq!(back.backend_version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn workspace_spec_serde_roundtrip() {
        let ws = WorkspaceSpec {
            root: "/tmp".into(),
            mode: WorkspaceMode::Staged,
            include: vec!["*.rs".into()],
            exclude: vec!["target/".into()],
        };
        let json = serde_json::to_string(&ws).unwrap();
        let back: WorkspaceSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.root, "/tmp");
    }

    #[test]
    fn context_packet_serde_roundtrip() {
        let cp = ContextPacket {
            files: vec!["main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "use async".into(),
            }],
        };
        let json = serde_json::to_string(&cp).unwrap();
        let back: ContextPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(back.files.len(), 1);
        assert_eq!(back.snippets.len(), 1);
    }

    #[test]
    fn policy_profile_serde_roundtrip() {
        let pp = PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec![],
            deny_read: vec![],
            deny_write: vec!["*.key".into()],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        };
        let json = serde_json::to_string(&pp).unwrap();
        let back: PolicyProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.allowed_tools, vec!["read"]);
    }

    #[test]
    fn usage_normalized_defaults() {
        let u = UsageNormalized::default();
        assert!(u.input_tokens.is_none());
        assert!(u.output_tokens.is_none());
        assert!(u.estimated_cost_usd.is_none());
    }

    #[test]
    fn verification_report_defaults() {
        let v = VerificationReport::default();
        assert!(v.git_diff.is_none());
        assert!(v.git_status.is_none());
        assert!(!v.harness_ok);
    }

    #[test]
    fn artifact_ref_serde_roundtrip() {
        let ar = ArtifactRef {
            kind: "patch".into(),
            path: "fix.patch".into(),
        };
        let json = serde_json::to_string(&ar).unwrap();
        let back: ArtifactRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, "patch");
        assert_eq!(back.path, "fix.patch");
    }

    #[test]
    fn execution_lane_serde() {
        let json = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
        assert_eq!(json, "\"patch_first\"");
        let json = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
        assert_eq!(json, "\"workspace_first\"");
    }

    #[test]
    fn workspace_mode_serde() {
        let json = serde_json::to_string(&WorkspaceMode::PassThrough).unwrap();
        assert_eq!(json, "\"pass_through\"");
        let json = serde_json::to_string(&WorkspaceMode::Staged).unwrap();
        assert_eq!(json, "\"staged\"");
    }

    #[test]
    fn contract_error_from_json_error() {
        let result: Result<serde_json::Value, _> = serde_json::from_str("bad");
        let json_err = result.unwrap_err();
        let contract_err: ContractError = json_err.into();
        assert!(matches!(contract_err, ContractError::Json(_)));
    }

    #[test]
    fn run_metadata_serde_roundtrip() {
        let ts = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let meta = RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 42,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: RunMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.duration_ms, 42);
        assert_eq!(back.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn min_support_serde_roundtrip() {
        let native_json = serde_json::to_string(&MinSupport::Native).unwrap();
        assert_eq!(native_json, "\"native\"");
        let emulated_json = serde_json::to_string(&MinSupport::Emulated).unwrap();
        assert_eq!(emulated_json, "\"emulated\"");
    }

    #[test]
    fn capability_requirement_serde_roundtrip() {
        let req = CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: CapabilityRequirement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.capability, Capability::Streaming);
    }
}
