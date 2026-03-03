// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep contract stability tests for ABP core types.
//!
//! These tests verify that serialization formats, field names, discriminators,
//! and hash computation remain stable across versions.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ContextPacket,
    ContextSnippet, ExecutionLane, ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt,
    ReceiptBuilder, RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder,
    WorkOrderBuilder, WorkspaceMode,
};
use abp_protocol::{Envelope, JsonlCodec};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// =========================================================================
// Helpers
// =========================================================================

fn fixed_time() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn sample_work_order() -> WorkOrder {
    WorkOrderBuilder::new("Test task")
        .lane(ExecutionLane::PatchFirst)
        .root("/tmp/ws")
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(1.0)
        .build()
}

fn sample_receipt() -> Receipt {
    let t = fixed_time();
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(t)
        .finished_at(t)
        .build()
}

fn sample_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_time(),
        kind,
        ext: None,
    }
}

// =========================================================================
// 1. WorkOrder field stability (15 tests)
// =========================================================================

#[test]
fn work_order_has_id_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert!(v.get("id").is_some(), "WorkOrder must have 'id' field");
}

#[test]
fn work_order_has_task_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["task"], "Test task");
}

#[test]
fn work_order_has_lane_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["lane"], "patch_first");
}

#[test]
fn work_order_lane_workspace_first_serialization() {
    let wo = WorkOrderBuilder::new("t")
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["lane"], "workspace_first");
}

#[test]
fn work_order_has_workspace_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    let ws = &v["workspace"];
    assert_eq!(ws["root"], "/tmp/ws");
    assert_eq!(ws["mode"], "staged");
    assert!(ws["include"].is_array());
    assert!(ws["exclude"].is_array());
}

#[test]
fn work_order_workspace_mode_pass_through() {
    let wo = WorkOrderBuilder::new("t")
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["workspace"]["mode"], "pass_through");
}

#[test]
fn work_order_has_context_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert!(v["context"]["files"].is_array());
    assert!(v["context"]["snippets"].is_array());
}

#[test]
fn work_order_has_policy_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    let p = &v["policy"];
    assert!(p["allowed_tools"].is_array());
    assert!(p["disallowed_tools"].is_array());
    assert!(p["deny_read"].is_array());
    assert!(p["deny_write"].is_array());
    assert!(p["allow_network"].is_array());
    assert!(p["deny_network"].is_array());
    assert!(p["require_approval_for"].is_array());
}

#[test]
fn work_order_has_requirements_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert!(v["requirements"]["required"].is_array());
}

#[test]
fn work_order_has_config_field() {
    let wo = sample_work_order();
    let v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    assert_eq!(v["config"]["model"], "gpt-4");
    assert_eq!(v["config"]["max_turns"], 5);
    assert_eq!(v["config"]["max_budget_usd"], 1.0);
}

#[test]
fn work_order_serde_roundtrip() {
    let wo = sample_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let wo2: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.task, wo2.task);
    assert_eq!(wo.id, wo2.id);
}

#[test]
fn work_order_builder_defaults() {
    let wo = WorkOrderBuilder::new("x").build();
    assert_eq!(wo.task, "x");
    assert!(matches!(wo.lane, ExecutionLane::PatchFirst));
    assert_eq!(wo.workspace.root, ".");
    assert!(matches!(wo.workspace.mode, WorkspaceMode::Staged));
    assert!(wo.config.model.is_none());
}

#[test]
fn work_order_builder_with_context() {
    let ctx = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "note".into(),
            content: "hello".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("t").context(ctx).build();
    assert_eq!(wo.context.files.len(), 1);
    assert_eq!(wo.context.snippets[0].name, "note");
}

#[test]
fn work_order_builder_with_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["**/.env".into()],
        deny_write: vec![],
        allow_network: vec![],
        deny_network: vec!["*.internal".into()],
        require_approval_for: vec!["write".into()],
    };
    let wo = WorkOrderBuilder::new("t").policy(policy).build();
    assert_eq!(wo.policy.allowed_tools, vec!["read"]);
    assert_eq!(wo.policy.deny_network, vec!["*.internal"]);
}

