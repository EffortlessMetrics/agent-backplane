#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

//! Comprehensive backward & forward compatibility tests for the Agent Backplane.
//!
//! Covers: forward-compat extra fields, contract version handling, JSON schema
//! evolution, JSONL protocol evolution, type coercion flexibility, and
//! interoperability.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode, Outcome,
    PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode, WorkspaceSpec,
    CONTRACT_VERSION,
};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec, ProtocolError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

// ============================================================================
// Helpers
// ============================================================================

/// Builds a minimal valid WorkOrder JSON object.
fn minimal_work_order_json() -> Value {
    let wo = WorkOrderBuilder::new("test task").build();
    serde_json::to_value(&wo).unwrap()
}

/// Builds a minimal valid Receipt JSON object.
fn minimal_receipt_json() -> Value {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    serde_json::to_value(&receipt).unwrap()
}

/// Builds a minimal valid Envelope::Hello JSON.
fn minimal_hello_json() -> Value {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    serde_json::to_value(&env).unwrap()
}

/// Builds a minimal AgentEvent JSON.
fn minimal_agent_event_json() -> Value {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    serde_json::to_value(&event).unwrap()
}

/// Builds a minimal Envelope::Fatal JSON.
fn minimal_fatal_json() -> Value {
    json!({
        "t": "fatal",
        "ref_id": null,
        "error": "boom"
    })
}

/// Builds a minimal Envelope::Run JSON.
fn minimal_run_json() -> Value {
    let wo = WorkOrderBuilder::new("test").build();
    json!({
        "t": "run",
        "id": Uuid::new_v4().to_string(),
        "work_order": serde_json::to_value(&wo).unwrap()
    })
}

/// Builds a minimal Envelope::Event JSON.
fn minimal_event_envelope_json() -> Value {
    json!({
        "t": "event",
        "ref_id": "run-1",
        "event": {
            "ts": Utc::now().to_rfc3339(),
            "type": "assistant_message",
            "text": "hello"
        }
    })
}

/// Builds a minimal Envelope::Final JSON.
fn minimal_final_json() -> Value {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    json!({
        "t": "final",
        "ref_id": "run-1",
        "receipt": serde_json::to_value(&receipt).unwrap()
    })
}

// ============================================================================
// Module 1: Forward Compatibility — unknown / extra fields (25+ tests)
// ============================================================================

mod forward_compat {
    use super::*;

    #[test]
    fn work_order_with_extra_top_level_field() {
        let mut j = minimal_work_order_json();
        j["future_field"] = json!("some value");
        let wo: WorkOrder = serde_json::from_value(j).unwrap();
        assert_eq!(wo.task, "test task");
    }

    #[test]
    fn work_order_with_extra_nested_field_in_workspace() {
        let mut j = minimal_work_order_json();
        j["workspace"]["future_optimization"] = json!(true);
        let wo: WorkOrder = serde_json::from_value(j).unwrap();
        assert_eq!(wo.workspace.root, ".");
    }

    #[test]
    fn work_order_with_extra_nested_field_in_config() {
        let mut j = minimal_work_order_json();
        j["config"]["future_param"] = json!(42);
        let wo: WorkOrder = serde_json::from_value(j).unwrap();
        assert!(wo.config.model.is_none());
    }

    #[test]
    fn work_order_with_extra_nested_field_in_policy() {
        let mut j = minimal_work_order_json();
        j["policy"]["future_governance_rules"] = json!(["rule1"]);
        let wo: WorkOrder = serde_json::from_value(j).unwrap();
        assert!(wo.policy.allowed_tools.is_empty());
    }

    #[test]
    fn work_order_with_extra_nested_field_in_context() {
        let mut j = minimal_work_order_json();
        j["context"]["embeddings"] = json!([1.0, 2.0, 3.0]);
        let wo: WorkOrder = serde_json::from_value(j).unwrap();
        assert!(wo.context.files.is_empty());
    }

    #[test]
    fn receipt_with_extra_top_level_field() {
        let mut j = minimal_receipt_json();
        j["future_audit_trail"] = json!({"signed_by": "admin"});
        let r: Receipt = serde_json::from_value(j).unwrap();
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_with_extra_nested_field_in_meta() {
        let mut j = minimal_receipt_json();
        j["meta"]["correlation_id"] = json!("abc-123");
        let r: Receipt = serde_json::from_value(j).unwrap();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_with_extra_nested_field_in_backend() {
        let mut j = minimal_receipt_json();
        j["backend"]["region"] = json!("us-east-1");
        let r: Receipt = serde_json::from_value(j).unwrap();
        assert_eq!(r.backend.id, "mock");
    }

    #[test]
    fn receipt_with_extra_nested_field_in_verification() {
        let mut j = minimal_receipt_json();
        j["verification"]["code_coverage_pct"] = json!(87.5);
        let r: Receipt = serde_json::from_value(j).unwrap();
        assert!(!r.verification.harness_ok);
    }

    #[test]
    fn receipt_with_extra_nested_field_in_usage() {
        let mut j = minimal_receipt_json();
        j["usage"]["reasoning_tokens"] = json!(500);
        let r: Receipt = serde_json::from_value(j).unwrap();
        assert!(r.usage.input_tokens.is_none());
    }

    #[test]
    fn envelope_hello_with_extra_fields() {
        let mut j = minimal_hello_json();
        j["extensions"] = json!({"custom": true});
        j["session_id"] = json!("sess-1");
        let env: Envelope = serde_json::from_value(j).unwrap();
        assert!(matches!(env, Envelope::Hello { .. }));
    }

    #[test]
    fn envelope_fatal_with_extra_fields() {
        let mut j = minimal_fatal_json();
        j["stack_trace"] = json!("at line 42");
        j["retry_after_ms"] = json!(5000);
        let env: Envelope = serde_json::from_value(j).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn envelope_run_with_extra_fields() {
        let mut j = minimal_run_json();
        j["priority"] = json!("high");
        let env: Envelope = serde_json::from_value(j).unwrap();
        assert!(matches!(env, Envelope::Run { .. }));
    }

    #[test]
    fn envelope_event_with_extra_fields() {
        let mut j = minimal_event_envelope_json();
        j["sequence_number"] = json!(42);
        let env: Envelope = serde_json::from_value(j).unwrap();
        assert!(matches!(env, Envelope::Event { .. }));
    }

    #[test]
    fn envelope_final_with_extra_fields() {
        let mut j = minimal_final_json();
        j["checksum"] = json!("abc123");
        let env: Envelope = serde_json::from_value(j).unwrap();
        assert!(matches!(env, Envelope::Final { .. }));
    }

    #[test]
    fn agent_event_with_extra_fields_in_ext() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "assistant_message",
            "text": "hi",
            "ext_future_field": "value"
        });
        // Extra fields at the top level may be captured in serde's flatten
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        assert!(matches!(
            event.kind,
            AgentEventKind::AssistantMessage { .. }
        ));
    }

