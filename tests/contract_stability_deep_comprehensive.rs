#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep comprehensive contract stability tests.
//!
//! These tests validate that all public API types in abp-core maintain
//! backward compatibility, CONTRACT_VERSION is consistent, serde round-trips
//! are stable, and schema evolution rules are followed.

use abp_core::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt::Debug;
use uuid::Uuid;

// ===================================================================
// Helpers
// ===================================================================

fn sample_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn sample_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn sample_agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn sample_run_metadata() -> RunMetadata {
    let now = Utc::now();
    RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: now,
        finished_at: now,
        duration_ms: 0,
    }
}

fn sample_backend_identity() -> BackendIdentity {
    BackendIdentity {
        id: "mock".into(),
        backend_version: None,
        adapter_version: None,
    }
}

fn sample_workspace_spec() -> WorkspaceSpec {
    WorkspaceSpec {
        root: ".".into(),
        mode: WorkspaceMode::Staged,
        include: vec![],
        exclude: vec![],
    }
}

fn sample_context_packet() -> ContextPacket {
    ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "some context".into(),
        }],
    }
}

fn sample_policy_profile() -> PolicyProfile {
    PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/*".into()],
        allow_network: vec!["example.com".into()],
        deny_network: vec!["*.evil.com".into()],
        require_approval_for: vec!["deploy".into()],
    }
}

fn sample_capability_requirements() -> CapabilityRequirements {
    CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    }
}

fn sample_runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        model: Some("gpt-4".into()),
        vendor: BTreeMap::new(),
        env: BTreeMap::new(),
        max_budget_usd: Some(1.0),
        max_turns: Some(10),
    }
}

fn sample_usage_normalized() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(10),
        cache_write_tokens: Some(5),
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    }
}

fn sample_verification_report() -> VerificationReport {
    VerificationReport {
        git_diff: Some("diff --git a/f b/f".into()),
        git_status: Some("M f".into()),
        harness_ok: true,
    }
}

fn sample_artifact_ref() -> ArtifactRef {
    ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    }
}

/// Assert a type can round-trip through serde_json.
fn assert_serde_roundtrip<T: Serialize + for<'de> Deserialize<'de> + Debug>(val: &T) {
    let json = serde_json::to_string(val).expect("serialize");
    let _: T = serde_json::from_str(&json).expect("deserialize");
}

/// Assert a type implements Clone + Debug.
fn assert_clone_debug<T: Clone + Debug>(val: &T) {
    let _cloned = val.clone();
    let _debug = format!("{:?}", val);
}

// ===================================================================
// 1. CONTRACT_VERSION format and value
// ===================================================================

#[test]
fn contract_version_value() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_starts_with_prefix() {
    assert!(CONTRACT_VERSION.starts_with("abp/v"));
}

#[test]
fn contract_version_has_major_minor() {
    let rest = CONTRACT_VERSION.strip_prefix("abp/v").unwrap();
    let parts: Vec<&str> = rest.split('.').collect();
    assert_eq!(parts.len(), 2);
    let _major: u32 = parts[0].parse().expect("major is u32");
    let _minor: u32 = parts[1].parse().expect("minor is u32");
}

#[test]
fn contract_version_in_receipt_builder() {
    let receipt = sample_receipt();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn contract_version_is_static_str() {
    let s: &'static str = CONTRACT_VERSION;
    assert!(!s.is_empty());
}

#[test]
fn contract_version_no_trailing_whitespace() {
    assert_eq!(CONTRACT_VERSION, CONTRACT_VERSION.trim());
}

// ===================================================================
// 2. Trait implementations: Serialize, Deserialize, Clone, Debug
// ===================================================================

#[test]
fn work_order_implements_required_traits() {
    let wo = sample_work_order();
    assert_clone_debug(&wo);
    assert_serde_roundtrip(&wo);
}

#[test]
fn receipt_implements_required_traits() {
    let r = sample_receipt();
    assert_clone_debug(&r);
    assert_serde_roundtrip(&r);
}

#[test]
fn agent_event_implements_required_traits() {
    let e = sample_agent_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    assert_clone_debug(&e);
    assert_serde_roundtrip(&e);
}

#[test]
fn execution_lane_implements_required_traits() {
    let l = ExecutionLane::PatchFirst;
    assert_clone_debug(&l);
    assert_serde_roundtrip(&l);
}

#[test]
fn workspace_spec_implements_required_traits() {
    let ws = sample_workspace_spec();
    assert_clone_debug(&ws);
    assert_serde_roundtrip(&ws);
}

#[test]
fn workspace_mode_implements_required_traits() {
    let m = WorkspaceMode::Staged;
    assert_clone_debug(&m);
    assert_serde_roundtrip(&m);
}

#[test]
fn context_packet_implements_required_traits() {
    let cp = sample_context_packet();
    assert_clone_debug(&cp);
    assert_serde_roundtrip(&cp);
}

#[test]
fn context_snippet_implements_required_traits() {
    let cs = ContextSnippet {
        name: "n".into(),
        content: "c".into(),
    };
    assert_clone_debug(&cs);
    assert_serde_roundtrip(&cs);
}

#[test]
fn runtime_config_implements_required_traits() {
    let rc = sample_runtime_config();
    assert_clone_debug(&rc);
    assert_serde_roundtrip(&rc);
}

#[test]
fn policy_profile_implements_required_traits() {
    let pp = sample_policy_profile();
    assert_clone_debug(&pp);
    assert_serde_roundtrip(&pp);
}

#[test]
fn capability_requirements_implements_required_traits() {
    let cr = sample_capability_requirements();
    assert_clone_debug(&cr);
    assert_serde_roundtrip(&cr);
}

#[test]
fn capability_requirement_implements_required_traits() {
    let cr = CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Emulated,
    };
    assert_clone_debug(&cr);
    assert_serde_roundtrip(&cr);
}

#[test]
fn capability_enum_implements_required_traits() {
    let c = Capability::ToolRead;
    assert_clone_debug(&c);
    assert_serde_roundtrip(&c);
}

#[test]
fn support_level_implements_required_traits() {
    let sl = SupportLevel::Native;
    assert_clone_debug(&sl);
    assert_serde_roundtrip(&sl);
}

#[test]
fn min_support_implements_required_traits() {
    let ms = MinSupport::Native;
    assert_clone_debug(&ms);
    assert_serde_roundtrip(&ms);
}

#[test]
fn execution_mode_implements_required_traits() {
    let em = ExecutionMode::Passthrough;
    assert_clone_debug(&em);
    assert_serde_roundtrip(&em);
}

#[test]
fn backend_identity_implements_required_traits() {
    let bi = sample_backend_identity();
    assert_clone_debug(&bi);
    assert_serde_roundtrip(&bi);
}

#[test]
fn run_metadata_implements_required_traits() {
    let rm = sample_run_metadata();
    assert_clone_debug(&rm);
    assert_serde_roundtrip(&rm);
}

#[test]
fn usage_normalized_implements_required_traits() {
    let un = sample_usage_normalized();
    assert_clone_debug(&un);
    assert_serde_roundtrip(&un);
}

#[test]
fn outcome_implements_required_traits() {
    let o = Outcome::Complete;
    assert_clone_debug(&o);
    assert_serde_roundtrip(&o);
}

#[test]
fn artifact_ref_implements_required_traits() {
    let ar = sample_artifact_ref();
    assert_clone_debug(&ar);
    assert_serde_roundtrip(&ar);
}

#[test]
fn verification_report_implements_required_traits() {
    let vr = sample_verification_report();
    assert_clone_debug(&vr);
    assert_serde_roundtrip(&vr);
}

#[test]
fn agent_event_kind_implements_required_traits() {
    let k = AgentEventKind::RunStarted {
        message: "go".into(),
    };
    assert_clone_debug(&k);
    assert_serde_roundtrip(&k);
}

// ===================================================================
// 3. Serde round-trip stability for all contract types
// ===================================================================

#[test]
fn roundtrip_work_order_json() {
    let wo = sample_work_order();
    let json = serde_json::to_value(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.id, wo2.id);
}

#[test]
fn roundtrip_receipt_json() {
    let r = sample_receipt();
    let json = serde_json::to_value(&r).unwrap();
    let r2: Receipt = serde_json::from_value(json).unwrap();
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.backend.id, r2.backend.id);
}

#[test]
fn roundtrip_receipt_with_hash() {
    let r = sample_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn roundtrip_all_agent_event_kinds() {
    let kinds = vec![
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
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "f.txt"}),
        },
        AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("t1".into()),
            output: json!({"content": "hello"}),
            is_error: false,
        },
        AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added fn".into(),
        },
        AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        AgentEventKind::Warning {
            message: "watch out".into(),
        },
        AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        },
    ];
    for kind in &kinds {
        let event = sample_agent_event(kind.clone());
        assert_serde_roundtrip(&event);
    }
}

#[test]
fn roundtrip_execution_lane_variants() {
    for lane in [ExecutionLane::PatchFirst, ExecutionLane::WorkspaceFirst] {
        assert_serde_roundtrip(&lane);
    }
}

#[test]
fn roundtrip_workspace_mode_variants() {
    for mode in [WorkspaceMode::PassThrough, WorkspaceMode::Staged] {
        assert_serde_roundtrip(&mode);
    }
}

#[test]
fn roundtrip_outcome_variants() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        assert_serde_roundtrip(&outcome);
    }
}

#[test]
fn roundtrip_execution_mode_variants() {
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        assert_serde_roundtrip(&mode);
    }
}

#[test]
fn roundtrip_support_level_variants() {
    let variants = vec![
        SupportLevel::Native,
        SupportLevel::Emulated,
        SupportLevel::Unsupported,
        SupportLevel::Restricted {
            reason: "test".into(),
        },
    ];
    for sl in &variants {
        assert_serde_roundtrip(sl);
    }
}

#[test]
fn roundtrip_min_support_variants() {
    for ms in [MinSupport::Native, MinSupport::Emulated] {
        assert_serde_roundtrip(&ms);
    }
}

#[test]
fn roundtrip_capability_manifest() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    assert_serde_roundtrip(&manifest);
}

#[test]
fn roundtrip_context_packet_default() {
    let cp = ContextPacket::default();
    assert_serde_roundtrip(&cp);
}

#[test]
fn roundtrip_policy_profile_default() {
    let pp = PolicyProfile::default();
    assert_serde_roundtrip(&pp);
}

#[test]
fn roundtrip_runtime_config_default() {
    let rc = RuntimeConfig::default();
    assert_serde_roundtrip(&rc);
}

#[test]
fn roundtrip_usage_normalized_default() {
    let un = UsageNormalized::default();
    assert_serde_roundtrip(&un);
}

#[test]
fn roundtrip_verification_report_default() {
    let vr = VerificationReport::default();
    assert_serde_roundtrip(&vr);
}

// ===================================================================
// 4. Serde format stability — field names and structure
// ===================================================================

#[test]
fn execution_lane_serde_format_patch_first() {
    let json = serde_json::to_string(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(json, r#""patch_first""#);
}

#[test]
fn execution_lane_serde_format_workspace_first() {
    let json = serde_json::to_string(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(json, r#""workspace_first""#);
}

#[test]
fn workspace_mode_serde_format_pass_through() {
    let json = serde_json::to_string(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(json, r#""pass_through""#);
}

#[test]
fn workspace_mode_serde_format_staged() {
    let json = serde_json::to_string(&WorkspaceMode::Staged).unwrap();
    assert_eq!(json, r#""staged""#);
}

#[test]
fn outcome_serde_format_complete() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(json, r#""complete""#);
}

#[test]
fn outcome_serde_format_partial() {
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    assert_eq!(json, r#""partial""#);
}

#[test]
fn outcome_serde_format_failed() {
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    assert_eq!(json, r#""failed""#);
}

#[test]
fn execution_mode_serde_format_passthrough() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(json, r#""passthrough""#);
}

#[test]
fn execution_mode_serde_format_mapped() {
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(json, r#""mapped""#);
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn support_level_serde_format_native() {
    let json = serde_json::to_string(&SupportLevel::Native).unwrap();
    assert_eq!(json, r#""native""#);
}

#[test]
fn support_level_serde_format_emulated() {
    let json = serde_json::to_string(&SupportLevel::Emulated).unwrap();
    assert_eq!(json, r#""emulated""#);
}

#[test]
fn support_level_serde_format_unsupported() {
    let json = serde_json::to_string(&SupportLevel::Unsupported).unwrap();
    assert_eq!(json, r#""unsupported""#);
}

#[test]
fn support_level_serde_format_restricted() {
    let val = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    let json: Value = serde_json::to_value(&val).unwrap();
    assert_eq!(json["restricted"]["reason"], "policy");
}

#[test]
fn min_support_serde_format_native() {
    let json = serde_json::to_string(&MinSupport::Native).unwrap();
    assert_eq!(json, r#""native""#);
}

#[test]
fn min_support_serde_format_emulated() {
    let json = serde_json::to_string(&MinSupport::Emulated).unwrap();
    assert_eq!(json, r#""emulated""#);
}

#[test]
fn agent_event_kind_uses_type_tag() {
    let kind = AgentEventKind::AssistantMessage {
        text: "hello".into(),
    };
    let event = sample_agent_event(kind);
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "assistant_message");
}

#[test]
fn agent_event_kind_run_started_tag() {
    let event = sample_agent_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "run_started");
}

#[test]
fn agent_event_kind_run_completed_tag() {
    let event = sample_agent_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "run_completed");
}

#[test]
fn agent_event_kind_assistant_delta_tag() {
    let event = sample_agent_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "assistant_delta");
}

#[test]
fn agent_event_kind_tool_call_tag() {
    let event = sample_agent_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool_call");
}

#[test]
fn agent_event_kind_tool_result_tag() {
    let event = sample_agent_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: None,
        output: json!({}),
        is_error: false,
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool_result");
}

#[test]
fn agent_event_kind_file_changed_tag() {
    let event = sample_agent_event(AgentEventKind::FileChanged {
        path: "f.rs".into(),
        summary: "new".into(),
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "file_changed");
}

#[test]
fn agent_event_kind_command_executed_tag() {
    let event = sample_agent_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(0),
        output_preview: None,
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "command_executed");
}

#[test]
fn agent_event_kind_warning_tag() {
    let event = sample_agent_event(AgentEventKind::Warning {
        message: "warn".into(),
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "warning");
}

#[test]
fn agent_event_kind_error_tag() {
    let event = sample_agent_event(AgentEventKind::Error {
        message: "err".into(),
        error_code: None,
    });
    let json: Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
}

// ===================================================================
// 5. Required fields present in all instances
// ===================================================================

#[test]
fn work_order_has_required_fields() {
    let wo = sample_work_order();
    let v: Value = serde_json::to_value(&wo).unwrap();
    assert!(v.get("id").is_some());
    assert!(v.get("task").is_some());
    assert!(v.get("lane").is_some());
    assert!(v.get("workspace").is_some());
    assert!(v.get("context").is_some());
    assert!(v.get("policy").is_some());
    assert!(v.get("requirements").is_some());
    assert!(v.get("config").is_some());
}

#[test]
fn receipt_has_required_fields() {
    let r = sample_receipt();
    let v: Value = serde_json::to_value(&r).unwrap();
    assert!(v.get("meta").is_some());
    assert!(v.get("backend").is_some());
    assert!(v.get("capabilities").is_some());
    assert!(v.get("usage_raw").is_some());
    assert!(v.get("usage").is_some());
    assert!(v.get("trace").is_some());
    assert!(v.get("artifacts").is_some());
    assert!(v.get("verification").is_some());
    assert!(v.get("outcome").is_some());
    assert!(v.get("receipt_sha256").is_some()); // null but present
}

#[test]
fn run_metadata_has_required_fields() {
    let rm = sample_run_metadata();
    let v: Value = serde_json::to_value(&rm).unwrap();
    assert!(v.get("run_id").is_some());
    assert!(v.get("work_order_id").is_some());
    assert!(v.get("contract_version").is_some());
    assert!(v.get("started_at").is_some());
    assert!(v.get("finished_at").is_some());
    assert!(v.get("duration_ms").is_some());
}

#[test]
fn workspace_spec_has_required_fields() {
    let ws = sample_workspace_spec();
    let v: Value = serde_json::to_value(&ws).unwrap();
    assert!(v.get("root").is_some());
    assert!(v.get("mode").is_some());
    assert!(v.get("include").is_some());
    assert!(v.get("exclude").is_some());
}

#[test]
fn backend_identity_has_required_fields() {
    let bi = sample_backend_identity();
    let v: Value = serde_json::to_value(&bi).unwrap();
    assert!(v.get("id").is_some());
    assert!(v.get("backend_version").is_some());
    assert!(v.get("adapter_version").is_some());
}

#[test]
fn context_snippet_has_required_fields() {
    let cs = ContextSnippet {
        name: "n".into(),
        content: "c".into(),
    };
    let v: Value = serde_json::to_value(&cs).unwrap();
    assert!(v.get("name").is_some());
    assert!(v.get("content").is_some());
}

#[test]
fn artifact_ref_has_required_fields() {
    let ar = sample_artifact_ref();
    let v: Value = serde_json::to_value(&ar).unwrap();
    assert!(v.get("kind").is_some());
    assert!(v.get("path").is_some());
}

#[test]
fn capability_requirement_has_required_fields() {
    let cr = CapabilityRequirement {
        capability: Capability::Streaming,
        min_support: MinSupport::Native,
    };
    let v: Value = serde_json::to_value(&cr).unwrap();
    assert!(v.get("capability").is_some());
    assert!(v.get("min_support").is_some());
}

// ===================================================================
// 6. Backward compatibility — old JSON with missing optional fields
// ===================================================================

#[test]
fn receipt_missing_mode_defaults() {
    // mode has #[serde(default)] so old JSON without it should work
    let r = sample_receipt();
    let mut v: Value = serde_json::to_value(&r).unwrap();
    v.as_object_mut().unwrap().remove("mode");
    let r2: Receipt = serde_json::from_value(v).unwrap();
    assert_eq!(r2.mode, ExecutionMode::Mapped); // default
}

#[test]
fn runtime_config_missing_optional_fields() {
    let json = json!({
        "model": null,
        "vendor": {},
        "env": {},
        "max_budget_usd": null,
        "max_turns": null,
    });
    let rc: RuntimeConfig = serde_json::from_value(json).unwrap();
    assert!(rc.model.is_none());
    assert!(rc.max_budget_usd.is_none());
    assert!(rc.max_turns.is_none());
}

#[test]
fn usage_normalized_all_none() {
    let json = json!({
        "input_tokens": null,
        "output_tokens": null,
        "cache_read_tokens": null,
        "cache_write_tokens": null,
        "request_units": null,
        "estimated_cost_usd": null,
    });
    let un: UsageNormalized = serde_json::from_value(json).unwrap();
    assert!(un.input_tokens.is_none());
    assert!(un.output_tokens.is_none());
}

#[test]
fn verification_report_all_none() {
    let json = json!({
        "git_diff": null,
        "git_status": null,
        "harness_ok": false,
    });
    let vr: VerificationReport = serde_json::from_value(json).unwrap();
    assert!(vr.git_diff.is_none());
    assert!(!vr.harness_ok);
}

#[test]
fn backend_identity_optional_versions_null() {
    let json = json!({
        "id": "test",
        "backend_version": null,
        "adapter_version": null,
    });
    let bi: BackendIdentity = serde_json::from_value(json).unwrap();
    assert_eq!(bi.id, "test");
    assert!(bi.backend_version.is_none());
    assert!(bi.adapter_version.is_none());
}

#[test]
fn agent_event_ext_missing_defaults_to_none() {
    // ext has skip_serializing_if and default, so missing is fine
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "assistant_message",
        "text": "hi"
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    assert!(e.ext.is_none());
}

#[test]
fn agent_event_with_ext_roundtrips() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    assert_serde_roundtrip(&event);
}

#[test]
fn tool_call_optional_ids_null() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "tool_call",
        "tool_name": "read",
        "tool_use_id": null,
        "parent_tool_use_id": null,
        "input": {}
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_use_id,
        parent_tool_use_id,
        ..
    } = &e.kind
    {
        assert!(tool_use_id.is_none());
        assert!(parent_tool_use_id.is_none());
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn tool_result_optional_id_null() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "tool_result",
        "tool_name": "read",
        "tool_use_id": null,
        "output": "ok",
        "is_error": false,
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    if let AgentEventKind::ToolResult { tool_use_id, .. } = &e.kind {
        assert!(tool_use_id.is_none());
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn command_executed_optional_fields_null() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "command_executed",
        "command": "echo hi",
        "exit_code": null,
        "output_preview": null,
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    if let AgentEventKind::CommandExecuted {
        exit_code,
        output_preview,
        ..
    } = &e.kind
    {
        assert!(exit_code.is_none());
        assert!(output_preview.is_none());
    } else {
        panic!("expected CommandExecuted");
    }
}

#[test]
fn error_event_optional_code_missing() {
    let json = json!({
        "ts": "2024-01-01T00:00:00Z",
        "type": "error",
        "message": "boom",
    });
    let e: AgentEvent = serde_json::from_value(json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert!(error_code.is_none());
    } else {
        panic!("expected Error");
    }
}

// ===================================================================
// 7. Schema stability — JSON structure validation
// ===================================================================

#[test]
fn work_order_json_schema_has_expected_keys() {
    let schema = schemars::schema_for!(WorkOrder);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("id"));
    assert!(props.contains_key("task"));
    assert!(props.contains_key("lane"));
    assert!(props.contains_key("workspace"));
    assert!(props.contains_key("context"));
    assert!(props.contains_key("policy"));
    assert!(props.contains_key("requirements"));
    assert!(props.contains_key("config"));
}

#[test]
fn receipt_json_schema_has_expected_keys() {
    let schema = schemars::schema_for!(Receipt);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("meta"));
    assert!(props.contains_key("backend"));
    assert!(props.contains_key("capabilities"));
    assert!(props.contains_key("usage_raw"));
    assert!(props.contains_key("usage"));
    assert!(props.contains_key("trace"));
    assert!(props.contains_key("artifacts"));
    assert!(props.contains_key("verification"));
    assert!(props.contains_key("outcome"));
    assert!(props.contains_key("receipt_sha256"));
}

#[test]
fn agent_event_json_schema_has_expected_keys() {
    let schema = schemars::schema_for!(AgentEvent);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("ts"));
}

#[test]
fn run_metadata_schema_has_expected_keys() {
    let schema = schemars::schema_for!(RunMetadata);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("run_id"));
    assert!(props.contains_key("work_order_id"));
    assert!(props.contains_key("contract_version"));
    assert!(props.contains_key("started_at"));
    assert!(props.contains_key("finished_at"));
    assert!(props.contains_key("duration_ms"));
}

#[test]
fn usage_normalized_schema_has_expected_keys() {
    let schema = schemars::schema_for!(UsageNormalized);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("input_tokens"));
    assert!(props.contains_key("output_tokens"));
    assert!(props.contains_key("cache_read_tokens"));
    assert!(props.contains_key("cache_write_tokens"));
    assert!(props.contains_key("request_units"));
    assert!(props.contains_key("estimated_cost_usd"));
}

#[test]
fn policy_profile_schema_has_expected_keys() {
    let schema = schemars::schema_for!(PolicyProfile);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("allowed_tools"));
    assert!(props.contains_key("disallowed_tools"));
    assert!(props.contains_key("deny_read"));
    assert!(props.contains_key("deny_write"));
    assert!(props.contains_key("allow_network"));
    assert!(props.contains_key("deny_network"));
    assert!(props.contains_key("require_approval_for"));
}

#[test]
fn workspace_spec_schema_has_expected_keys() {
    let schema = schemars::schema_for!(WorkspaceSpec);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("root"));
    assert!(props.contains_key("mode"));
    assert!(props.contains_key("include"));
    assert!(props.contains_key("exclude"));
}

#[test]
fn backend_identity_schema_has_expected_keys() {
    let schema = schemars::schema_for!(BackendIdentity);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("id"));
    assert!(props.contains_key("backend_version"));
    assert!(props.contains_key("adapter_version"));
}

#[test]
fn runtime_config_schema_has_expected_keys() {
    let schema = schemars::schema_for!(RuntimeConfig);
    let v: Value = serde_json::to_value(&schema).unwrap();
    let props = v["properties"].as_object().unwrap();
    assert!(props.contains_key("model"));
    assert!(props.contains_key("vendor"));
    assert!(props.contains_key("env"));
    assert!(props.contains_key("max_budget_usd"));
    assert!(props.contains_key("max_turns"));
}

// ===================================================================
// 8. Receipt hashing stability
// ===================================================================

#[test]
fn receipt_hash_is_deterministic() {
    let r = sample_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_is_64_hex_chars() {
    let r = sample_receipt();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_hash_ignores_receipt_sha256_field() {
    let r1 = sample_receipt();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("should_be_ignored".into());
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn receipt_with_hash_sets_field() {
    let r = sample_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let h = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_with_hash_matches_receipt_hash() {
    let r = sample_receipt();
    let expected = receipt_hash(&r).unwrap();
    let r_hashed = r.with_hash().unwrap();
    assert_eq!(r_hashed.receipt_sha256.as_ref().unwrap(), &expected);
}

// ===================================================================
// 9. Canonical JSON stability
// ===================================================================

#[test]
fn canonical_json_is_deterministic() {
    let val = json!({"z": 1, "a": 2, "m": 3});
    let c1 = canonical_json(&val).unwrap();
    let c2 = canonical_json(&val).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonical_json_sorts_keys() {
    let val = json!({"z": 1, "a": 2});
    let c = canonical_json(&val).unwrap();
    let a_pos = c.find("\"a\"").unwrap();
    let z_pos = c.find("\"z\"").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn sha256_hex_produces_64_chars() {
    let h = sha256_hex(b"test");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn sha256_hex_is_deterministic() {
    assert_eq!(sha256_hex(b"abc"), sha256_hex(b"abc"));
}

#[test]
fn sha256_hex_different_inputs_different_outputs() {
    assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
}

// ===================================================================
// 10. WorkOrder builder stability
// ===================================================================

#[test]
fn work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("test").build();
    assert_eq!(wo.task, "test");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert_eq!(wo.workspace.root, ".");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert!(wo.workspace.include.is_empty());
    assert!(wo.workspace.exclude.is_empty());
    assert!(wo.context.files.is_empty());
    assert!(wo.context.snippets.is_empty());
    assert!(wo.policy.allowed_tools.is_empty());
    assert!(wo.requirements.required.is_empty());
    assert!(wo.config.model.is_none());
}

#[test]
fn work_order_builder_all_fields() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["*.rs".into()])
        .exclude(vec!["target/*".into()])
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(2.0)
        .build();
    assert_eq!(wo.task, "task");
    assert!(matches!(wo.lane, ExecutionLane::WorkspaceFirst));
    assert_eq!(wo.workspace.root, "/tmp");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::PassThrough));
    assert_eq!(wo.workspace.include, vec!["*.rs"]);
    assert_eq!(wo.workspace.exclude, vec!["target/*"]);
    assert_eq!(wo.config.model, Some("gpt-4".into()));
    assert_eq!(wo.config.max_turns, Some(5));
    assert_eq!(wo.config.max_budget_usd, Some(2.0));
}

#[test]
fn work_order_builder_generates_unique_ids() {
    let wo1 = WorkOrderBuilder::new("a").build();
    let wo2 = WorkOrderBuilder::new("b").build();
    assert_ne!(wo1.id, wo2.id);
}

// ===================================================================
// 11. ReceiptBuilder stability
// ===================================================================

#[test]
fn receipt_builder_defaults() {
    let r = ReceiptBuilder::new("mock").build();
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn receipt_builder_with_outcome() {
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn receipt_builder_with_mode() {
    let r = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn receipt_builder_with_trace_event() {
    let event = sample_agent_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let r = ReceiptBuilder::new("mock").add_trace_event(event).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn receipt_builder_with_artifact() {
    let r = ReceiptBuilder::new("mock")
        .add_artifact(sample_artifact_ref())
        .build();
    assert_eq!(r.artifacts.len(), 1);
}

#[test]
fn receipt_builder_generates_unique_run_ids() {
    let r1 = ReceiptBuilder::new("mock").build();
    let r2 = ReceiptBuilder::new("mock").build();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[test]
fn receipt_builder_with_hash_method() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

// ===================================================================
// 12. SupportLevel::satisfies correctness
// ===================================================================

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
fn unsupported_does_not_satisfy_native() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
}

#[test]
fn unsupported_does_not_satisfy_emulated() {
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn restricted_does_not_satisfy_native() {
    let r = SupportLevel::Restricted { reason: "x".into() };
    assert!(!r.satisfies(&MinSupport::Native));
}

#[test]
fn restricted_satisfies_emulated() {
    let r = SupportLevel::Restricted { reason: "x".into() };
    assert!(r.satisfies(&MinSupport::Emulated));
}

// ===================================================================
// 13. Capability enum variant stability
// ===================================================================

#[test]
fn capability_streaming_serde() {
    let c = Capability::Streaming;
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(json, r#""streaming""#);
}

#[test]
fn capability_tool_read_serde() {
    let c = Capability::ToolRead;
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(json, r#""tool_read""#);
}

#[test]
fn capability_tool_write_serde() {
    let json = serde_json::to_string(&Capability::ToolWrite).unwrap();
    assert_eq!(json, r#""tool_write""#);
}

#[test]
fn capability_tool_edit_serde() {
    let json = serde_json::to_string(&Capability::ToolEdit).unwrap();
    assert_eq!(json, r#""tool_edit""#);
}

#[test]
fn capability_tool_bash_serde() {
    let json = serde_json::to_string(&Capability::ToolBash).unwrap();
    assert_eq!(json, r#""tool_bash""#);
}

#[test]
fn capability_tool_glob_serde() {
    let json = serde_json::to_string(&Capability::ToolGlob).unwrap();
    assert_eq!(json, r#""tool_glob""#);
}

#[test]
fn capability_tool_grep_serde() {
    let json = serde_json::to_string(&Capability::ToolGrep).unwrap();
    assert_eq!(json, r#""tool_grep""#);
}

#[test]
fn capability_mcp_client_serde() {
    let json = serde_json::to_string(&Capability::McpClient).unwrap();
    assert_eq!(json, r#""mcp_client""#);
}

#[test]
fn capability_mcp_server_serde() {
    let json = serde_json::to_string(&Capability::McpServer).unwrap();
    assert_eq!(json, r#""mcp_server""#);
}

#[test]
fn capability_tool_use_serde() {
    let json = serde_json::to_string(&Capability::ToolUse).unwrap();
    assert_eq!(json, r#""tool_use""#);
}

#[test]
fn capability_extended_thinking_serde() {
    let json = serde_json::to_string(&Capability::ExtendedThinking).unwrap();
    assert_eq!(json, r#""extended_thinking""#);
}

#[test]
fn capability_image_input_serde() {
    let json = serde_json::to_string(&Capability::ImageInput).unwrap();
    assert_eq!(json, r#""image_input""#);
}

#[test]
fn capability_checkpointing_serde() {
    let json = serde_json::to_string(&Capability::Checkpointing).unwrap();
    assert_eq!(json, r#""checkpointing""#);
}

#[test]
fn capability_session_resume_serde() {
    let json = serde_json::to_string(&Capability::SessionResume).unwrap();
    assert_eq!(json, r#""session_resume""#);
}

#[test]
fn capability_vision_serde() {
    let json = serde_json::to_string(&Capability::Vision).unwrap();
    assert_eq!(json, r#""vision""#);
}

#[test]
fn capability_json_mode_serde() {
    let json = serde_json::to_string(&Capability::JsonMode).unwrap();
    assert_eq!(json, r#""json_mode""#);
}

#[test]
fn capability_batch_mode_serde() {
    let json = serde_json::to_string(&Capability::BatchMode).unwrap();
    assert_eq!(json, r#""batch_mode""#);
}

// ===================================================================
// 14. BTreeMap deterministic serialization
// ===================================================================

#[test]
fn capability_manifest_serializes_deterministically() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::Streaming, SupportLevel::Emulated);
    let j1 = serde_json::to_string(&m).unwrap();
    let j2 = serde_json::to_string(&m).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn runtime_config_vendor_map_deterministic() {
    let mut rc = RuntimeConfig::default();
    rc.vendor.insert("z_key".into(), json!("z_val"));
    rc.vendor.insert("a_key".into(), json!("a_val"));
    let j = serde_json::to_string(&rc).unwrap();
    let a_pos = j.find("a_key").unwrap();
    let z_pos = j.find("z_key").unwrap();
    assert!(a_pos < z_pos);
}

#[test]
fn runtime_config_env_map_deterministic() {
    let mut rc = RuntimeConfig::default();
    rc.env.insert("ZZZ".into(), "1".into());
    rc.env.insert("AAA".into(), "2".into());
    let j = serde_json::to_string(&rc).unwrap();
    let a_pos = j.find("AAA").unwrap();
    let z_pos = j.find("ZZZ").unwrap();
    assert!(a_pos < z_pos);
}

// ===================================================================
// 15. Default implementations stability
// ===================================================================

#[test]
fn context_packet_default_is_empty() {
    let cp = ContextPacket::default();
    assert!(cp.files.is_empty());
    assert!(cp.snippets.is_empty());
}

#[test]
fn policy_profile_default_permits_all() {
    let pp = PolicyProfile::default();
    assert!(pp.allowed_tools.is_empty());
    assert!(pp.disallowed_tools.is_empty());
    assert!(pp.deny_read.is_empty());
    assert!(pp.deny_write.is_empty());
    assert!(pp.allow_network.is_empty());
    assert!(pp.deny_network.is_empty());
    assert!(pp.require_approval_for.is_empty());
}

#[test]
fn capability_requirements_default_is_empty() {
    let cr = CapabilityRequirements::default();
    assert!(cr.required.is_empty());
}

#[test]
fn runtime_config_default_all_none() {
    let rc = RuntimeConfig::default();
    assert!(rc.model.is_none());
    assert!(rc.vendor.is_empty());
    assert!(rc.env.is_empty());
    assert!(rc.max_budget_usd.is_none());
    assert!(rc.max_turns.is_none());
}

#[test]
fn usage_normalized_default_all_none() {
    let un = UsageNormalized::default();
    assert!(un.input_tokens.is_none());
    assert!(un.output_tokens.is_none());
    assert!(un.cache_read_tokens.is_none());
    assert!(un.cache_write_tokens.is_none());
    assert!(un.request_units.is_none());
    assert!(un.estimated_cost_usd.is_none());
}

#[test]
fn verification_report_default_is_empty() {
    let vr = VerificationReport::default();
    assert!(vr.git_diff.is_none());
    assert!(vr.git_status.is_none());
    assert!(!vr.harness_ok);
}

#[test]
fn execution_mode_default_value() {
    let em = ExecutionMode::default();
    assert_eq!(em, ExecutionMode::Mapped);
}

// ===================================================================
// 16. Complex type composition stability
// ===================================================================

#[test]
fn full_receipt_roundtrip_with_all_fields_populated() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let event = sample_agent_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("t1".into()),
        parent_tool_use_id: Some("p1".into()),
        input: json!({"path": "main.rs"}),
    });

    let r = ReceiptBuilder::new("sidecar:node")
        .outcome(Outcome::Partial)
        .mode(ExecutionMode::Passthrough)
        .backend_version("1.0.0")
        .adapter_version("0.2.0")
        .capabilities(caps)
        .usage(sample_usage_normalized())
        .usage_raw(json!({"tokens": 150}))
        .verification(sample_verification_report())
        .add_trace_event(event)
        .add_artifact(sample_artifact_ref())
        .build()
        .with_hash()
        .unwrap();

    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.mode, r2.mode);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.trace.len(), r2.trace.len());
    assert_eq!(r.artifacts.len(), r2.artifacts.len());
}

#[test]
fn work_order_full_roundtrip() {
    let wo = WorkOrderBuilder::new("refactor auth")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/home/user/project")
        .workspace_mode(WorkspaceMode::PassThrough)
        .include(vec!["src/**".into()])
        .exclude(vec!["target/**".into()])
        .context(sample_context_packet())
        .policy(sample_policy_profile())
        .requirements(sample_capability_requirements())
        .config(sample_runtime_config())
        .build();

    let json = serde_json::to_string_pretty(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();

    assert_eq!(wo.id, wo2.id);
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.workspace.root, wo2.workspace.root);
    assert_eq!(wo.context.files, wo2.context.files);
    assert_eq!(wo.policy.allowed_tools, wo2.policy.allowed_tools);
    assert_eq!(wo.config.model, wo2.config.model);
}

// ===================================================================
// 17. Edge cases and boundary tests
// ===================================================================

#[test]
fn empty_task_work_order() {
    let wo = WorkOrderBuilder::new("").build();
    assert_eq!(wo.task, "");
    assert_serde_roundtrip(&wo);
}

#[test]
fn unicode_task_work_order() {
    let wo = WorkOrderBuilder::new("修复认证模块 🔧").build();
    assert_eq!(wo.task, "修复认证模块 🔧");
    assert_serde_roundtrip(&wo);
}

#[test]
fn large_trace_receipt() {
    let events: Vec<AgentEvent> = (0..100)
        .map(|i| {
            sample_agent_event(AgentEventKind::AssistantDelta {
                text: format!("token_{}", i),
            })
        })
        .collect();

    let mut builder = ReceiptBuilder::new("mock");
    for e in events {
        builder = builder.add_trace_event(e);
    }
    let r = builder.build();
    assert_eq!(r.trace.len(), 100);
    assert_serde_roundtrip(&r);
}

#[test]
fn receipt_with_empty_usage_raw() {
    let r = ReceiptBuilder::new("mock").usage_raw(json!({})).build();
    assert_eq!(r.usage_raw, json!({}));
    assert_serde_roundtrip(&r);
}

#[test]
fn receipt_with_complex_usage_raw() {
    let raw = json!({
        "model": "gpt-4",
        "choices": [{"message": {"content": "hi"}}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5}
    });
    let r = ReceiptBuilder::new("mock").usage_raw(raw.clone()).build();
    assert_eq!(r.usage_raw, raw);
    assert_serde_roundtrip(&r);
}

#[test]
fn nil_uuid_work_order_id_in_receipt() {
    let r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .build();
    assert_eq!(r.meta.work_order_id, Uuid::nil());
}

#[test]
fn capability_manifest_with_all_support_levels() {
    let mut m = CapabilityManifest::new();
    m.insert(Capability::ToolRead, SupportLevel::Native);
    m.insert(Capability::ToolWrite, SupportLevel::Emulated);
    m.insert(Capability::ToolBash, SupportLevel::Unsupported);
    m.insert(
        Capability::ToolEdit,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    );
    assert_serde_roundtrip(&m);
    assert_eq!(m.len(), 4);
}

// ===================================================================
// 18. Deserialization from hardcoded JSON (wire format stability)
// ===================================================================

#[test]
fn deserialize_outcome_from_string() {
    let complete: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    assert_eq!(complete, Outcome::Complete);
    let partial: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    assert_eq!(partial, Outcome::Partial);
    let failed: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(failed, Outcome::Failed);
}

#[test]
fn deserialize_execution_mode_from_string() {
    let p: ExecutionMode = serde_json::from_str(r#""passthrough""#).unwrap();
    assert_eq!(p, ExecutionMode::Passthrough);
    let m: ExecutionMode = serde_json::from_str(r#""mapped""#).unwrap();
    assert_eq!(m, ExecutionMode::Mapped);
}

#[test]
fn deserialize_execution_lane_from_string() {
    let pf: ExecutionLane = serde_json::from_str(r#""patch_first""#).unwrap();
    assert!(matches!(pf, ExecutionLane::PatchFirst));
    let wf: ExecutionLane = serde_json::from_str(r#""workspace_first""#).unwrap();
    assert!(matches!(wf, ExecutionLane::WorkspaceFirst));
}

#[test]
fn deserialize_workspace_mode_from_string() {
    let pt: WorkspaceMode = serde_json::from_str(r#""pass_through""#).unwrap();
    assert!(matches!(pt, WorkspaceMode::PassThrough));
    let st: WorkspaceMode = serde_json::from_str(r#""staged""#).unwrap();
    assert!(matches!(st, WorkspaceMode::Staged));
}

#[test]
fn deserialize_hardcoded_assistant_message_event() {
    let json = r#"{
        "ts": "2024-06-15T12:00:00Z",
        "type": "assistant_message",
        "text": "Hello, world!"
    }"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::AssistantMessage { text } = &e.kind {
        assert_eq!(text, "Hello, world!");
    } else {
        panic!("expected AssistantMessage");
    }
}

#[test]
fn deserialize_hardcoded_tool_call_event() {
    let json = r#"{
        "ts": "2024-06-15T12:00:00Z",
        "type": "tool_call",
        "tool_name": "read_file",
        "tool_use_id": "tc_001",
        "parent_tool_use_id": null,
        "input": {"path": "src/main.rs"}
    }"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        input,
        ..
    } = &e.kind
    {
        assert_eq!(tool_name, "read_file");
        assert_eq!(tool_use_id.as_deref(), Some("tc_001"));
        assert_eq!(input["path"], "src/main.rs");
    } else {
        panic!("expected ToolCall");
    }
}

#[test]
fn deserialize_hardcoded_file_changed_event() {
    let json = r#"{
        "ts": "2024-06-15T12:00:00Z",
        "type": "file_changed",
        "path": "src/lib.rs",
        "summary": "Added new function"
    }"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::FileChanged { path, summary } = &e.kind {
        assert_eq!(path, "src/lib.rs");
        assert_eq!(summary, "Added new function");
    } else {
        panic!("expected FileChanged");
    }
}

#[test]
fn deserialize_hardcoded_warning_event() {
    let json = r#"{
        "ts": "2024-06-15T12:00:00Z",
        "type": "warning",
        "message": "Rate limit approaching"
    }"#;
    let e: AgentEvent = serde_json::from_str(json).unwrap();
    if let AgentEventKind::Warning { message } = &e.kind {
        assert_eq!(message, "Rate limit approaching");
    } else {
        panic!("expected Warning");
    }
}

// ===================================================================
// 19. Schema evolution: adding optional fields is backward-compatible
// ===================================================================

#[test]
fn work_order_extra_json_fields_ignored() {
    let wo = sample_work_order();
    let mut v: Value = serde_json::to_value(&wo).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("new_future_field".into(), json!("value"));
    // Should still deserialize without error (unknown fields ignored by default)
    let _wo2: WorkOrder = serde_json::from_value(v).unwrap();
}

#[test]
fn receipt_extra_json_fields_ignored() {
    let r = sample_receipt();
    let mut v: Value = serde_json::to_value(&r).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("future_field".into(), json!(42));
    let _r2: Receipt = serde_json::from_value(v).unwrap();
}

#[test]
fn runtime_config_extra_json_fields_ignored() {
    let rc = RuntimeConfig::default();
    let mut v: Value = serde_json::to_value(&rc).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("future_option".into(), json!(true));
    let _rc2: RuntimeConfig = serde_json::from_value(v).unwrap();
}

#[test]
fn backend_identity_extra_json_fields_ignored() {
    let bi = sample_backend_identity();
    let mut v: Value = serde_json::to_value(&bi).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("metadata".into(), json!({"region": "us-east"}));
    let _bi2: BackendIdentity = serde_json::from_value(v).unwrap();
}

#[test]
fn usage_normalized_extra_json_fields_ignored() {
    let un = UsageNormalized::default();
    let mut v: Value = serde_json::to_value(&un).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("future_counter".into(), json!(999));
    let _un2: UsageNormalized = serde_json::from_value(v).unwrap();
}

#[test]
fn policy_profile_extra_json_fields_ignored() {
    let pp = PolicyProfile::default();
    let mut v: Value = serde_json::to_value(&pp).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("future_policy".into(), json!([]));
    let _pp2: PolicyProfile = serde_json::from_value(v).unwrap();
}

#[test]
fn workspace_spec_extra_json_fields_ignored() {
    let ws = sample_workspace_spec();
    let mut v: Value = serde_json::to_value(&ws).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("future_setting".into(), json!("on"));
    let _ws2: WorkspaceSpec = serde_json::from_value(v).unwrap();
}

#[test]
fn verification_report_extra_json_fields_ignored() {
    let vr = VerificationReport::default();
    let mut v: Value = serde_json::to_value(&vr).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("coverage_pct".into(), json!(95.2));
    let _vr2: VerificationReport = serde_json::from_value(v).unwrap();
}

// ===================================================================
// 20. Cross-type consistency checks
// ===================================================================

#[test]
fn receipt_contract_version_matches_constant() {
    let r = ReceiptBuilder::new("any").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_work_order_id_propagates() {
    let wo = sample_work_order();
    let r = ReceiptBuilder::new("mock").work_order_id(wo.id).build();
    assert_eq!(r.meta.work_order_id, wo.id);
}

#[test]
fn multiple_outcomes_equality() {
    assert_eq!(Outcome::Complete, Outcome::Complete);
    assert_eq!(Outcome::Partial, Outcome::Partial);
    assert_eq!(Outcome::Failed, Outcome::Failed);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Partial, Outcome::Complete);
}

#[test]
fn execution_mode_equality() {
    assert_eq!(ExecutionMode::Passthrough, ExecutionMode::Passthrough);
    assert_eq!(ExecutionMode::Mapped, ExecutionMode::Mapped);
    assert_ne!(ExecutionMode::Passthrough, ExecutionMode::Mapped);
}

#[test]
fn capability_ord_is_consistent() {
    // Capability derives Ord, so BTreeMap keys are sorted.
    let a = Capability::Streaming;
    let b = Capability::ToolRead;
    // Just check that comparison doesn't panic and is consistent
    let cmp1 = a.cmp(&b);
    let cmp2 = a.cmp(&b);
    assert_eq!(cmp1, cmp2);
}

#[test]
fn capability_hash_is_consistent() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(Capability::ToolRead);
    set.insert(Capability::ToolRead);
    assert_eq!(set.len(), 1);
}