#[test]
fn work_order_builder_with_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = WorkOrderBuilder::new("t").requirements(reqs).build();
    assert_eq!(wo.requirements.required.len(), 1);
}

// =========================================================================
// 2. Receipt field stability (15 tests)
// =========================================================================

#[test]
fn receipt_has_meta_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    let meta = &v["meta"];
    assert!(meta.get("run_id").is_some());
    assert!(meta.get("work_order_id").is_some());
    assert_eq!(meta["contract_version"], CONTRACT_VERSION);
    assert!(meta.get("started_at").is_some());
    assert!(meta.get("finished_at").is_some());
    assert!(meta.get("duration_ms").is_some());
}

#[test]
fn receipt_has_backend_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["backend"]["id"], "mock");
}

#[test]
fn receipt_has_capabilities_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(v["capabilities"].is_object());
}

#[test]
fn receipt_has_mode_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["mode"], "mapped");
}

#[test]
fn receipt_has_usage_raw_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(v.get("usage_raw").is_some());
}

#[test]
fn receipt_has_usage_normalized_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    let u = &v["usage"];
    assert!(u.get("input_tokens").is_some());
    assert!(u.get("output_tokens").is_some());
    assert!(u.get("cache_read_tokens").is_some());
    assert!(u.get("cache_write_tokens").is_some());
    assert!(u.get("request_units").is_some());
    assert!(u.get("estimated_cost_usd").is_some());
}

#[test]
fn receipt_has_trace_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(v["trace"].is_array());
}

#[test]
fn receipt_has_artifacts_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(v["artifacts"].is_array());
}

#[test]
fn receipt_has_verification_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    let ver = &v["verification"];
    assert!(ver.get("harness_ok").is_some());
}

#[test]
fn receipt_has_outcome_field() {
    let r = sample_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert_eq!(v["outcome"], "complete");
}

#[test]
fn receipt_outcome_variants_serialize_snake_case() {
    assert_eq!(serde_json::to_value(Outcome::Complete).unwrap(), "complete");
    assert_eq!(serde_json::to_value(Outcome::Partial).unwrap(), "partial");
    assert_eq!(serde_json::to_value(Outcome::Failed).unwrap(), "failed");
}

#[test]
fn receipt_with_hash_fills_sha256() {
    let r = sample_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn receipt_hash_is_deterministic() {
    let t = fixed_time();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .started_at(t)
        .finished_at(t)
        .work_order_id(Uuid::nil())
        .build();
    let h1 = abp_core::receipt_hash(&r1).unwrap();
    let h2 = abp_core::receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn receipt_hash_excludes_sha256_field() {
    let r = sample_receipt();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let mut r2 = r;
    r2.receipt_sha256 = Some("bogus".into());
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2, "receipt_sha256 must not influence hash");
}

#[test]
fn receipt_serde_roundtrip() {
    let r = sample_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.backend.id, r2.backend.id);
}

// =========================================================================
// 3. AgentEvent stability (15 tests)
// =========================================================================

#[test]
fn agent_event_run_started_type_tag() {
    let e = sample_event(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "run_started");
    assert_eq!(v["message"], "go");
}

#[test]
fn agent_event_run_completed_type_tag() {
    let e = sample_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "run_completed");
}

#[test]
fn agent_event_assistant_delta_type_tag() {
    let e = sample_event(AgentEventKind::AssistantDelta { text: "tok".into() });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "assistant_delta");
    assert_eq!(v["text"], "tok");
}

#[test]
fn agent_event_assistant_message_type_tag() {
    let e = sample_event(AgentEventKind::AssistantMessage { text: "Hi".into() });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "assistant_message");
    assert_eq!(v["text"], "Hi");
}