    #[test]
    fn unknown_envelope_tag_value_errors() {
        let j = json!({"t": "unknown_future_type", "data": 42});
        let result = serde_json::from_value::<Envelope>(j);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_envelope_tag_value_via_codec_errors() {
        let line = r#"{"t":"stream_checkpoint","offset":100}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err());
    }

    #[test]
    fn config_with_unknown_vendor_keys() {
        let j = json!({
            "model": "gpt-5",
            "vendor": {
                "openai": {"reasoning_effort": "high"},
                "future_vendor": {"key": "val"}
            },
            "env": {},
            "max_budget_usd": null,
            "max_turns": null
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert_eq!(config.vendor.len(), 2);
        assert!(config.vendor.contains_key("future_vendor"));
    }

    #[test]
    fn backend_identity_with_extra_fields() {
        let j = json!({
            "id": "sidecar:future",
            "backend_version": "2.0.0",
            "adapter_version": null,
            "capabilities_hash": "sha256:abc"
        });
        let id: BackendIdentity = serde_json::from_value(j).unwrap();
        assert_eq!(id.id, "sidecar:future");
    }

    #[test]
    fn usage_normalized_with_extra_fields() {
        let j = json!({
            "input_tokens": 100,
            "output_tokens": 200,
            "cache_read_tokens": null,
            "cache_write_tokens": null,
            "request_units": null,
            "estimated_cost_usd": null,
            "reasoning_tokens": 50,
            "search_tokens": 10
        });
        let usage: UsageNormalized = serde_json::from_value(j).unwrap();
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(200));
    }

    #[test]
    fn artifact_ref_with_extra_fields() {
        let j = json!({
            "kind": "patch",
            "path": "output.diff",
            "size_bytes": 1024,
            "checksum": "sha256:def"
        });
        let artifact: ArtifactRef = serde_json::from_value(j).unwrap();
        assert_eq!(artifact.kind, "patch");
    }

    #[test]
    fn context_snippet_with_extra_fields() {
        let j = json!({
            "name": "readme",
            "content": "# Hello",
            "language": "markdown",
            "line_range": [1, 10]
        });
        let snippet: ContextSnippet = serde_json::from_value(j).unwrap();
        assert_eq!(snippet.name, "readme");
    }

    #[test]
    fn verification_report_with_extra_fields() {
        let j = json!({
            "git_diff": "+ new line",
            "git_status": "M file.rs",
            "harness_ok": true,
            "test_results": {"passed": 42, "failed": 0}
        });
        let report: VerificationReport = serde_json::from_value(j).unwrap();
        assert!(report.harness_ok);
    }

    #[test]
    fn run_metadata_with_extra_fields() {
        let now = Utc::now();
        let j = json!({
            "run_id": Uuid::nil().to_string(),
            "work_order_id": Uuid::nil().to_string(),
            "contract_version": "abp/v0.1",
            "started_at": now.to_rfc3339(),
            "finished_at": now.to_rfc3339(),
            "duration_ms": 100,
            "host_machine": "runner-42",
            "retry_count": 0
        });
        let meta: RunMetadata = serde_json::from_value(j).unwrap();
        assert_eq!(meta.duration_ms, 100);
    }

    #[test]
    fn workspace_spec_with_extra_fields() {
        let j = json!({
            "root": "/tmp",
            "mode": "staged",
            "include": [],
            "exclude": [],
            "max_depth": 10,
            "follow_symlinks": false
        });
        let ws: WorkspaceSpec = serde_json::from_value(j).unwrap();
        assert_eq!(ws.root, "/tmp");
    }

    #[test]
    fn multiple_unknown_fields_at_once() {
        let mut j = minimal_work_order_json();
        j["alpha"] = json!(1);
        j["beta"] = json!("two");
        j["gamma"] = json!([3, 4, 5]);
        j["delta"] = json!({"nested": true});
        let wo: WorkOrder = serde_json::from_value(j).unwrap();
        assert_eq!(wo.task, "test task");
    }

    #[test]
    fn deeply_nested_unknown_fields() {
        let mut j = minimal_work_order_json();
        j["config"]["vendor"]["future_sdk"] = json!({
            "deep": {"nested": {"value": 42}}
        });
        let wo: WorkOrder = serde_json::from_value(j).unwrap();
        let future = &wo.config.vendor["future_sdk"];
        assert_eq!(future["deep"]["nested"]["value"], 42);
    }
}

// ============================================================================
// Module 2: Contract Version Handling (20+ tests)
// ============================================================================

mod contract_version {
    use super::*;

    #[test]
    fn contract_version_constant_is_v0_1() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn parse_current_version() {
        assert_eq!(parse_version(CONTRACT_VERSION), Some((0, 1)));
    }

    #[test]
    fn parse_future_minor_version() {
        assert_eq!(parse_version("abp/v0.2"), Some((0, 2)));
    }

    #[test]
    fn parse_future_major_version() {
        assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
    }

    #[test]
    fn parse_high_versions() {
        assert_eq!(parse_version("abp/v99.999"), Some((99, 999)));
    }

    #[test]
    fn parse_invalid_missing_prefix() {
        assert_eq!(parse_version("v0.1"), None);
    }

    #[test]
    fn parse_invalid_empty() {
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn parse_invalid_no_dot() {
        assert_eq!(parse_version("abp/v01"), None);
    }

    #[test]
    fn parse_invalid_non_numeric_major() {
        assert_eq!(parse_version("abp/vX.1"), None);
    }

    #[test]
    fn parse_invalid_non_numeric_minor() {
        assert_eq!(parse_version("abp/v0.Y"), None);
    }

    #[test]
    fn parse_invalid_extra_segments() {
        // "abp/v0.1.2" → minor_str = "1.2" → parse fails
        assert_eq!(parse_version("abp/v0.1.2"), None);
    }

    #[test]
    fn compatible_same_version() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    }

    #[test]
    fn compatible_minor_bump() {
        assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
    }

    #[test]
    fn incompatible_major_mismatch() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    }

    #[test]
    fn incompatible_with_invalid_version() {
        assert!(!is_compatible_version("garbage", "abp/v0.1"));
    }

    #[test]
    fn incompatible_both_invalid() {
        assert!(!is_compatible_version("nope", "also_nope"));
    }

    // -- ProtocolVersion structured type tests --

    #[test]
    fn protocol_version_parse_valid() {
        let v = ProtocolVersion::parse("abp/v0.1").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
    }

    #[test]
    fn protocol_version_parse_invalid_format() {
        let err = ProtocolVersion::parse("invalid").unwrap_err();
        assert_eq!(err, VersionError::InvalidFormat);
    }

    #[test]
    fn protocol_version_parse_invalid_major() {
        let err = ProtocolVersion::parse("abp/vX.1").unwrap_err();
        assert_eq!(err, VersionError::InvalidMajor);
    }

    #[test]
    fn protocol_version_parse_invalid_minor() {
        let err = ProtocolVersion::parse("abp/v0.Z").unwrap_err();
        assert_eq!(err, VersionError::InvalidMinor);
    }

    #[test]
    fn protocol_version_current() {
        let current = ProtocolVersion::current();
        assert_eq!(current.major, 0);
        assert_eq!(current.minor, 1);
    }

    #[test]
    fn protocol_version_display() {
        let v = ProtocolVersion { major: 2, minor: 3 };
        assert_eq!(format!("{v}"), "abp/v2.3");
    }

    #[test]
    fn protocol_version_to_string() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        assert_eq!(v.to_string(), "abp/v0.1");
    }

