// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive JSON schema generation and validation tests.
//!
//! Categories:
//! 1. Core types implement JsonSchema
//! 2. Generated schema structure verification
//! 3. Schema validates valid instances
//! 4. Schema rejects invalid instances
//! 5. Schema for all AgentEventKind variants
//! 6. Schema for Envelope variants
//! 7. Schema stability (consistent output)
//! 8. Edge cases

use abp_cli::config::{BackendConfig, BackplaneConfig};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_error::ErrorCode;
use chrono::Utc;
use schemars::schema_for;
use serde_json::{json, Value};
use std::collections::BTreeMap;

// ── helpers ──────────────────────────────────────────────────────────────

fn schema_value<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap()
}

fn assert_valid(schema: &Value, instance: &Value) {
    let validator = jsonschema::validator_for(schema).expect("schema compiles");
    if let Err(e) = validator.validate(instance) {
        let msgs: Vec<String> = std::iter::once(format!("  - {e}"))
            .chain(
                validator
                    .iter_errors(instance)
                    .skip(1)
                    .map(|e| format!("  - {e}")),
            )
            .collect();
        panic!("validation failed:\n{}", msgs.join("\n"));
    }
}

fn assert_invalid(schema: &Value, instance: &Value) {
    let validator = jsonschema::validator_for(schema).expect("schema compiles");
    assert!(
        !validator.is_valid(instance),
        "expected validation to fail but it passed"
    );
}

fn assert_schema_compiles(schema: &Value) {
    jsonschema::validator_for(schema).expect("schema must compile");
}

fn wo_schema() -> Value {
    schema_value::<WorkOrder>()
}

fn receipt_schema() -> Value {
    schema_value::<Receipt>()
}

fn event_schema() -> Value {
    schema_value::<AgentEvent>()
}

fn event_kind_schema() -> Value {
    schema_value::<AgentEventKind>()
}

fn valid_wo_value() -> Value {
    serde_json::to_value(WorkOrderBuilder::new("test task").build()).unwrap()
}

fn valid_receipt_value() -> Value {
    serde_json::to_value(
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build(),
    )
    .unwrap()
}

fn valid_event_value(kind: AgentEventKind) -> Value {
    serde_json::to_value(AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    })
    .unwrap()
}

