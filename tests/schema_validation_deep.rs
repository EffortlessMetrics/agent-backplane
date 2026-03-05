#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep schema validation tests ensuring JSON schemas stay in sync with Rust types.
//!
//! Categories:
//! 1.  WorkOrder schema — fields, required/optional, nested types
//! 2.  Receipt schema — fields, receipt_sha256 nullable, timestamps
//! 3.  AgentEvent schema — all event kind variants
//! 4.  Capability schema — CapabilityManifest, Capability, SupportLevel
//! 5.  Protocol schema — Envelope variants (hello/run/event/final/fatal)
//! 6.  ErrorCode schema — all error codes as enum values
//! 7.  Policy schema — PolicyProfile allow/deny lists
//! 8.  Schema generation consistency — compare generated vs on-disk
//! 9.  Schema validation against sample data — valid accepts, invalid rejects
//! 10. Schema evolution — backward compatibility
//! 11. Serde ↔ Schema alignment — Rust serde output validates against schema
//! 12. Enum representation — tagged enums correctly represented

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec, CONTRACT_VERSION,
};
use chrono::Utc;
use schemars::schema_for;
use serde_json::{json, Value};

// ── helpers ──────────────────────────────────────────────────────────────

fn schema_of<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap()
}

fn work_order_schema() -> Value {
    schema_of::<WorkOrder>()
}

fn receipt_schema() -> Value {
    schema_of::<Receipt>()
}

fn assert_valid(schema: &Value, instance: &Value) {
    let validator =
        jsonschema::validator_for(schema).expect("schema should compile into a validator");
    if let Err(err) = validator.validate(instance) {
        let msgs: Vec<String> = std::iter::once(format!("  - {err}"))
            .chain(
                validator
                    .iter_errors(instance)
                    .skip(1)
                    .map(|e| format!("  - {e}")),
            )
            .collect();
        panic!(
            "Instance should validate but got errors:\n{}",
            msgs.join("\n")
        );
    }
}

fn assert_invalid(schema: &Value, instance: &Value) {
    let validator =
        jsonschema::validator_for(schema).expect("schema should compile into a validator");
    assert!(
        !validator.is_valid(instance),
        "Instance should NOT validate, but it did"
    );
}