    #[test]
    fn protocol_version_is_compatible_same() {
        let v1 = ProtocolVersion { major: 0, minor: 1 };
        let v2 = ProtocolVersion { major: 0, minor: 1 };
        assert!(v1.is_compatible(&v2));
    }

    #[test]
    fn protocol_version_is_compatible_newer_minor() {
        let v1 = ProtocolVersion { major: 0, minor: 1 };
        let v2 = ProtocolVersion { major: 0, minor: 3 };
        assert!(v1.is_compatible(&v2));
    }

    #[test]
    fn protocol_version_is_not_compatible_older_minor() {
        let v1 = ProtocolVersion { major: 0, minor: 3 };
        let v2 = ProtocolVersion { major: 0, minor: 1 };
        assert!(!v1.is_compatible(&v2));
    }

    #[test]
    fn protocol_version_is_not_compatible_different_major() {
        let v1 = ProtocolVersion { major: 0, minor: 1 };
        let v2 = ProtocolVersion { major: 1, minor: 0 };
        assert!(!v1.is_compatible(&v2));
    }

    // -- VersionRange tests --

    #[test]
    fn version_range_contains() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 5 },
        };
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    }

    #[test]
    fn version_range_contains_boundary_min() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 5 },
        };
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    }

    #[test]
    fn version_range_contains_boundary_max() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 5 },
        };
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 5 }));
    }

    #[test]
    fn version_range_does_not_contain_below_min() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 2 },
            max: ProtocolVersion { major: 0, minor: 5 },
        };
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    }

    #[test]
    fn version_range_does_not_contain_above_max() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 5 },
        };
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 6 }));
    }

    // -- negotiate_version tests --

    #[test]
    fn negotiate_same_version() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&v, &v).unwrap();
        assert_eq!(result, v);
    }

    #[test]
    fn negotiate_picks_minimum() {
        let local = ProtocolVersion { major: 0, minor: 3 };
        let remote = ProtocolVersion { major: 0, minor: 1 };
        let result = negotiate_version(&local, &remote).unwrap();
        assert_eq!(result.minor, 1);
    }

    #[test]
    fn negotiate_fails_on_major_mismatch() {
        let local = ProtocolVersion { major: 0, minor: 1 };
        let remote = ProtocolVersion { major: 1, minor: 0 };
        let err = negotiate_version(&local, &remote).unwrap_err();
        assert!(matches!(err, VersionError::Incompatible { .. }));
    }

    #[test]
    fn receipt_meta_has_contract_version() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn hello_envelope_has_contract_version() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        match env {
            Envelope::Hello {
                contract_version, ..
            } => {
                assert_eq!(contract_version, CONTRACT_VERSION);
            }
            _ => panic!("Expected Hello"),
        }
    }
}