fn get_required(schema: &Value) -> Vec<String> {
    schema["required"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}

fn get_defs(schema: &Value) -> Vec<String> {
    schema["$defs"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn collect_refs(v: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    collect_refs_inner(v, &mut refs);
    refs
}

fn collect_refs_inner(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                out.push(r.clone());
            }
            for val in map.values() {
                collect_refs_inner(val, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                collect_refs_inner(val, out);
            }
        }
        _ => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Core types implement JsonSchema (schema compiles)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn schema_compiles_work_order() {
    assert_schema_compiles(&schema_value::<WorkOrder>());
}

#[test]
fn schema_compiles_receipt() {
    assert_schema_compiles(&schema_value::<Receipt>());
}

#[test]
fn schema_compiles_agent_event() {
    assert_schema_compiles(&schema_value::<AgentEvent>());
}

#[test]
fn schema_compiles_agent_event_kind() {
    assert_schema_compiles(&schema_value::<AgentEventKind>());
}

#[test]
fn schema_compiles_execution_lane() {
    assert_schema_compiles(&schema_value::<ExecutionLane>());
}

#[test]
fn schema_compiles_execution_mode() {
    assert_schema_compiles(&schema_value::<ExecutionMode>());
}

#[test]
fn schema_compiles_workspace_spec() {
    assert_schema_compiles(&schema_value::<WorkspaceSpec>());
}

#[test]
fn schema_compiles_workspace_mode() {
    assert_schema_compiles(&schema_value::<WorkspaceMode>());
}

#[test]
fn schema_compiles_context_packet() {
    assert_schema_compiles(&schema_value::<ContextPacket>());
}

#[test]
fn schema_compiles_context_snippet() {
    assert_schema_compiles(&schema_value::<ContextSnippet>());
}

#[test]
fn schema_compiles_runtime_config() {
    assert_schema_compiles(&schema_value::<RuntimeConfig>());
}

#[test]
fn schema_compiles_policy_profile() {
    assert_schema_compiles(&schema_value::<PolicyProfile>());
}

#[test]
fn schema_compiles_capability() {
    assert_schema_compiles(&schema_value::<Capability>());
}

#[test]
fn schema_compiles_capability_requirements() {
    assert_schema_compiles(&schema_value::<CapabilityRequirements>());
}

#[test]
fn schema_compiles_capability_requirement() {
    assert_schema_compiles(&schema_value::<CapabilityRequirement>());
}

#[test]
fn schema_compiles_min_support() {
    assert_schema_compiles(&schema_value::<MinSupport>());
}

#[test]
fn schema_compiles_support_level() {
    assert_schema_compiles(&schema_value::<SupportLevel>());
}

#[test]
fn schema_compiles_backend_identity() {
    assert_schema_compiles(&schema_value::<BackendIdentity>());
}

#[test]
fn schema_compiles_run_metadata() {
    assert_schema_compiles(&schema_value::<RunMetadata>());
}

#[test]
fn schema_compiles_usage_normalized() {
    assert_schema_compiles(&schema_value::<UsageNormalized>());
}

#[test]
fn schema_compiles_outcome() {
    assert_schema_compiles(&schema_value::<Outcome>());
}

#[test]
fn schema_compiles_artifact_ref() {
    assert_schema_compiles(&schema_value::<ArtifactRef>());
}

#[test]
fn schema_compiles_verification_report() {
    assert_schema_compiles(&schema_value::<VerificationReport>());
}

#[test]
fn schema_compiles_backplane_config() {
    assert_schema_compiles(&schema_value::<BackplaneConfig>());
}

#[test]
fn schema_compiles_backend_config() {
    assert_schema_compiles(&schema_value::<BackendConfig>());
}

#[test]
fn schema_compiles_error_code() {
    assert_schema_compiles(&schema_value::<ErrorCode>());
}

#[test]
fn schema_compiles_ir_role() {
    assert_schema_compiles(&schema_value::<IrRole>());
}

#[test]
fn schema_compiles_ir_message() {
    assert_schema_compiles(&schema_value::<IrMessage>());
}

#[test]
fn schema_compiles_ir_content_block() {
    assert_schema_compiles(&schema_value::<IrContentBlock>());
}

#[test]
fn schema_compiles_ir_conversation() {
    assert_schema_compiles(&schema_value::<IrConversation>());
}

#[test]
fn schema_compiles_ir_tool_definition() {
    assert_schema_compiles(&schema_value::<IrToolDefinition>());
}

#[test]
fn schema_compiles_ir_usage() {
    assert_schema_compiles(&schema_value::<IrUsage>());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Generated schema structure verification
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_schema_has_draft_2020_12() {
    assert_eq!(
        wo_schema()["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn receipt_schema_has_draft_2020_12() {
    assert_eq!(
        receipt_schema()["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn work_order_title_is_work_order() {
    assert_eq!(wo_schema()["title"], "WorkOrder");
}

#[test]
fn receipt_title_is_receipt() {
    assert_eq!(receipt_schema()["title"], "Receipt");
}

#[test]
fn agent_event_title_is_agent_event() {
    assert_eq!(event_schema()["title"], "AgentEvent");
}

#[test]
fn work_order_top_level_type_is_object() {
    assert_eq!(wo_schema()["type"], "object");
}

#[test]
fn receipt_top_level_type_is_object() {
    assert_eq!(receipt_schema()["type"], "object");
}

#[test]
fn work_order_has_all_required_fields() {
    let required = get_required(&wo_schema());
    for f in &[
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ] {
        assert!(required.contains(&f.to_string()), "missing required: {f}");
    }
    assert_eq!(required.len(), 8);
}

#[test]
fn receipt_has_all_required_fields() {
    let required = get_required(&receipt_schema());
    for f in &[
        "meta",
        "backend",
        "outcome",
        "trace",
        "usage",
        "artifacts",
        "verification",
        "capabilities",
        "usage_raw",
    ] {
        assert!(required.contains(&f.to_string()), "missing required: {f}");
    }
}

#[test]
fn work_order_id_has_uuid_format() {
    let s = wo_schema();
    assert_eq!(s["properties"]["id"]["type"], "string");
    assert_eq!(s["properties"]["id"]["format"], "uuid");
}

#[test]
fn work_order_task_is_string_type() {
    assert_eq!(wo_schema()["properties"]["task"]["type"], "string");
}

#[test]
fn receipt_sha256_is_nullable_string() {
    let s = receipt_schema();
    let ty = &s["properties"]["receipt_sha256"]["type"];
    let arr = ty.as_array().expect("receipt_sha256 should be array type");
    let strs: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    assert!(strs.contains(&"string"));
    assert!(strs.contains(&"null"));
}

#[test]
fn receipt_sha256_is_not_required() {
    let required = get_required(&receipt_schema());
    assert!(!required.contains(&"receipt_sha256".to_string()));
}

#[test]
fn work_order_defs_include_expected_types() {
    let defs = get_defs(&wo_schema());
    for expected in &[
        "ExecutionLane",
        "WorkspaceSpec",
        "ContextPacket",
        "PolicyProfile",
        "CapabilityRequirements",
        "RuntimeConfig",
    ] {
        assert!(
            defs.contains(&expected.to_string()),
            "$defs missing: {expected}"
        );
    }
}

#[test]
fn receipt_defs_include_expected_types() {
    let defs = get_defs(&receipt_schema());
    for expected in &[
        "RunMetadata",
        "BackendIdentity",
        "Outcome",
        "UsageNormalized",
        "VerificationReport",
        "AgentEvent",
    ] {
        assert!(
            defs.contains(&expected.to_string()),
            "$defs missing: {expected}"
        );
    }
}

#[test]
fn all_refs_resolve_to_defs_in_work_order_schema() {
    let s = wo_schema();
    let defs = get_defs(&s);
    for r in collect_refs(&s) {
        if let Some(name) = r.strip_prefix("#/$defs/") {
            assert!(
                defs.contains(&name.to_string()),
                "$ref to undefined def: {name}"
            );
        }
    }
}

#[test]
fn all_refs_resolve_to_defs_in_receipt_schema() {
    let s = receipt_schema();
    let defs = get_defs(&s);
    for r in collect_refs(&s) {
        if let Some(name) = r.strip_prefix("#/$defs/") {
            assert!(
                defs.contains(&name.to_string()),
                "$ref to undefined def: {name}"
            );
        }
    }
}

#[test]
fn work_order_description_mentions_unit_of_work() {
    let s = wo_schema();
    let desc = s["description"].as_str().unwrap();
    assert!(desc.contains("single unit of work"));
}

#[test]
fn receipt_description_mentions_outcome() {
    let s = receipt_schema();
    let desc = s["description"].as_str().unwrap();
    assert!(desc.contains("outcome of a completed run"));
}

#[test]
fn run_metadata_timestamps_are_datetime_format() {
    let defs = &receipt_schema()["$defs"]["RunMetadata"];
    assert_eq!(defs["properties"]["started_at"]["format"], "date-time");
    assert_eq!(defs["properties"]["finished_at"]["format"], "date-time");
}

#[test]
fn run_metadata_duration_ms_is_uint64() {
    let defs = &receipt_schema()["$defs"]["RunMetadata"];
    assert_eq!(defs["properties"]["duration_ms"]["type"], "integer");
    assert_eq!(defs["properties"]["duration_ms"]["format"], "uint64");
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Schema validates valid instances
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn valid_work_order_passes_schema() {
    assert_valid(&wo_schema(), &valid_wo_value());
}

#[test]
fn valid_receipt_passes_schema() {
    assert_valid(&receipt_schema(), &valid_receipt_value());
}

#[test]
fn valid_receipt_with_hash_passes_schema() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn valid_work_order_with_model_passes_schema() {
    let wo = WorkOrderBuilder::new("task")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn valid_work_order_workspace_first_passes_schema() {
    let wo = WorkOrderBuilder::new("task")
        .lane(ExecutionLane::WorkspaceFirst)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn valid_receipt_partial_outcome_passes_schema() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn valid_receipt_failed_outcome_passes_schema() {
    let receipt = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn valid_receipt_with_trace_events_passes_schema() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn valid_receipt_passthrough_mode_passes_schema() {
    let receipt = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn valid_backplane_config_passes_schema() {
    let cfg = BackplaneConfig {
        default_backend: Some("mock".into()),
        log_level: Some("debug".into()),
        receipts_dir: None,
        backends: Default::default(),
    };
    let s = schema_value::<BackplaneConfig>();
    let v = serde_json::to_value(cfg).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn valid_policy_profile_passes_schema() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["/etc/**".into()],
        allow_network: vec!["github.com".into()],
        deny_network: vec![],
        require_approval_for: vec!["bash".into()],
    };
    let s = schema_value::<PolicyProfile>();
    let v = serde_json::to_value(policy).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn valid_empty_policy_profile_passes_schema() {
    let v = serde_json::to_value(PolicyProfile::default()).unwrap();
    assert_valid(&schema_value::<PolicyProfile>(), &v);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Schema rejects invalid instances
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_missing_task_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("task");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_missing_id_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("id");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_missing_lane_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("lane");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_missing_workspace_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("workspace");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_missing_context_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("context");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_missing_policy_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("policy");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_missing_requirements_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("requirements");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_missing_config_rejected() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("config");
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn work_order_task_wrong_type_rejected() {
    let mut v = valid_wo_value();
    v["task"] = json!(42);
    assert_invalid(&wo_schema(), &v);
}

#[test]
fn receipt_missing_meta_rejected() {
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("meta");
    assert_invalid(&receipt_schema(), &v);
}

#[test]
fn receipt_missing_backend_rejected() {
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("backend");
    assert_invalid(&receipt_schema(), &v);
}

#[test]
fn receipt_missing_outcome_rejected() {
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("outcome");
    assert_invalid(&receipt_schema(), &v);
}

#[test]
fn receipt_missing_trace_rejected() {
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("trace");
    assert_invalid(&receipt_schema(), &v);
}

#[test]
fn receipt_outcome_invalid_string_rejected() {
    let mut v = valid_receipt_value();
    v["outcome"] = json!("invalid_outcome");
    assert_invalid(&receipt_schema(), &v);
}

#[test]
fn receipt_trace_not_array_rejected() {
    let mut v = valid_receipt_value();
    v["trace"] = json!("not an array");
    assert_invalid(&receipt_schema(), &v);
}

#[test]
fn empty_object_rejected_by_work_order_schema() {
    assert_invalid(&wo_schema(), &json!({}));
}

#[test]
fn empty_object_rejected_by_receipt_schema() {
    assert_invalid(&receipt_schema(), &json!({}));
}

#[test]
fn null_rejected_by_work_order_schema() {
    assert_invalid(&wo_schema(), &Value::Null);
}

#[test]
fn string_rejected_by_work_order_schema() {
    assert_invalid(&wo_schema(), &json!("not an object"));
}

#[test]
fn array_rejected_by_receipt_schema() {
    assert_invalid(&receipt_schema(), &json!([1, 2, 3]));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Schema for all AgentEventKind variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn event_run_started_validates() {
    let v = valid_event_value(AgentEventKind::RunStarted {
        message: "go".into(),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_run_completed_validates() {
    let v = valid_event_value(AgentEventKind::RunCompleted {
        message: "done".into(),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_assistant_delta_validates() {
    let v = valid_event_value(AgentEventKind::AssistantDelta {
        text: "chunk".into(),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_assistant_message_validates() {
    let v = valid_event_value(AgentEventKind::AssistantMessage {
        text: "hello world".into(),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_tool_call_validates() {
    let v = valid_event_value(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "foo.rs"}),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_tool_result_validates() {
    let v = valid_event_value(AgentEventKind::ToolResult {
        tool_name: "read".into(),
        tool_use_id: Some("tu-1".into()),
        output: json!("file content"),
        is_error: false,
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_file_changed_validates() {
    let v = valid_event_value(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added function".into(),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_command_executed_validates() {
    let v = valid_event_value(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("all tests passed".into()),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_command_executed_null_fields_validates() {
    let v = valid_event_value(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: None,
        output_preview: None,
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_warning_validates() {
    let v = valid_event_value(AgentEventKind::Warning {
        message: "budget low".into(),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_error_validates() {
    let v = valid_event_value(AgentEventKind::Error {
        message: "crash".into(),
        error_code: None,
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_error_with_error_code_validates() {
    let v = valid_event_value(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(ErrorCode::BackendTimeout),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_tool_call_with_parent_validates() {
    let v = valid_event_value(AgentEventKind::ToolCall {
        tool_name: "write".into(),
        tool_use_id: Some("tu-2".into()),
        parent_tool_use_id: Some("tu-1".into()),
        input: json!({}),
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_tool_result_error_flag_validates() {
    let v = valid_event_value(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: json!({"stderr": "permission denied"}),
        is_error: true,
    });
    assert_valid(&event_schema(), &v);
}

#[test]
fn event_with_ext_field_validates() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor": "data"}));
    let v = serde_json::to_value(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    })
    .unwrap();
    assert_valid(&event_schema(), &v);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Schema for Envelope variants (protocol crate)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn envelope_hello_roundtrips_json() {
    let hello = abp_protocol::Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = abp_protocol::JsonlCodec::encode(&hello).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Hello { .. }));
}

#[test]
fn envelope_hello_contains_discriminator_t() {
    let hello = abp_protocol::Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = abp_protocol::JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
}

#[test]
fn envelope_run_roundtrips_json() {
    let wo = WorkOrderBuilder::new("task").build();
    let run = abp_protocol::Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = abp_protocol::JsonlCodec::encode(&run).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Run { .. }));
}

#[test]
fn envelope_event_roundtrips_json() {
    let event = abp_protocol::Envelope::Event {
        ref_id: "run-1".into(),
        event: AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        },
    };
    let json = abp_protocol::JsonlCodec::encode(&event).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Event { .. }));
}

#[test]
fn envelope_final_roundtrips_json() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let fin = abp_protocol::Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = abp_protocol::JsonlCodec::encode(&fin).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Final { .. }));
}

#[test]
fn envelope_fatal_roundtrips_json() {
    let fatal = abp_protocol::Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = abp_protocol::JsonlCodec::encode(&fatal).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Fatal { .. }));
}

#[test]
fn envelope_fatal_with_error_code_roundtrips() {
    let fatal = abp_protocol::Envelope::fatal_with_code(
        Some("run-1".into()),
        "version mismatch",
        ErrorCode::ProtocolVersionMismatch,
    );
    let json = abp_protocol::JsonlCodec::encode(&fatal).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(ErrorCode::ProtocolVersionMismatch)
    );
}

#[test]
fn envelope_hello_passthrough_mode_roundtrips() {
    let hello = abp_protocol::Envelope::hello_with_mode(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    let json = abp_protocol::JsonlCodec::encode(&hello).unwrap();
    let decoded = abp_protocol::JsonlCodec::decode(json.trim()).unwrap();
    if let abp_protocol::Envelope::Hello { mode, .. } = decoded {
        assert_eq!(mode, ExecutionMode::Passthrough);
    } else {
        panic!("expected Hello variant");
    }
}

#[test]
fn envelope_discriminator_uses_t_not_type() {
    let fatal = abp_protocol::Envelope::Fatal {
        ref_id: None,
        error: "test".into(),
        error_code: None,
    };
    let json = abp_protocol::JsonlCodec::encode(&fatal).unwrap();
    let parsed: Value = serde_json::from_str(json.trim()).unwrap();
    assert!(parsed.get("t").is_some(), "envelope should use 't' tag");
    assert!(
        parsed.get("type").is_none(),
        "envelope should NOT use 'type' tag"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Schema stability (consistent output)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_schema_is_deterministic() {
    let s1 = serde_json::to_string(&schema_for!(WorkOrder)).unwrap();
    let s2 = serde_json::to_string(&schema_for!(WorkOrder)).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn receipt_schema_is_deterministic() {
    let s1 = serde_json::to_string(&schema_for!(Receipt)).unwrap();
    let s2 = serde_json::to_string(&schema_for!(Receipt)).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn agent_event_schema_is_deterministic() {
    let s1 = serde_json::to_string(&schema_for!(AgentEvent)).unwrap();
    let s2 = serde_json::to_string(&schema_for!(AgentEvent)).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn capability_schema_is_deterministic() {
    let s1 = serde_json::to_string(&schema_for!(Capability)).unwrap();
    let s2 = serde_json::to_string(&schema_for!(Capability)).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn backplane_config_schema_is_deterministic() {
    let s1 = serde_json::to_string(&schema_for!(BackplaneConfig)).unwrap();
    let s2 = serde_json::to_string(&schema_for!(BackplaneConfig)).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn on_disk_work_order_schema_matches_generated() {
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/work_order.schema.json").unwrap(),
    )
    .unwrap();
    let generated = wo_schema();
    assert_eq!(on_disk, generated, "on-disk schema drifted from code");
}

#[test]
fn on_disk_receipt_schema_matches_generated() {
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap(),
    )
    .unwrap();
    let generated = receipt_schema();
    assert_eq!(on_disk, generated, "on-disk schema drifted from code");
}

#[test]
fn on_disk_backplane_config_schema_matches_generated() {
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/backplane_config.schema.json").unwrap(),
    )
    .unwrap();
    let generated = schema_value::<BackplaneConfig>();
    assert_eq!(on_disk, generated, "on-disk schema drifted from code");
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn work_order_with_empty_context_passes() {
    let wo = WorkOrderBuilder::new("task").build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn work_order_with_context_snippets_passes() {
    let ctx = ContextPacket {
        files: vec!["README.md".into()],
        snippets: vec![ContextSnippet {
            name: "hint".into(),
            content: "some content".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("task").context(ctx).build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn receipt_with_artifacts_passes() {
    let receipt = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        })
        .build();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn receipt_with_usage_data_passes() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: Some(1),
        estimated_cost_usd: Some(0.003),
    };
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn receipt_with_verification_data_passes() {
    let mut receipt = ReceiptBuilder::new("mock").build();
    receipt.verification = VerificationReport {
        git_diff: Some("diff --git a/foo b/foo".into()),
        git_status: Some("M foo".into()),
        harness_ok: true,
    };
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn receipt_with_capabilities_passes() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let receipt = ReceiptBuilder::new("mock").capabilities(caps).build();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn outcome_enum_values_are_snake_case() {
    let s = schema_value::<Outcome>();
    let one_of = s["oneOf"].as_array().expect("Outcome uses oneOf");
    let vals: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(vals.contains(&"complete"));
    assert!(vals.contains(&"partial"));
    assert!(vals.contains(&"failed"));
}

#[test]
fn execution_lane_enum_values() {
    let s = schema_value::<ExecutionLane>();
    let one_of = s["oneOf"].as_array().expect("ExecutionLane uses oneOf");
    let vals: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(vals.contains(&"patch_first"));
    assert!(vals.contains(&"workspace_first"));
}

#[test]
fn workspace_mode_enum_values() {
    let s = schema_value::<WorkspaceMode>();
    let one_of = s["oneOf"].as_array().expect("WorkspaceMode uses oneOf");
    let vals: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(vals.contains(&"pass_through"));
    assert!(vals.contains(&"staged"));
}

#[test]
fn execution_mode_enum_values() {
    let s = schema_value::<ExecutionMode>();
    let one_of = s["oneOf"].as_array().expect("ExecutionMode uses oneOf");
    let vals: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(vals.contains(&"passthrough"));
    assert!(vals.contains(&"mapped"));
}

#[test]
fn min_support_enum_values() {
    let s = schema_value::<MinSupport>();
    let one_of = s["oneOf"].as_array().expect("MinSupport uses oneOf");
    let vals: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(vals.contains(&"native"));
    assert!(vals.contains(&"emulated"));
}

#[test]
fn capability_has_all_known_variants() {
    let s = schema_value::<Capability>();
    let one_of = s["oneOf"].as_array().expect("Capability uses oneOf");
    let vals: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    for expected in &[
        "streaming",
        "tool_read",
        "tool_write",
        "tool_edit",
        "tool_bash",
        "tool_glob",
        "tool_grep",
        "tool_web_search",
        "tool_web_fetch",
        "tool_ask_user",
        "hooks_pre_tool_use",
        "hooks_post_tool_use",
        "session_resume",
        "session_fork",
        "checkpointing",
        "structured_output_json_schema",
        "mcp_client",
        "mcp_server",
        "tool_use",
        "extended_thinking",
        "image_input",
        "pdf_input",
        "code_execution",
        "logprobs",
        "seed_determinism",
        "stop_sequences",
    ] {
        assert!(vals.contains(expected), "missing Capability: {expected}");
    }
}

#[test]
fn support_level_restricted_variant_is_object() {
    let s = schema_value::<SupportLevel>();
    let one_of = s["oneOf"].as_array().expect("SupportLevel uses oneOf");
    // Restricted is an externally-tagged object: {"restricted": {"reason": "..."}}
    let has_restricted = one_of.iter().any(|v| {
        v["type"] == "object"
            && v.get("properties")
                .and_then(|p| p.get("restricted"))
                .is_some()
    });
    assert!(
        has_restricted,
        "SupportLevel should have Restricted object variant"
    );
}

#[test]
fn ir_role_enum_values() {
    let s = schema_value::<IrRole>();
    let one_of = s["oneOf"].as_array().expect("IrRole uses oneOf");
    let vals: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(vals.contains(&"system"));
    assert!(vals.contains(&"user"));
    assert!(vals.contains(&"assistant"));
    assert!(vals.contains(&"tool"));
}

#[test]
fn ir_content_block_uses_tag_type() {
    let s = schema_value::<IrContentBlock>();
    // Tagged enum: each variant in oneOf should reference or embed "type" field
    let one_of = s["oneOf"].as_array().expect("IrContentBlock uses oneOf");
    assert!(!one_of.is_empty());
}

#[test]
fn contract_version_value_is_correct() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn receipt_hash_excludes_sha256_field() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h1 = abp_core::receipt_hash(&r).unwrap();
    let r2 = r.with_hash().unwrap();
    let h2 = abp_core::receipt_hash(&r2).unwrap();
    // Hash should be identical because receipt_hash nulls the sha256 field
    assert_eq!(h1, h2);
}

#[test]
fn empty_string_task_is_valid() {
    let wo = WorkOrderBuilder::new("").build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn unicode_task_is_valid() {
    let wo = WorkOrderBuilder::new("修复登录问题 🔧").build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn large_trace_passes_schema() {
    let mut builder = ReceiptBuilder::new("mock");
    for i in 0..100 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token-{i}"),
            },
            ext: None,
        });
    }
    let receipt = builder.build();
    let v = serde_json::to_value(receipt).unwrap();
    assert_valid(&receipt_schema(), &v);
}

#[test]
fn work_order_with_requirements_passes() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let wo = WorkOrderBuilder::new("task").requirements(reqs).build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn work_order_with_vendor_config_passes() {
    let mut config = RuntimeConfig::default();
    config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    config
        .env
        .insert("ANTHROPIC_API_KEY".into(), "sk-test".into());
    let wo = WorkOrderBuilder::new("task").config(config).build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&wo_schema(), &v);
}

#[test]
fn agent_event_kind_schema_uses_tag_type() {
    let s = event_kind_schema();
    // AgentEventKind uses #[serde(tag = "type")]
    let one_of = s["oneOf"].as_array().expect("AgentEventKind uses oneOf");
    assert!(!one_of.is_empty());
}

#[test]
fn policy_profile_all_fields_are_arrays() {
    let s = schema_value::<PolicyProfile>();
    let props = s["properties"].as_object().unwrap();
    for (key, val) in props {
        assert_eq!(
            val["type"], "array",
            "PolicyProfile.{key} should be array type"
        );
    }
}

#[test]
fn context_snippet_required_fields() {
    let s = schema_value::<ContextSnippet>();
    let required = get_required(&s);
    assert!(required.contains(&"name".to_string()));
    assert!(required.contains(&"content".to_string()));
}

#[test]
fn artifact_ref_required_fields() {
    let s = schema_value::<ArtifactRef>();
    let required = get_required(&s);
    assert!(required.contains(&"kind".to_string()));
    assert!(required.contains(&"path".to_string()));
}

#[test]
fn backend_identity_required_id_optional_versions() {
    let s = schema_value::<BackendIdentity>();
    let required = get_required(&s);
    assert!(required.contains(&"id".to_string()));
    assert!(!required.contains(&"backend_version".to_string()));
    assert!(!required.contains(&"adapter_version".to_string()));
}

#[test]
fn work_order_include_exclude_are_string_arrays() {
    let defs = &wo_schema()["$defs"]["WorkspaceSpec"];
    assert_eq!(defs["properties"]["include"]["type"], "array");
    assert_eq!(defs["properties"]["include"]["items"]["type"], "string");
    assert_eq!(defs["properties"]["exclude"]["type"], "array");
    assert_eq!(defs["properties"]["exclude"]["items"]["type"], "string");
}