fn get_required(schema: &Value) -> Vec<String> {
    schema["required"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn get_properties(schema: &Value) -> Vec<String> {
    schema["properties"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn get_defs(schema: &Value) -> Vec<String> {
    schema["$defs"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// Extract a `$defs` sub-schema as a standalone schema with full `$defs` context
/// so that `$ref` paths like `#/$defs/ErrorCode` resolve correctly.
fn sub_schema(root: &Value, def_name: &str) -> Value {
    let mut sub = root["$defs"][def_name].clone();
    if let Some(defs) = root.get("$defs") {
        sub.as_object_mut()
            .unwrap()
            .insert("$defs".to_string(), defs.clone());
    }
    sub
}

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

// ═══════════════════════════════════════════════════════════════════════════
// 1. WorkOrder schema — fields, required/optional, nested types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn wo_schema_is_draft_2020_12() {
    let s = work_order_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn wo_schema_title() {
    assert_eq!(work_order_schema()["title"], "WorkOrder");
}

#[test]
fn wo_schema_type_is_object() {
    assert_eq!(work_order_schema()["type"], "object");
}

#[test]
fn wo_required_fields() {
    let req = get_required(&work_order_schema());
    for field in &[
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ] {
        assert!(
            req.contains(&field.to_string()),
            "missing required: {field}"
        );
    }
}

#[test]
fn wo_has_all_properties() {
    let props = get_properties(&work_order_schema());
    for field in &[
        "id",
        "task",
        "lane",
        "workspace",
        "context",
        "policy",
        "requirements",
        "config",
    ] {
        assert!(
            props.contains(&field.to_string()),
            "missing property: {field}"
        );
    }
}

#[test]
fn wo_id_is_uuid_format() {
    let s = work_order_schema();
    assert_eq!(s["properties"]["id"]["format"], "uuid");
}

#[test]
fn wo_task_is_string() {
    let s = work_order_schema();
    assert_eq!(s["properties"]["task"]["type"], "string");
}

#[test]
fn wo_defs_include_nested_types() {
    let defs = get_defs(&work_order_schema());
    for name in &[
        "ExecutionLane",
        "WorkspaceSpec",
        "ContextPacket",
        "PolicyProfile",
        "RuntimeConfig",
        "CapabilityRequirements",
    ] {
        assert!(defs.contains(&name.to_string()), "missing $def: {name}");
    }
}

#[test]
fn wo_execution_lane_has_two_variants() {
    let s = work_order_schema();
    let lane = &s["$defs"]["ExecutionLane"];
    let variants = lane["oneOf"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"patch_first"));
    assert!(consts.contains(&"workspace_first"));
}

#[test]
fn wo_workspace_mode_variants() {
    let s = work_order_schema();
    let mode = &s["$defs"]["WorkspaceMode"];
    let variants = mode["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"pass_through"));
    assert!(consts.contains(&"staged"));
}

#[test]
fn wo_workspace_spec_required() {
    let s = work_order_schema();
    let ws = &s["$defs"]["WorkspaceSpec"];
    let req = get_required(ws);
    for f in &["root", "mode", "include", "exclude"] {
        assert!(req.contains(&f.to_string()), "missing workspace req: {f}");
    }
}

#[test]
fn wo_context_packet_required() {
    let s = work_order_schema();
    let cp = &s["$defs"]["ContextPacket"];
    let req = get_required(cp);
    assert!(req.contains(&"files".to_string()));
    assert!(req.contains(&"snippets".to_string()));
}

#[test]
fn wo_runtime_config_optional_model() {
    let s = work_order_schema();
    let rc = &s["$defs"]["RuntimeConfig"];
    let model_type = &rc["properties"]["model"]["type"];
    let types: Vec<&str> = model_type
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(types.contains(&"string"));
    assert!(types.contains(&"null"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Receipt schema — fields, receipt_sha256 nullable, timestamps
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn receipt_schema_is_draft_2020_12() {
    let s = receipt_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn receipt_schema_title() {
    assert_eq!(receipt_schema()["title"], "Receipt");
}

#[test]
fn receipt_required_fields() {
    let req = get_required(&receipt_schema());
    for field in &[
        "meta",
        "backend",
        "capabilities",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
    ] {
        assert!(
            req.contains(&field.to_string()),
            "missing required: {field}"
        );
    }
}

#[test]
fn receipt_sha256_is_nullable() {
    let s = receipt_schema();
    let sha_type = &s["properties"]["receipt_sha256"]["type"];
    let types: Vec<&str> = sha_type
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(types.contains(&"string"));
    assert!(types.contains(&"null"));
}

#[test]
fn receipt_sha256_not_required() {
    let req = get_required(&receipt_schema());
    assert!(
        !req.contains(&"receipt_sha256".to_string()),
        "receipt_sha256 should NOT be required"
    );
}

#[test]
fn receipt_mode_not_required() {
    let req = get_required(&receipt_schema());
    assert!(
        !req.contains(&"mode".to_string()),
        "mode has a default so should not be required"
    );
}

#[test]
fn receipt_run_metadata_timestamps() {
    let s = receipt_schema();
    let meta = &s["$defs"]["RunMetadata"];
    assert_eq!(meta["properties"]["started_at"]["format"], "date-time");
    assert_eq!(meta["properties"]["finished_at"]["format"], "date-time");
}

#[test]
fn receipt_run_metadata_required() {
    let s = receipt_schema();
    let meta = &s["$defs"]["RunMetadata"];
    let req = get_required(meta);
    for f in &[
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(req.contains(&f.to_string()), "missing meta required: {f}");
    }
}

#[test]
fn receipt_run_id_is_uuid() {
    let s = receipt_schema();
    assert_eq!(
        s["$defs"]["RunMetadata"]["properties"]["run_id"]["format"],
        "uuid"
    );
}

#[test]
fn receipt_outcome_enum() {
    let s = receipt_schema();
    let outcome = &s["$defs"]["Outcome"];
    let variants = outcome["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"complete"));
    assert!(consts.contains(&"partial"));
    assert!(consts.contains(&"failed"));
    assert_eq!(consts.len(), 3);
}

#[test]
fn receipt_usage_normalized_all_optional() {
    let s = receipt_schema();
    let usage = &s["$defs"]["UsageNormalized"];
    let req = get_required(usage);
    assert!(
        req.is_empty(),
        "UsageNormalized should have no required fields"
    );
}

#[test]
fn receipt_backend_identity_required_id() {
    let s = receipt_schema();
    let bi = &s["$defs"]["BackendIdentity"];
    let req = get_required(bi);
    assert!(req.contains(&"id".to_string()));
    assert!(!req.contains(&"backend_version".to_string()));
    assert!(!req.contains(&"adapter_version".to_string()));
}

#[test]
fn receipt_verification_report_required() {
    let s = receipt_schema();
    let vr = &s["$defs"]["VerificationReport"];
    let req = get_required(vr);
    assert!(req.contains(&"harness_ok".to_string()));
    assert!(!req.contains(&"git_diff".to_string()));
    assert!(!req.contains(&"git_status".to_string()));
}

#[test]
fn receipt_artifact_ref_required() {
    let s = receipt_schema();
    let ar = &s["$defs"]["ArtifactRef"];
    let req = get_required(ar);
    assert!(req.contains(&"kind".to_string()));
    assert!(req.contains(&"path".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. AgentEvent schema — all event kind variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_event_has_ts_required() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let req = get_required(ae);
    assert!(req.contains(&"ts".to_string()));
}

#[test]
fn agent_event_ts_is_datetime() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    assert_eq!(ae["properties"]["ts"]["format"], "date-time");
}

#[test]
fn agent_event_has_one_of_for_kinds() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    assert!(
        one_of.len() >= 10,
        "expected ≥10 event kind variants, got {}",
        one_of.len()
    );
}

#[test]
fn agent_event_kind_run_started() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    let variant = one_of
        .iter()
        .find(|v| v["properties"]["type"]["const"] == "run_started")
        .expect("run_started variant missing");
    let req = get_required(variant);
    assert!(req.contains(&"type".to_string()));
    assert!(req.contains(&"message".to_string()));
}

#[test]
fn agent_event_kind_run_completed() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    let variant = one_of
        .iter()
        .find(|v| v["properties"]["type"]["const"] == "run_completed")
        .expect("run_completed variant missing");
    let req = get_required(variant);
    assert!(req.contains(&"type".to_string()));
    assert!(req.contains(&"message".to_string()));
}

#[test]
fn agent_event_kind_assistant_delta() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    let variant = one_of
        .iter()
        .find(|v| v["properties"]["type"]["const"] == "assistant_delta")
        .expect("assistant_delta variant missing");
    let req = get_required(variant);
    assert!(req.contains(&"text".to_string()));
}

#[test]
fn agent_event_kind_assistant_message() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    assert!(
        one_of
            .iter()
            .any(|v| v["properties"]["type"]["const"] == "assistant_message"),
        "assistant_message variant missing"
    );
}

#[test]
fn agent_event_kind_tool_call() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    let variant = one_of
        .iter()
        .find(|v| v["properties"]["type"]["const"] == "tool_call")
        .expect("tool_call variant missing");
    let req = get_required(variant);
    assert!(req.contains(&"tool_name".to_string()));
    assert!(req.contains(&"input".to_string()));
}

#[test]
fn agent_event_kind_tool_result() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    let variant = one_of
        .iter()
        .find(|v| v["properties"]["type"]["const"] == "tool_result")
        .expect("tool_result variant missing");
    let req = get_required(variant);
    assert!(req.contains(&"tool_name".to_string()));
    assert!(req.contains(&"output".to_string()));
    assert!(req.contains(&"is_error".to_string()));
}

#[test]
fn agent_event_kind_file_changed() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    let variant = one_of
        .iter()
        .find(|v| v["properties"]["type"]["const"] == "file_changed")
        .expect("file_changed variant missing");
    let req = get_required(variant);
    assert!(req.contains(&"path".to_string()));
    assert!(req.contains(&"summary".to_string()));
}

#[test]
fn agent_event_kind_command_executed() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    let variant = one_of
        .iter()
        .find(|v| v["properties"]["type"]["const"] == "command_executed")
        .expect("command_executed variant missing");
    let req = get_required(variant);
    assert!(req.contains(&"command".to_string()));
}

#[test]
fn agent_event_kind_warning() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    assert!(
        one_of
            .iter()
            .any(|v| v["properties"]["type"]["const"] == "warning"),
        "warning variant missing"
    );
}

#[test]
fn agent_event_kind_error() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let one_of = ae["oneOf"].as_array().unwrap();
    assert!(
        one_of
            .iter()
            .any(|v| v["properties"]["type"]["const"] == "error"),
        "error variant missing"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Capability schema — Capability enum, SupportLevel, MinSupport
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn capability_enum_has_all_variants() {
    let s = work_order_schema();
    let cap = &s["$defs"]["Capability"];
    let one_of = cap["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
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
        assert!(
            consts.contains(expected),
            "Capability missing variant: {expected}"
        );
    }
}

#[test]
fn support_level_has_four_variants() {
    let s = receipt_schema();
    let sl = &s["$defs"]["SupportLevel"];
    let one_of = sl["oneOf"].as_array().unwrap();
    assert!(one_of.len() >= 4, "SupportLevel should have ≥4 variants");
    let simple_consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(simple_consts.contains(&"native"));
    assert!(simple_consts.contains(&"emulated"));
    assert!(simple_consts.contains(&"unsupported"));
}

#[test]
fn support_level_restricted_is_object() {
    let s = receipt_schema();
    let sl = &s["$defs"]["SupportLevel"];
    let one_of = sl["oneOf"].as_array().unwrap();
    let restricted = one_of
        .iter()
        .find(|v| v["properties"]["restricted"].is_object())
        .expect("restricted variant missing from SupportLevel");
    assert_eq!(restricted["type"], "object");
}

#[test]
fn min_support_has_two_variants() {
    let s = work_order_schema();
    let ms = &s["$defs"]["MinSupport"];
    let one_of = ms["oneOf"].as_array().unwrap();
    assert_eq!(one_of.len(), 2);
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"native"));
    assert!(consts.contains(&"emulated"));
}

#[test]
fn capability_requirement_required_fields() {
    let s = work_order_schema();
    let cr = &s["$defs"]["CapabilityRequirement"];
    let req = get_required(cr);
    assert!(req.contains(&"capability".to_string()));
    assert!(req.contains(&"min_support".to_string()));
}

#[test]
fn capability_requirements_has_required_array() {
    let s = work_order_schema();
    let cr = &s["$defs"]["CapabilityRequirements"];
    let req = get_required(cr);
    assert!(req.contains(&"required".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Protocol schema — Envelope variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_hello_roundtrip() {
    let hello = abp_protocol::Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = serde_json::to_string(&hello).unwrap();
    assert!(json.contains(r#""t":"hello""#));
    let decoded: abp_protocol::Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Hello { .. }));
}

#[test]
fn envelope_run_roundtrip() {
    let wo = sample_work_order();
    let run = abp_protocol::Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let json = serde_json::to_string(&run).unwrap();
    assert!(json.contains(r#""t":"run""#));
    let decoded: abp_protocol::Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Run { .. }));
}

#[test]
fn envelope_event_roundtrip() {
    let ev = sample_agent_event(AgentEventKind::AssistantMessage { text: "hi".into() });
    let envelope = abp_protocol::Envelope::Event {
        ref_id: "run-1".into(),
        event: ev,
    };
    let json = serde_json::to_string(&envelope).unwrap();
    assert!(json.contains(r#""t":"event""#));
    let decoded: abp_protocol::Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Event { .. }));
}

#[test]
fn envelope_final_roundtrip() {
    let receipt = sample_receipt();
    let envelope = abp_protocol::Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = serde_json::to_string(&envelope).unwrap();
    assert!(json.contains(r#""t":"final""#));
    let decoded: abp_protocol::Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Final { .. }));
}

#[test]
fn envelope_fatal_roundtrip() {
    let envelope = abp_protocol::Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "boom".into(),
        error_code: None,
    };
    let json = serde_json::to_string(&envelope).unwrap();
    assert!(json.contains(r#""t":"fatal""#));
    let decoded: abp_protocol::Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, abp_protocol::Envelope::Fatal { .. }));
}

#[test]
fn envelope_discriminator_is_t() {
    let hello = abp_protocol::Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let v: Value = serde_json::to_value(&hello).unwrap();
    assert!(
        v.get("t").is_some(),
        "envelope must use 't' as discriminator"
    );
    assert!(v.get("type").is_none(), "envelope must NOT use 'type'");
}

#[test]
fn envelope_fatal_with_error_code() {
    let envelope = abp_protocol::Envelope::Fatal {
        ref_id: Some("run-1".into()),
        error: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    };
    let json = serde_json::to_string(&envelope).unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert!(v["error_code"].is_string());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. ErrorCode schema — all error codes as enum values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_code_schema_has_one_of() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    assert!(
        ec["oneOf"].is_array(),
        "ErrorCode should be represented as oneOf"
    );
}

#[test]
fn error_code_includes_protocol_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"protocol_invalid_envelope"));
    assert!(consts.contains(&"protocol_unexpected_message"));
    assert!(consts.contains(&"protocol_version_mismatch"));
}