// ============================================================================
// Module 3: JSON Schema Evolution (15+ tests)
// ============================================================================

mod schema_evolution {
    use super::*;

    #[test]
    fn older_receipt_without_mode_field() {
        // `mode` has #[serde(default)] so it should default when missing
        let mut j = minimal_receipt_json();
        j.as_object_mut().unwrap().remove("mode");
        let r: Receipt = serde_json::from_value(j).unwrap();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn older_agent_event_without_ext_field() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "assistant_message",
            "text": "hi"
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        assert!(event.ext.is_none());
    }

    #[test]
    fn older_fatal_without_error_code_field() {
        let j = json!({
            "t": "fatal",
            "ref_id": null,
            "error": "something failed"
        });
        let env: Envelope = serde_json::from_value(j).unwrap();
        match env {
            Envelope::Fatal { error_code, .. } => assert!(error_code.is_none()),
            _ => panic!("Expected Fatal"),
        }
    }

    #[test]
    fn older_hello_without_mode_field() {
        let j = json!({
            "t": "hello",
            "contract_version": "abp/v0.1",
            "backend": {
                "id": "test",
                "backend_version": null,
                "adapter_version": null
            },
            "capabilities": {}
        });
        let env: Envelope = serde_json::from_value(j).unwrap();
        match env {
            Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
            _ => panic!("Expected Hello"),
        }
    }