#[test]
fn agent_event_tool_call_type_tag() {
    let e = sample_event(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: serde_json::json!({"path": "foo.rs"}),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "tool_call");
    assert_eq!(v["tool_name"], "read");
    assert_eq!(v["tool_use_id"], "tu-1");
    assert!(v["parent_tool_use_id"].is_null());
}

#[test]
fn agent_event_tool_result_type_tag() {
    let e = sample_event(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        output: serde_json::json!("contents"),
        is_error: false,
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "tool_result");
    assert!(!v["is_error"].as_bool().unwrap());
}

#[test]
fn agent_event_tool_result_error_flag() {
    let e = sample_event(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: serde_json::json!("fail"),
        is_error: true,
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert!(v["is_error"].as_bool().unwrap());
}

#[test]
fn agent_event_file_changed_type_tag() {
    let e = sample_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added fn".into(),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "file_changed");
    assert_eq!(v["path"], "src/main.rs");
}

#[test]
fn agent_event_command_executed_type_tag() {
    let e = sample_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "command_executed");
    assert_eq!(v["exit_code"], 0);
}

#[test]
fn agent_event_warning_type_tag() {
    let e = sample_event(AgentEventKind::Warning {
        message: "careful".into(),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "warning");
}

#[test]
fn agent_event_error_type_tag() {
    let e = sample_event(AgentEventKind::Error {
        message: "boom".into(),
        error_code: None,
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["message"], "boom");
}

#[test]
fn agent_event_error_with_error_code() {
    let e = sample_event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["error_code"], "backend_timeout");
}

#[test]
fn agent_event_serde_roundtrip() {
    let e = sample_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    });
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(e2.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn agent_event_has_ts_field() {
    let e = sample_event(AgentEventKind::RunStarted {
        message: "s".into(),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ts").is_some());
}

#[test]
fn agent_event_ext_field_omitted_when_none() {
    let e = sample_event(AgentEventKind::RunStarted {
        message: "s".into(),
    });
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ext").is_none(), "ext should be skipped when None");
}

// =========================================================================
// 4. Envelope stability (10 tests)
// =========================================================================

#[test]
fn envelope_hello_uses_t_discriminator() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn envelope_run_uses_t_discriminator() {
    let wo = sample_work_order();
    let env = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"run""#));
}

#[test]
fn envelope_event_uses_t_discriminator() {
    let event = sample_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let env = Envelope::Event {
        ref_id: "run-1".into(),
        event,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"event""#));
}

#[test]
fn envelope_final_uses_t_discriminator() {
    let r = sample_receipt();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: r,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"final""#));
}

#[test]
fn envelope_fatal_uses_t_discriminator() {
    let env = Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
}

#[test]
fn envelope_hello_roundtrip() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "node".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn envelope_hello_contains_contract_version() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let v: serde_json::Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["contract_version"], CONTRACT_VERSION);
}

#[test]
fn envelope_event_has_ref_id() {
    let event = sample_event(AgentEventKind::Warning {
        message: "w".into(),
    });
    let env = Envelope::Event {
        ref_id: "r-42".into(),
        event,
    };
    let v: serde_json::Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["ref_id"], "r-42");
}

#[test]
fn envelope_fatal_with_error_code() {
    let env = Envelope::fatal_with_code(
        Some("r-1".into()),
        "timed out",
        abp_error::ErrorCode::BackendTimeout,
    );
    let v: serde_json::Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["error_code"], "backend_timeout");
    assert_eq!(env.error_code(), Some(abp_error::ErrorCode::BackendTimeout));
}

#[test]
fn envelope_fatal_ref_id_nullable() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "early fail".into(),
        error_code: None,
    };
    let v: serde_json::Value = serde_json::to_value(&env).unwrap();
    assert!(v["ref_id"].is_null());
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
        _ => panic!("expected Fatal"),
    }
}

// =========================================================================
// 5. Schema backward compatibility (15 tests)
// =========================================================================