#[test]
fn error_code_includes_backend_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"backend_not_found"));
    assert!(consts.contains(&"backend_timeout"));
    assert!(consts.contains(&"backend_crashed"));
}

#[test]
fn error_code_includes_capability_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"capability_unsupported"));
    assert!(consts.contains(&"capability_emulation_failed"));
}

#[test]
fn error_code_includes_policy_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"policy_denied"));
    assert!(consts.contains(&"policy_invalid"));
}

#[test]
fn error_code_includes_workspace_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"workspace_init_failed"));
    assert!(consts.contains(&"workspace_staging_failed"));
}

#[test]
fn error_code_includes_ir_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"ir_lowering_failed"));
    assert!(consts.contains(&"ir_invalid"));
}

#[test]
fn error_code_includes_receipt_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"receipt_hash_mismatch"));
    assert!(consts.contains(&"receipt_chain_broken"));
}

#[test]
fn error_code_includes_dialect_errors() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"dialect_unknown"));
    assert!(consts.contains(&"dialect_mapping_failed"));
}

#[test]
fn error_code_includes_config_and_internal() {
    let s = receipt_schema();
    let ec = &s["$defs"]["ErrorCode"];
    let one_of = ec["oneOf"].as_array().unwrap();
    let consts: Vec<&str> = one_of.iter().filter_map(|v| v["const"].as_str()).collect();
    assert!(consts.contains(&"config_invalid"));
    assert!(consts.contains(&"internal"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Policy schema — PolicyProfile allow/deny lists
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn policy_profile_required_fields() {
    let s = work_order_schema();
    let pp = &s["$defs"]["PolicyProfile"];
    let req = get_required(pp);
    for f in &[
        "allowed_tools",
        "disallowed_tools",
        "deny_read",
        "deny_write",
        "allow_network",
        "deny_network",
        "require_approval_for",
    ] {
        assert!(req.contains(&f.to_string()), "missing policy required: {f}");
    }
}

#[test]
fn policy_profile_all_fields_are_string_arrays() {
    let s = work_order_schema();
    let pp = &s["$defs"]["PolicyProfile"];
    let props = pp["properties"].as_object().unwrap();
    for (name, def) in props {
        assert_eq!(def["type"], "array", "PolicyProfile.{name} should be array");
        assert_eq!(
            def["items"]["type"], "string",
            "PolicyProfile.{name} items should be string"
        );
    }
}

#[test]
fn policy_profile_has_seven_fields() {
    let s = work_order_schema();
    let pp = &s["$defs"]["PolicyProfile"];
    let props = pp["properties"].as_object().unwrap();
    assert_eq!(props.len(), 7);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Schema generation consistency — compare generated vs on-disk
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_schema_matches_disk() {
    let generated = serde_json::to_value(schema_for!(WorkOrder)).unwrap();
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/work_order.schema.json")
            .expect("work_order.schema.json must exist"),
    )
    .unwrap();
    assert_eq!(
        generated, on_disk,
        "WorkOrder schema on disk is out of date — run `cargo run -p xtask -- schema`"
    );
}

#[test]
fn receipt_schema_matches_disk() {
    let generated = serde_json::to_value(schema_for!(Receipt)).unwrap();
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/receipt.schema.json")
            .expect("receipt.schema.json must exist"),
    )
    .unwrap();
    assert_eq!(
        generated, on_disk,
        "Receipt schema on disk is out of date — run `cargo run -p xtask -- schema`"
    );
}

#[test]
fn config_schema_matches_disk() {
    let generated = serde_json::to_value(schema_for!(abp_cli::config::BackplaneConfig)).unwrap();
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/backplane_config.schema.json")
            .expect("backplane_config.schema.json must exist"),
    )
    .unwrap();
    assert_eq!(
        generated, on_disk,
        "BackplaneConfig schema on disk is out of date — run `cargo run -p xtask -- schema`"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Schema validation against sample data — valid/invalid
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn valid_work_order_passes_schema() {
    let s = work_order_schema();
    let wo = serde_json::to_value(sample_work_order()).unwrap();
    assert_valid(&s, &wo);
}

#[test]
fn valid_receipt_passes_schema() {
    let s = receipt_schema();
    let r = serde_json::to_value(sample_receipt()).unwrap();
    assert_valid(&s, &r);
}

#[test]
fn invalid_wo_missing_task_rejected() {
    let s = work_order_schema();
    let mut wo = serde_json::to_value(sample_work_order()).unwrap();
    wo.as_object_mut().unwrap().remove("task");
    assert_invalid(&s, &wo);
}

#[test]
fn invalid_wo_missing_id_rejected() {
    let s = work_order_schema();
    let mut wo = serde_json::to_value(sample_work_order()).unwrap();
    wo.as_object_mut().unwrap().remove("id");
    assert_invalid(&s, &wo);
}

#[test]
fn invalid_wo_wrong_type_task_rejected() {
    let s = work_order_schema();
    let mut wo = serde_json::to_value(sample_work_order()).unwrap();
    wo["task"] = json!(42);
    assert_invalid(&s, &wo);
}

#[test]
fn invalid_wo_wrong_lane_rejected() {
    let s = work_order_schema();
    let mut wo = serde_json::to_value(sample_work_order()).unwrap();
    wo["lane"] = json!("nonexistent_lane");
    assert_invalid(&s, &wo);
}

#[test]
fn invalid_receipt_missing_meta_rejected() {
    let s = receipt_schema();
    let mut r = serde_json::to_value(sample_receipt()).unwrap();
    r.as_object_mut().unwrap().remove("meta");
    assert_invalid(&s, &r);
}

#[test]
fn invalid_receipt_missing_outcome_rejected() {
    let s = receipt_schema();
    let mut r = serde_json::to_value(sample_receipt()).unwrap();
    r.as_object_mut().unwrap().remove("outcome");
    assert_invalid(&s, &r);
}

#[test]
fn invalid_receipt_wrong_outcome_rejected() {
    let s = receipt_schema();
    let mut r = serde_json::to_value(sample_receipt()).unwrap();
    r["outcome"] = json!("unknown_outcome");
    assert_invalid(&s, &r);
}

#[test]
fn receipt_with_null_sha256_passes() {
    let s = receipt_schema();
    let mut r = serde_json::to_value(sample_receipt()).unwrap();
    r["receipt_sha256"] = Value::Null;
    assert_valid(&s, &r);
}

#[test]
fn receipt_with_string_sha256_passes() {
    let s = receipt_schema();
    let mut r = serde_json::to_value(sample_receipt()).unwrap();
    r["receipt_sha256"] = json!("abc123");
    assert_valid(&s, &r);
}

#[test]
fn empty_object_rejected_as_work_order() {
    let s = work_order_schema();
    assert_invalid(&s, &json!({}));
}

#[test]
fn empty_object_rejected_as_receipt() {
    let s = receipt_schema();
    assert_invalid(&s, &json!({}));
}

#[test]
fn null_rejected_as_work_order() {
    let s = work_order_schema();
    assert_invalid(&s, &Value::Null);
}

#[test]
fn array_rejected_as_work_order() {
    let s = work_order_schema();
    assert_invalid(&s, &json!([]));
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Schema evolution — backward compatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn execution_mode_default_is_mapped() {
    let s = receipt_schema();
    let em = &s["properties"]["mode"];
    assert_eq!(em["default"], "mapped");
}

#[test]
fn contract_version_stays_v01() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn work_order_schema_stable_title() {
    assert_eq!(work_order_schema()["title"], "WorkOrder");
}

#[test]
fn receipt_schema_stable_title() {
    assert_eq!(receipt_schema()["title"], "Receipt");
}

#[test]
fn receipt_schema_required_count_stable() {
    let req = get_required(&receipt_schema());
    assert!(
        req.len() >= 9,
        "Receipt should have ≥9 required fields, got {}",
        req.len()
    );
}

#[test]
fn work_order_schema_required_count_stable() {
    let req = get_required(&work_order_schema());
    assert_eq!(req.len(), 8, "WorkOrder should have 8 required fields");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Serde ↔ Schema alignment — Rust serde output validates
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_work_order_validates() {
    let s = work_order_schema();
    let wo = WorkOrderBuilder::new("serde test")
        .lane(ExecutionLane::PatchFirst)
        .build();
    let v = serde_json::to_value(&wo).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_receipt_with_hash_validates() {
    let s = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_receipt_partial_validates() {
    let s = receipt_schema();
    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .build();
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_receipt_failed_validates() {
    let s = receipt_schema();
    let r = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Failed)
        .build();
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_agent_event_run_started_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::RunStarted {
        message: "starting".into(),
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_agent_event_tool_call_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::ToolCall {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/main.rs"}),
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_agent_event_tool_result_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::ToolResult {
        tool_name: "read_file".into(),
        tool_use_id: Some("tu-1".into()),
        output: json!("file contents"),
        is_error: false,
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_agent_event_file_changed_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "added function".into(),
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_agent_event_command_executed_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("all tests passed".into()),
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_agent_event_warning_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::Warning {
        message: "heads up".into(),
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_agent_event_error_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::Error {
        message: "something broke".into(),
        error_code: None,
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_agent_event_error_with_code_validates() {
    let s = receipt_schema();
    let ae_schema = sub_schema(&s, "AgentEvent");
    let ev = sample_agent_event(AgentEventKind::Error {
        message: "timeout".into(),
        error_code: Some(abp_error::ErrorCode::BackendTimeout),
    });
    let v = serde_json::to_value(&ev).unwrap();
    assert_valid(&ae_schema, &v);
}

#[test]
fn serde_work_order_with_policy_validates() {
    let s = work_order_schema();
    let mut wo = sample_work_order();
    wo.policy = PolicyProfile {
        allowed_tools: vec!["read_file".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.secret".into()],
        deny_write: vec!["*.lock".into()],
        allow_network: vec!["api.example.com".into()],
        deny_network: vec!["evil.com".into()],
        require_approval_for: vec!["bash".into()],
    };
    let v = serde_json::to_value(&wo).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_work_order_with_requirements_validates() {
    let s = work_order_schema();
    let mut wo = sample_work_order();
    wo.requirements = CapabilityRequirements {
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
    let v = serde_json::to_value(&wo).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_work_order_with_config_validates() {
    let s = work_order_schema();
    let mut wo = sample_work_order();
    wo.config = RuntimeConfig {
        model: Some("claude-3.5-sonnet".into()),
        vendor: BTreeMap::from([("key".into(), json!("val"))]),
        env: BTreeMap::from([("API_KEY".into(), "xxx".into())]),
        max_budget_usd: Some(10.0),
        max_turns: Some(50),
    };
    let v = serde_json::to_value(&wo).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_receipt_with_capabilities_validates() {
    let s = receipt_schema();
    let mut r = sample_receipt();
    r.capabilities = BTreeMap::from([
        (Capability::Streaming, SupportLevel::Native),
        (Capability::ToolRead, SupportLevel::Emulated),
        (
            Capability::ToolBash,
            SupportLevel::Restricted {
                reason: "policy".into(),
            },
        ),
        (Capability::McpServer, SupportLevel::Unsupported),
    ]);
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_receipt_with_artifacts_validates() {
    let s = receipt_schema();
    let mut r = sample_receipt();
    r.artifacts = vec![
        ArtifactRef {
            kind: "patch".into(),
            path: "changes.patch".into(),
        },
        ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        },
    ];
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_receipt_with_usage_validates() {
    let s = receipt_schema();
    let mut r = sample_receipt();
    r.usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: Some(1),
        estimated_cost_usd: Some(0.005),
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn serde_receipt_with_verification_validates() {
    let s = receipt_schema();
    let mut r = sample_receipt();
    r.verification = VerificationReport {
        git_diff: Some("diff --git ...".into()),
        git_status: Some("M src/main.rs".into()),
        harness_ok: true,
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Enum representation — tagged enums correctly represented
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn agent_event_kind_uses_type_tag() {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    let v: Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "assistant_delta");
}

#[test]
fn agent_event_kind_tag_is_snake_case() {
    let cases: Vec<(AgentEventKind, &str)> = vec![
        (
            AgentEventKind::RunStarted {
                message: "m".into(),
            },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted {
                message: "m".into(),
            },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta { text: "t".into() },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage { text: "t".into() },
            "assistant_message",
        ),
        (
            AgentEventKind::FileChanged {
                path: "p".into(),
                summary: "s".into(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: "c".into(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (
            AgentEventKind::Warning {
                message: "w".into(),
            },
            "warning",
        ),
        (
            AgentEventKind::Error {
                message: "e".into(),
                error_code: None,
            },
            "error",
        ),
    ];
    for (kind, expected_tag) in cases {
        let ev = AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        };
        let v: Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(
            v["type"].as_str().unwrap(),
            expected_tag,
            "wrong tag for variant"
        );
    }
}

#[test]
fn execution_lane_is_snake_case_string() {
    let v: Value = serde_json::to_value(&ExecutionLane::PatchFirst).unwrap();
    assert_eq!(v, "patch_first");
    let v: Value = serde_json::to_value(&ExecutionLane::WorkspaceFirst).unwrap();
    assert_eq!(v, "workspace_first");
}

#[test]
fn execution_mode_is_snake_case_string() {
    let v: Value = serde_json::to_value(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(v, "passthrough");
    let v: Value = serde_json::to_value(&ExecutionMode::Mapped).unwrap();
    assert_eq!(v, "mapped");
}

#[test]
fn outcome_is_snake_case_string() {
    let v: Value = serde_json::to_value(&Outcome::Complete).unwrap();
    assert_eq!(v, "complete");
    let v: Value = serde_json::to_value(&Outcome::Partial).unwrap();
    assert_eq!(v, "partial");
    let v: Value = serde_json::to_value(&Outcome::Failed).unwrap();
    assert_eq!(v, "failed");
}

#[test]
fn workspace_mode_is_snake_case_string() {
    let v: Value = serde_json::to_value(&WorkspaceMode::PassThrough).unwrap();
    assert_eq!(v, "pass_through");
    let v: Value = serde_json::to_value(&WorkspaceMode::Staged).unwrap();
    assert_eq!(v, "staged");
}

#[test]
fn capability_is_snake_case_string() {
    let v: Value = serde_json::to_value(&Capability::Streaming).unwrap();
    assert_eq!(v, "streaming");
    let v: Value = serde_json::to_value(&Capability::ToolRead).unwrap();
    assert_eq!(v, "tool_read");
    let v: Value = serde_json::to_value(&Capability::McpClient).unwrap();
    assert_eq!(v, "mcp_client");
}

#[test]
fn support_level_native_emulated_unsupported() {
    let v: Value = serde_json::to_value(&SupportLevel::Native).unwrap();
    assert_eq!(v, "native");
    let v: Value = serde_json::to_value(&SupportLevel::Emulated).unwrap();
    assert_eq!(v, "emulated");
    let v: Value = serde_json::to_value(&SupportLevel::Unsupported).unwrap();
    assert_eq!(v, "unsupported");
}

#[test]
fn support_level_restricted_is_tagged_object() {
    let v: Value = serde_json::to_value(&SupportLevel::Restricted {
        reason: "policy says no".into(),
    })
    .unwrap();
    assert!(v["restricted"].is_object());
    assert_eq!(v["restricted"]["reason"], "policy says no");
}

#[test]
fn min_support_is_snake_case_string() {
    let v: Value = serde_json::to_value(&MinSupport::Native).unwrap();
    assert_eq!(v, "native");
    let v: Value = serde_json::to_value(&MinSupport::Emulated).unwrap();
    assert_eq!(v, "emulated");
}

#[test]
fn envelope_uses_t_not_type() {
    let run = abp_protocol::Envelope::Run {
        id: "r".into(),
        work_order: sample_work_order(),
    };
    let v: Value = serde_json::to_value(&run).unwrap();
    assert!(v.get("t").is_some());
    assert!(v.get("type").is_none());
}

#[test]
fn backplane_config_schema_is_valid() {
    let s = schema_of::<abp_cli::config::BackplaneConfig>();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
    assert_eq!(s["title"], "BackplaneConfig");
}

#[test]
fn config_schema_has_backends_property() {
    let s = schema_of::<abp_cli::config::BackplaneConfig>();
    assert!(s["properties"]["backends"].is_object());
}

#[test]
fn config_schema_has_default_backend_nullable() {
    let s = schema_of::<abp_cli::config::BackplaneConfig>();
    let db = &s["properties"]["default_backend"];
    let types: Vec<&str> = db["type"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(types.contains(&"string"));
    assert!(types.contains(&"null"));
}

#[test]
fn all_schemas_compile_successfully() {
    for schema in [
        work_order_schema(),
        receipt_schema(),
        schema_of::<abp_cli::config::BackplaneConfig>(),
    ] {
        jsonschema::validator_for(&schema).expect("schema must compile");
    }
}

#[test]
fn work_order_roundtrip_serde_json() {
    let wo = sample_work_order();
    let json = serde_json::to_string(&wo).unwrap();
    let decoded: WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(wo.id, decoded.id);
    assert_eq!(wo.task, decoded.task);
}

#[test]
fn receipt_roundtrip_serde_json() {
    let r = sample_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let decoded: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.meta.run_id, decoded.meta.run_id);
    assert_eq!(r.outcome, decoded.outcome);
}

#[test]
fn receipt_hashed_validates_against_schema() {
    let s = receipt_schema();
    let r = sample_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn agent_event_ext_field_nullable() {
    let s = receipt_schema();
    let ae = &s["$defs"]["AgentEvent"];
    let ext_type = &ae["properties"]["ext"]["type"];
    let types: Vec<&str> = ext_type
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(types.contains(&"object"));
    assert!(types.contains(&"null"));
}

#[test]
fn wo_context_snippet_required_fields() {
    let s = work_order_schema();
    let cs = &s["$defs"]["ContextSnippet"];
    let req = get_required(cs);
    assert!(req.contains(&"name".to_string()));
    assert!(req.contains(&"content".to_string()));
}

#[test]
fn wo_with_context_snippets_validates() {
    let s = work_order_schema();
    let mut wo = sample_work_order();
    wo.context = ContextPacket {
        files: vec!["src/main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "snippet1".into(),
            content: "some code".into(),
        }],
    };
    let v = serde_json::to_value(&wo).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn wo_with_workspace_spec_validates() {
    let s = work_order_schema();
    let mut wo = sample_work_order();
    wo.workspace = WorkspaceSpec {
        root: "/tmp/workspace".into(),
        mode: WorkspaceMode::Staged,
        include: vec!["src/**".into()],
        exclude: vec!["target/**".into()],
    };
    let v = serde_json::to_value(&wo).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn receipt_with_trace_validates() {
    let s = receipt_schema();
    let mut r = sample_receipt();
    r.trace = vec![
        sample_agent_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }),
        sample_agent_event(AgentEventKind::AssistantMessage {
            text: "done".into(),
        }),
        sample_agent_event(AgentEventKind::RunCompleted {
            message: "ok".into(),
        }),
    ];
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn receipt_passthrough_mode_validates() {
    let s = receipt_schema();
    let mut r = sample_receipt();
    r.mode = ExecutionMode::Passthrough;
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn receipt_usage_raw_any_value_validates() {
    let s = receipt_schema();
    let mut r = sample_receipt();
    r.usage_raw = json!({"custom_field": 42, "nested": {"a": true}});
    let v = serde_json::to_value(&r).unwrap();
    assert_valid(&s, &v);
}