    #[test]
    fn nullable_field_present_as_null() {
        let j = json!({
            "model": null,
            "vendor": {},
            "env": {},
            "max_budget_usd": null,
            "max_turns": null
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert!(config.model.is_none());
        assert!(config.max_budget_usd.is_none());
    }

    #[test]
    fn nullable_field_present_with_value() {
        let j = json!({
            "model": "gpt-4",
            "vendor": {},
            "env": {},
            "max_budget_usd": 10.0,
            "max_turns": 50
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert_eq!(config.model.as_deref(), Some("gpt-4"));
        assert_eq!(config.max_budget_usd, Some(10.0));
    }

    #[test]
    fn default_values_fill_for_empty_collections() {
        let j = json!({
            "files": [],
            "snippets": []
        });
        let ctx: ContextPacket = serde_json::from_value(j).unwrap();
        assert!(ctx.files.is_empty());
        assert!(ctx.snippets.is_empty());
    }

    #[test]
    fn default_policy_profile_is_all_empty() {
        let j = json!({
            "allowed_tools": [],
            "disallowed_tools": [],
            "deny_read": [],
            "deny_write": [],
            "allow_network": [],
            "deny_network": [],
            "require_approval_for": []
        });
        let policy: PolicyProfile = serde_json::from_value(j).unwrap();
        assert!(policy.allowed_tools.is_empty());
    }

    #[test]
    fn newer_json_with_additional_usage_fields_deserializes() {
        let mut j = minimal_receipt_json();
        j["usage"]["reasoning_tokens"] = json!(500);
        j["usage"]["audio_tokens"] = json!(100);
        let r: Receipt = serde_json::from_value(j).unwrap();
        // Unknown fields ignored, known fields still work
        assert!(r.usage.input_tokens.is_none());
    }

    #[test]
    fn newer_json_with_additional_outcome_still_works_for_known() {
        // Outcome::Complete serializes as "complete"
        let j = json!("complete");
        let outcome: Outcome = serde_json::from_value(j).unwrap();
        assert_eq!(outcome, Outcome::Complete);
    }

    #[test]
    fn unknown_outcome_value_errors() {
        let j = json!("suspended");
        let result = serde_json::from_value::<Outcome>(j);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_execution_lane_errors() {
        let j = json!("parallel_first");
        let result = serde_json::from_value::<ExecutionLane>(j);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_workspace_mode_errors() {
        let j = json!("containerized");
        let result = serde_json::from_value::<WorkspaceMode>(j);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_execution_mode_errors() {
        let j = json!("hybrid");
        let result = serde_json::from_value::<ExecutionMode>(j);
        assert!(result.is_err());
    }

    #[test]
    fn capability_manifest_empty_round_trips() {
        let manifest = CapabilityManifest::new();
        let j = serde_json::to_value(&manifest).unwrap();
        let back: CapabilityManifest = serde_json::from_value(j).unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn capability_manifest_with_known_capabilities_roundtrips() {
        let mut manifest = CapabilityManifest::new();
        manifest.insert(Capability::Streaming, SupportLevel::Native);
        manifest.insert(Capability::ToolRead, SupportLevel::Emulated);
        let j = serde_json::to_value(&manifest).unwrap();
        let back: CapabilityManifest = serde_json::from_value(j).unwrap();
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn receipt_usage_raw_preserves_arbitrary_vendor_json() {
        let raw_usage = json!({
            "anthropic": {"cache_creation_input_tokens": 1000},
            "future_vendor": {"custom_metric": "abc"}
        });
        let receipt = ReceiptBuilder::new("mock")
            .usage_raw(raw_usage.clone())
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.usage_raw, raw_usage);
    }
}

// ============================================================================
// Module 4: JSONL Protocol Evolution (15+ tests)
// ============================================================================

mod protocol_evolution {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn decode_known_hello_envelope() {
        let line = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
        let env = JsonlCodec::decode(line).unwrap();
        assert!(matches!(env, Envelope::Hello { .. }));
    }

    #[test]
    fn decode_known_fatal_envelope() {
        let line = r#"{"t":"fatal","ref_id":null,"error":"crash"}"#;
        let env = JsonlCodec::decode(line).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn unknown_envelope_type_returns_json_error() {
        let line = r#"{"t":"checkpoint","data":{}}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProtocolError::Json(_)));
    }

    #[test]
    fn unknown_envelope_type_heartbeat() {
        let line = r#"{"t":"heartbeat","timestamp":"2024-01-01T00:00:00Z"}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_envelope_type_ack() {
        let line = r#"{"t":"ack","ref_id":"run-1"}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err());
    }

    #[test]
    fn missing_t_field_errors() {
        let line = r#"{"error":"no type tag"}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err());
    }

    #[test]
    fn empty_json_object_errors() {
        let line = r#"{}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_json_errors() {
        let line = r#"not json at all"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err());
    }

    #[test]
    fn hello_with_unknown_capabilities() {
        let line = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{"future_cap":"native"}}"#;
        // Unknown capabilities aren't in the Capability enum; this will error because the key is unknown
        let result = JsonlCodec::decode(line);
        // Since Capability is a fixed enum, unknown keys in BTreeMap<Capability, SupportLevel> will fail
        assert!(result.is_err());
    }

    #[test]
    fn hello_with_empty_capabilities_succeeds() {
        let line = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let env = JsonlCodec::decode(line).unwrap();
        assert!(matches!(env, Envelope::Hello { .. }));
    }

    #[test]
    fn jsonl_stream_with_blank_lines_skipped() {
        let input = format!(
            "{}\n\n{}\n",
            r#"{"t":"fatal","ref_id":null,"error":"a"}"#,
            r#"{"t":"fatal","ref_id":null,"error":"b"}"#
        );
        let reader = BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 2);
    }

    #[test]
    fn jsonl_stream_single_message() {
        let input = r#"{"t":"fatal","ref_id":null,"error":"only one"}"#.to_string() + "\n";
        let reader = BufReader::new(input.as_bytes());
        let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(envelopes.len(), 1);
    }

    #[test]
    fn encode_decode_roundtrip_hello() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "rt".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Hello { .. }));
    }

    #[test]
    fn encode_decode_roundtrip_fatal() {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "oom".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(decoded, Envelope::Fatal { .. }));
    }

    #[test]
    fn encode_produces_newline_terminated() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "test".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        assert!(line.ends_with('\n'));
        assert!(!line.ends_with("\n\n"));
    }

    #[test]
    fn version_negotiation_in_hello() {
        // Simulate a future sidecar announcing v0.3
        let j = json!({
            "t": "hello",
            "contract_version": "abp/v0.3",
            "backend": {"id": "future-sidecar", "backend_version": null, "adapter_version": null},
            "capabilities": {}
        });
        let env: Envelope = serde_json::from_value(j).unwrap();
        match env {
            Envelope::Hello {
                contract_version, ..
            } => {
                let theirs = ProtocolVersion::parse(&contract_version).unwrap();
                let ours = ProtocolVersion::current();
                // Same major → compatible
                let negotiated = negotiate_version(&ours, &theirs).unwrap();
                assert_eq!(negotiated.minor, 1); // min of 1 and 3
            }
            _ => panic!("Expected Hello"),
        }
    }

    #[test]
    fn version_negotiation_incompatible_hello() {
        let j = json!({
            "t": "hello",
            "contract_version": "abp/v1.0",
            "backend": {"id": "future-sidecar", "backend_version": null, "adapter_version": null},
            "capabilities": {}
        });
        let env: Envelope = serde_json::from_value(j).unwrap();
        match env {
            Envelope::Hello {
                contract_version, ..
            } => {
                let theirs = ProtocolVersion::parse(&contract_version).unwrap();
                let ours = ProtocolVersion::current();
                let result = negotiate_version(&ours, &theirs);
                assert!(result.is_err());
            }
            _ => panic!("Expected Hello"),
        }
    }
}

// ============================================================================
// Module 5: Type Coercion and Flexibility (15+ tests)
// ============================================================================

mod type_coercion {
    use super::*;

    #[test]
    fn uuid_as_string_in_json() {
        let id = Uuid::new_v4();
        let j = json!(id.to_string());
        let parsed: Uuid = serde_json::from_value(j).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn uuid_nil_roundtrips() {
        let j = json!(Uuid::nil().to_string());
        let parsed: Uuid = serde_json::from_value(j).unwrap();
        assert_eq!(parsed, Uuid::nil());
    }

    #[test]
    fn duration_ms_as_integer() {
        let j = json!({"run_id": Uuid::nil(), "work_order_id": Uuid::nil(),
                       "contract_version": "abp/v0.1",
                       "started_at": "2024-01-01T00:00:00Z",
                       "finished_at": "2024-01-01T00:00:01Z",
                       "duration_ms": 1000});
        let meta: RunMetadata = serde_json::from_value(j).unwrap();
        assert_eq!(meta.duration_ms, 1000);
    }

    #[test]
    fn max_turns_as_integer() {
        let j = json!({
            "model": null, "vendor": {}, "env": {},
            "max_budget_usd": null, "max_turns": 10
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert_eq!(config.max_turns, Some(10));
    }

    #[test]
    fn max_budget_as_float() {
        let j = json!({
            "model": null, "vendor": {}, "env": {},
            "max_budget_usd": 5.50, "max_turns": null
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert_eq!(config.max_budget_usd, Some(5.50));
    }

    #[test]
    fn max_budget_as_integer_coerced_to_float() {
        let j = json!({
            "model": null, "vendor": {}, "env": {},
            "max_budget_usd": 5, "max_turns": null
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert_eq!(config.max_budget_usd, Some(5.0));
    }

    #[test]
    fn empty_string_model() {
        let j = json!({
            "model": "", "vendor": {}, "env": {},
            "max_budget_usd": null, "max_turns": null
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert_eq!(config.model, Some("".to_string()));
    }

    #[test]
    fn null_model() {
        let j = json!({
            "model": null, "vendor": {}, "env": {},
            "max_budget_usd": null, "max_turns": null
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert!(config.model.is_none());
    }

    #[test]
    fn vendor_with_mixed_value_types() {
        let j = json!({
            "model": null, "env": {},
            "max_budget_usd": null, "max_turns": null,
            "vendor": {
                "string_val": "hello",
                "number_val": 42,
                "bool_val": true,
                "null_val": null,
                "array_val": [1, 2, 3],
                "object_val": {"nested": true}
            }
        });
        let config: RuntimeConfig = serde_json::from_value(j).unwrap();
        assert_eq!(config.vendor.len(), 6);
    }

    #[test]
    fn tool_call_input_as_string_json() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "tool_call",
            "tool_name": "read_file",
            "tool_use_id": null,
            "parent_tool_use_id": null,
            "input": "plain string input"
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        match event.kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert!(input.is_string());
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn tool_call_input_as_object_json() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "tool_call",
            "tool_name": "read_file",
            "tool_use_id": null,
            "parent_tool_use_id": null,
            "input": {"path": "/tmp/file.txt"}
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        match event.kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert!(input.is_object());
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn tool_result_output_as_null() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "tool_result",
            "tool_name": "bash",
            "tool_use_id": null,
            "output": null,
            "is_error": false
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        match event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert!(output.is_null());
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn tool_result_output_as_array() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "tool_result",
            "tool_name": "glob",
            "tool_use_id": null,
            "output": ["file1.rs", "file2.rs"],
            "is_error": false
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        match event.kind {
            AgentEventKind::ToolResult { output, .. } => {
                assert!(output.is_array());
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn nested_null_in_ext_field() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "assistant_message",
            "text": "hi",
            "ext": {"raw_message": null}
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        // ext is flattened, so "ext" key may be captured
        assert!(matches!(
            event.kind,
            AgentEventKind::AssistantMessage { .. }
        ));
    }

    #[test]
    fn exit_code_null_vs_integer() {
        // null exit_code
        let j1 = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "command_executed",
            "command": "ls",
            "exit_code": null,
            "output_preview": null
        });
        let e1: AgentEvent = serde_json::from_value(j1).unwrap();

        // integer exit_code
        let j2 = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "command_executed",
            "command": "ls",
            "exit_code": 0,
            "output_preview": "file.txt"
        });
        let e2: AgentEvent = serde_json::from_value(j2).unwrap();

        match e1.kind {
            AgentEventKind::CommandExecuted { exit_code, .. } => assert!(exit_code.is_none()),
            _ => panic!("Expected CommandExecuted"),
        }
        match e2.kind {
            AgentEventKind::CommandExecuted { exit_code, .. } => assert_eq!(exit_code, Some(0)),
            _ => panic!("Expected CommandExecuted"),
        }
    }

    #[test]
    fn boolean_is_error_field() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "tool_result",
            "tool_name": "bash",
            "tool_use_id": null,
            "output": "error text",
            "is_error": true
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        match event.kind {
            AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn usage_raw_accepts_any_json_shape() {
        let shapes = vec![
            json!(null),
            json!(42),
            json!("string"),
            json!([1, 2, 3]),
            json!({"nested": {"deep": true}}),
        ];
        for shape in shapes {
            let receipt = ReceiptBuilder::new("mock")
                .usage_raw(shape.clone())
                .outcome(Outcome::Complete)
                .build();
            assert_eq!(receipt.usage_raw, shape);
        }
    }
}

// ============================================================================
// Module 6: Interoperability (10+ tests)
// ============================================================================

mod interoperability {
    use super::*;

    #[test]
    fn serde_json_roundtrip_work_order() {
        let wo = WorkOrderBuilder::new("test interop").build();
        let json_str = serde_json::to_string(&wo).unwrap();
        let back: WorkOrder = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.task, "test interop");
        assert_eq!(back.id, wo.id);
    }

    #[test]
    fn serde_json_roundtrip_receipt() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let json_str = serde_json::to_string(&receipt).unwrap();
        let back: Receipt = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.outcome, Outcome::Complete);
        assert_eq!(back.meta.run_id, receipt.meta.run_id);
    }

    #[test]
    fn serde_json_roundtrip_envelope() {
        let env = Envelope::Fatal {
            ref_id: Some("run-1".into()),
            error: "test".into(),
            error_code: None,
        };
        let json_str = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json_str).unwrap();
        assert!(matches!(back, Envelope::Fatal { .. }));
    }

    #[test]
    fn canonical_json_is_deterministic() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        let h1 = abp_core::receipt_hash(&receipt).unwrap();
        let h2 = abp_core::receipt_hash(&receipt).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn canonical_json_keys_sorted() {
        let v = json!({"z": 1, "a": 2, "m": 3});
        let canonical = abp_core::canonical_json(&v).unwrap();
        // serde_json preserves insertion order for Map but to_value converts
        // to BTreeMap if the `preserve_order` feature isn't set, so keys are sorted
        let first_key_pos_a = canonical.find("\"a\"").unwrap();
        let first_key_pos_m = canonical.find("\"m\"").unwrap();
        let first_key_pos_z = canonical.find("\"z\"").unwrap();
        assert!(first_key_pos_a < first_key_pos_m);
        assert!(first_key_pos_m < first_key_pos_z);
    }

    #[test]
    fn utf8_in_work_order_task() {
        let wo = WorkOrderBuilder::new("修复登录问题 🔧").build();
        let json_str = serde_json::to_string(&wo).unwrap();
        let back: WorkOrder = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.task, "修复登录问题 🔧");
    }

    #[test]
    fn utf8_in_agent_event_text() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "こんにちは世界 🌍".into(),
            },
            ext: None,
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
        match back.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "こんにちは世界 🌍");
            }
            _ => panic!("Expected AssistantMessage"),
        }
    }

    #[test]
    fn utf8_in_error_message() {
        let env = Envelope::Fatal {
            ref_id: None,
            error: "Ошибка: файл не найден 🚫".into(),
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let back = JsonlCodec::decode(line.trim()).unwrap();
        match back {
            Envelope::Fatal { error, .. } => {
                assert_eq!(error, "Ошибка: файл не найден 🚫");
            }
            _ => panic!("Expected Fatal"),
        }
    }

    #[test]
    fn line_ending_lf_in_jsonl() {
        let line = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n";
        let env = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn line_ending_crlf_in_jsonl() {
        let line = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\r\n";
        let env = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn jsonl_with_trailing_whitespace() {
        let line = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}   ";
        let env = JsonlCodec::decode(line.trim()).unwrap();
        assert!(matches!(env, Envelope::Fatal { .. }));
    }

    #[test]
    fn special_characters_in_strings_roundtrip() {
        let special = "line1\nline2\ttab\"quote\\backslash";
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: special.into(),
            },
            ext: None,
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let back: AgentEvent = serde_json::from_str(&json_str).unwrap();
        match back.kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, special),
            _ => panic!("Expected AssistantMessage"),
        }
    }