#[test]
fn unknown_fields_ignored_on_work_order_deserialization() {
    let wo = sample_work_order();
    let mut v: serde_json::Value = serde_json::to_value(&wo).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("future_field".into(), serde_json::json!("whatever"));
    let wo2: WorkOrder = serde_json::from_value(v).unwrap();
    assert_eq!(wo2.task, "Test task");
}

#[test]
fn unknown_fields_ignored_on_receipt_deserialization() {
    let r = sample_receipt();
    let mut v: serde_json::Value = serde_json::to_value(&r).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("new_metric".into(), serde_json::json!(99));
    let r2: Receipt = serde_json::from_value(v).unwrap();
    assert_eq!(r2.backend.id, "mock");
}

#[test]
fn unknown_fields_ignored_on_envelope_deserialization() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"boom","unknown_field":42}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn version_parsing() {
    assert_eq!(abp_protocol::parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(abp_protocol::parse_version("abp/v1.0"), Some((1, 0)));
    assert_eq!(abp_protocol::parse_version("invalid"), None);
}

#[test]
fn version_compatibility_same_major() {
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(abp_protocol::is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn version_incompatibility_different_major() {
    assert!(!abp_protocol::is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serialization() {
    assert_eq!(
        serde_json::to_value(ExecutionMode::Mapped).unwrap(),
        "mapped"
    );
    assert_eq!(
        serde_json::to_value(ExecutionMode::Passthrough).unwrap(),
        "passthrough"
    );
}

#[test]
fn capability_serializes_snake_case() {
    assert_eq!(
        serde_json::to_value(Capability::ToolRead).unwrap(),
        "tool_read"
    );
    assert_eq!(
        serde_json::to_value(Capability::Streaming).unwrap(),
        "streaming"
    );
    assert_eq!(
        serde_json::to_value(Capability::ExtendedThinking).unwrap(),
        "extended_thinking"
    );
}

#[test]
fn support_level_satisfies_semantics() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
}

#[test]
fn error_code_serializes_snake_case() {
    let code = abp_error::ErrorCode::BackendTimeout;
    let v = serde_json::to_value(code).unwrap();
    assert_eq!(v, "backend_timeout");
}

#[test]
fn error_code_display_uses_message() {
    let code = abp_error::ErrorCode::BackendTimeout;
    assert_eq!(code.to_string(), code.message());
    assert_eq!(code.to_string(), "backend timed out");
}

#[test]
fn error_code_as_str_matches_serde() {
    let code = abp_error::ErrorCode::PolicyDenied;
    let serde_str = serde_json::to_value(code).unwrap();
    assert_eq!(code.as_str(), serde_str.as_str().unwrap());
}

#[test]
fn btreemap_produces_deterministic_json() {
    let mut map = BTreeMap::new();
    map.insert("z_key".to_string(), serde_json::json!("last"));
    map.insert("a_key".to_string(), serde_json::json!("first"));
    let json = serde_json::to_string(&map).unwrap();
    assert!(
        json.find("a_key").unwrap() < json.find("z_key").unwrap(),
        "BTreeMap keys must serialize in sorted order"
    );
}

// =========================================================================
// Additional cross-cutting stability tests
// =========================================================================

#[test]
fn capability_manifest_is_btreemap() {
    let mut manifest = CapabilityManifest::new();
    manifest.insert(Capability::ToolRead, SupportLevel::Native);
    manifest.insert(Capability::Streaming, SupportLevel::Emulated);
    let json = serde_json::to_string(&manifest).unwrap();
    // BTreeMap guarantees sorted keys
    assert!(
        json.find("streaming").unwrap() < json.find("tool_read").unwrap(),
        "Capability manifest must use sorted keys"
    );
}

#[test]
fn receipt_builder_sets_contract_version() {
    let r = sample_receipt();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_builder_chain_with_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .with_hash()
        .unwrap();
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn receipt_with_trace_events() {
    let e1 = sample_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    let e2 = sample_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(e1)
        .add_trace_event(e2)
        .build();
    assert_eq!(r.trace.len(), 2);
}

#[test]
fn receipt_with_artifacts() {
    let r = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn envelope_hello_mode_defaults_to_mapped() {
    let env = Envelope::hello(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    match env {
        Envelope::Hello { mode, .. } => assert_eq!(mode, ExecutionMode::Mapped),
        _ => panic!("expected Hello"),
    }
}

#[test]
fn envelope_hello_with_explicit_passthrough_mode() {
    let env = Envelope::hello_with_mode(
        BackendIdentity {
            id: "x".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let v: serde_json::Value = serde_json::to_value(&env).unwrap();
    assert_eq!(v["mode"], "passthrough");
}

#[test]
fn envelope_jsonl_ends_with_newline() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "x".into(),
        error_code: None,
    };
    let line = JsonlCodec::encode(&env).unwrap();
    assert!(line.ends_with('\n'));
}

#[test]
fn agent_event_ext_field_preserved_when_present() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        serde_json::json!({"role": "assistant"}),
    );
    let e = AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();
    assert!(v.get("ext").is_some());
    let json = serde_json::to_string(&e).unwrap();
    let e2: AgentEvent = serde_json::from_str(&json).unwrap();
    assert!(e2.ext.is_some());
    assert_eq!(e2.ext.unwrap()["raw_message"]["role"], "assistant");
}

#[test]
fn runtime_config_vendor_uses_btreemap() {
    let mut vendor = BTreeMap::new();
    vendor.insert("z_vendor".to_string(), serde_json::json!(1));
    vendor.insert("a_vendor".to_string(), serde_json::json!(2));
    let config = RuntimeConfig {
        model: None,
        vendor,
        env: BTreeMap::new(),
        max_budget_usd: None,
        max_turns: None,
    };
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.find("a_vendor").unwrap() < json.find("z_vendor").unwrap());
}

#[test]
fn verification_report_default() {
    let v = VerificationReport::default();
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
    assert!(!v.harness_ok);
}

#[test]
fn usage_normalized_default() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn error_code_category_mapping() {
    use abp_error::{ErrorCategory, ErrorCode};
    assert_eq!(
        ErrorCode::ProtocolInvalidEnvelope.category(),
        ErrorCategory::Protocol
    );
    assert_eq!(
        ErrorCode::BackendNotFound.category(),
        ErrorCategory::Backend
    );
    assert_eq!(ErrorCode::PolicyDenied.category(), ErrorCategory::Policy);
    assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
}

#[test]
fn error_code_retryable() {
    use abp_error::ErrorCode;
    assert!(ErrorCode::BackendTimeout.is_retryable());
    assert!(ErrorCode::BackendRateLimited.is_retryable());
    assert!(!ErrorCode::PolicyDenied.is_retryable());
    assert!(!ErrorCode::Internal.is_retryable());
}

#[test]
fn error_code_serde_roundtrip() {
    use abp_error::ErrorCode;
    let code = ErrorCode::ProtocolHandshakeFailed;
    let json = serde_json::to_string(&code).unwrap();
    let code2: ErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(code, code2);
}

#[test]
fn error_info_serialization() {
    use abp_error::{ErrorCode, ErrorInfo};
    let info =
        ErrorInfo::new(ErrorCode::BackendTimeout, "timed out").with_detail("backend", "openai");
    let json = serde_json::to_string(&info).unwrap();
    let info2: ErrorInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, info2);
    assert_eq!(info2.code, ErrorCode::BackendTimeout);
    assert!(info2.is_retryable);
}

#[test]
fn canonical_json_key_order() {
    let json = abp_core::canonical_json(&serde_json::json!({"b": 2, "a": 1})).unwrap();
    assert!(json.starts_with(r#"{"a":1"#));
}

#[test]
fn sha256_hex_length() {
    let hex = abp_core::sha256_hex(b"test");
    assert_eq!(hex.len(), 64);
}