    #[test]
    fn large_json_value_in_usage_raw() {
        let large_obj: serde_json::Map<String, Value> =
            (0..1000).map(|i| (format!("key_{i}"), json!(i))).collect();
        let receipt = ReceiptBuilder::new("mock")
            .usage_raw(Value::Object(large_obj.clone()))
            .outcome(Outcome::Complete)
            .build();
        let json_str = serde_json::to_string(&receipt).unwrap();
        let back: Receipt = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back.usage_raw.as_object().unwrap().len(), 1000);
    }

    #[test]
    fn receipt_hash_stable_across_serialization() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        let json_str = serde_json::to_string(&receipt).unwrap();
        let back: Receipt = serde_json::from_str(&json_str).unwrap();
        // Re-hash the deserialized receipt and compare
        let rehashed = abp_core::receipt_hash(&back).unwrap();
        assert_eq!(receipt.receipt_sha256.as_ref().unwrap(), &rehashed);
    }
}

// ============================================================================
// Module 7: Error code handling (additional tests for completeness)
// ============================================================================

mod error_code_compat {
    use super::*;

    #[test]
    fn known_error_code_roundtrips() {
        let code = abp_error::ErrorCode::BackendTimeout;
        let j = serde_json::to_value(code).unwrap();
        let back: abp_error::ErrorCode = serde_json::from_value(j).unwrap();
        assert_eq!(back, abp_error::ErrorCode::BackendTimeout);
    }

    #[test]
    fn error_code_serializes_to_snake_case() {
        let code = abp_error::ErrorCode::ProtocolInvalidEnvelope;
        let j = serde_json::to_value(code).unwrap();
        assert_eq!(j.as_str().unwrap(), "protocol_invalid_envelope");
    }

    #[test]
    fn unknown_error_code_string_fails_deser() {
        let j = json!("future_unknown_error");
        let result = serde_json::from_value::<abp_error::ErrorCode>(j);
        assert!(result.is_err());
    }

    #[test]
    fn fatal_envelope_with_known_error_code() {
        let j = json!({
            "t": "fatal",
            "ref_id": "run-1",
            "error": "timed out",
            "error_code": "backend_timeout"
        });
        let env: Envelope = serde_json::from_value(j).unwrap();
        match env {
            Envelope::Fatal { error_code, .. } => {
                assert_eq!(error_code, Some(abp_error::ErrorCode::BackendTimeout));
            }
            _ => panic!("Expected Fatal"),
        }
    }

    #[test]
    fn fatal_envelope_with_unknown_error_code_fails() {
        let j = json!({
            "t": "fatal",
            "ref_id": null,
            "error": "something new",
            "error_code": "future_error_code"
        });
        let result = serde_json::from_value::<Envelope>(j);
        assert!(result.is_err());
    }

    #[test]
    fn fatal_envelope_with_null_error_code() {
        let j = json!({
            "t": "fatal",
            "ref_id": null,
            "error": "generic error",
            "error_code": null
        });
        let env: Envelope = serde_json::from_value(j).unwrap();
        match env {
            Envelope::Fatal { error_code, .. } => assert!(error_code.is_none()),
            _ => panic!("Expected Fatal"),
        }
    }

    #[test]
    fn agent_event_error_with_error_code() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "error",
            "message": "tool failed",
            "error_code": "execution_tool_failed"
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        match event.kind {
            AgentEventKind::Error { error_code, .. } => {
                assert_eq!(error_code, Some(abp_error::ErrorCode::ExecutionToolFailed));
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn agent_event_error_without_error_code() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "error",
            "message": "something broke"
        });
        let event: AgentEvent = serde_json::from_value(j).unwrap();
        match event.kind {
            AgentEventKind::Error { error_code, .. } => assert!(error_code.is_none()),
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn error_code_category_mapping_stable() {
        assert_eq!(
            abp_error::ErrorCode::ProtocolInvalidEnvelope.category(),
            abp_error::ErrorCategory::Protocol
        );
        assert_eq!(
            abp_error::ErrorCode::BackendNotFound.category(),
            abp_error::ErrorCategory::Backend
        );
        assert_eq!(
            abp_error::ErrorCode::PolicyDenied.category(),
            abp_error::ErrorCategory::Policy
        );
    }

    #[test]
    fn error_code_as_str_stable() {
        assert_eq!(abp_error::ErrorCode::Internal.as_str(), "internal");
        assert_eq!(
            abp_error::ErrorCode::ConfigInvalid.as_str(),
            "config_invalid"
        );
    }

    #[test]
    fn protocol_error_carries_error_code() {
        let err = ProtocolError::Violation("test".into());
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    }

    #[test]
    fn envelope_fatal_error_code_accessor() {
        let env = Envelope::fatal_with_code(
            Some("run-1".into()),
            "timed out",
            abp_error::ErrorCode::BackendTimeout,
        );
        assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
    }

    #[test]
    fn envelope_non_fatal_error_code_is_none() {
        let env = Envelope::hello(
            BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            CapabilityManifest::new(),
        );
        assert!(env.error_code().is_none());
    }
}

// ============================================================================
// Module 8: Agent Event variant compatibility
// ============================================================================

mod agent_event_compat {
    use super::*;

    #[test]
    fn unknown_agent_event_type_errors() {
        let j = json!({
            "ts": Utc::now().to_rfc3339(),
            "type": "thinking_delta",
            "text": "hmm..."
        });
        let result = serde_json::from_value::<AgentEvent>(j);
        assert!(result.is_err());
    }

    #[test]
    fn all_known_event_kinds_roundtrip() {
        let events = vec![
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            AgentEventKind::AssistantDelta { text: "tok".into() },
            AgentEventKind::AssistantMessage {
                text: "full".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: None,
                output: json!("content"),
                is_error: false,
            },
            AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "edit".into(),
            },
            AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: Some("files".into()),
            },
            AgentEventKind::Warning {
                message: "warn".into(),
            },
            AgentEventKind::Error {
                message: "err".into(),
                error_code: None,
            },
        ];

        for kind in events {
            let event = AgentEvent {
                ts: Utc::now(),
                kind: kind.clone(),
                ext: None,
            };
            let j = serde_json::to_value(&event).unwrap();
            let back: AgentEvent = serde_json::from_value(j).unwrap();
            // Verify the type tag roundtrips
            let original_json = serde_json::to_string(&event).unwrap();
            let back_json = serde_json::to_string(&back).unwrap();
            assert_eq!(original_json, back_json);
        }
    }

    #[test]
    fn agent_event_with_ext_data_roundtrips() {
        let mut ext = BTreeMap::new();
        ext.insert("raw_message".to_string(), json!({"role": "assistant"}));
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext.clone()),
        };
        let j = serde_json::to_value(&event).unwrap();
        let back: AgentEvent = serde_json::from_value(j).unwrap();
        assert!(back.ext.is_some());
    }

    #[test]
    fn capability_enum_all_variants_roundtrip() {
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
            Capability::FunctionCalling,
            Capability::Vision,
            Capability::Audio,
            Capability::JsonMode,
            Capability::SystemMessage,
            Capability::Temperature,
            Capability::TopP,
            Capability::TopK,
            Capability::MaxTokens,
            Capability::FrequencyPenalty,
            Capability::PresencePenalty,
            Capability::CacheControl,
            Capability::BatchMode,
            Capability::Embeddings,
            Capability::ImageGeneration,
        ];
        for cap in caps {
            let j = serde_json::to_value(&cap).unwrap();
            let back: Capability = serde_json::from_value(j).unwrap();
            assert_eq!(back, cap);
        }
    }

    #[test]
    fn unknown_capability_string_fails() {
        let j = json!("quantum_compute");
        let result = serde_json::from_value::<Capability>(j);
        assert!(result.is_err());
    }

    #[test]
    fn support_level_restricted_with_reason_roundtrips() {
        let level = SupportLevel::Restricted {
            reason: "disabled by policy".into(),
        };
        let j = serde_json::to_value(&level).unwrap();
        let back: SupportLevel = serde_json::from_value(j).unwrap();
        match back {
            SupportLevel::Restricted { reason } => {
                assert_eq!(reason, "disabled by policy");
            }
            _ => panic!("Expected Restricted"),
        }
    }

    #[test]
    fn outcome_all_variants_roundtrip() {
        for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
            let j = serde_json::to_value(&outcome).unwrap();
            let back: Outcome = serde_json::from_value(j).unwrap();
            assert_eq!(back, outcome);
        }
    }

    #[test]
    fn execution_mode_all_variants_roundtrip() {
        for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
            let j = serde_json::to_value(&mode).unwrap();
            let back: ExecutionMode = serde_json::from_value(j).unwrap();
            assert_eq!(back, mode);
        }
    }
}
