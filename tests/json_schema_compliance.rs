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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests ensuring all ABP types produce valid JSON schemas.
//!
//! Categories covered:
//! 1. WorkOrder schema generation
//! 2. Receipt schema generation
//! 3. AgentEvent schema generation
//! 4. Envelope schema generation (via protocol crate)
//! 5. Capability schema generation
//! 6. PolicyProfile schema generation
//! 7. Config schema generation
//! 8. Schema titles and descriptions present
//! 9. Schema $ref resolution
//! 10. Schema validates known-good instances
//! 11. Schema rejects known-bad instances
//! 12. Schema versioning
//! 13. Schema backward compatibility
//! 14. All public types have schemas
//! 15. Schema determinism (same type → same schema)

use abp_config::{BackendEntry as CfgBackendEntry, BackplaneConfig};
use abp_core::error::MappingErrorKind;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode,
    MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec, CONTRACT_VERSION,
};
use abp_error::ErrorCode;
use abp_policy::Decision;
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

fn get_title(schema: &Value) -> Option<&str> {
    schema.get("title").and_then(Value::as_str)
}

fn get_description(schema: &Value) -> Option<&str> {
    schema.get("description").and_then(Value::as_str)
}

fn get_required(schema: &Value) -> Vec<String> {
    schema["required"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
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

fn valid_event_value() -> Value {
    serde_json::to_value(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    })
    .unwrap()
}

// =========================================================================
// 1. WorkOrder schema generation
// =========================================================================

#[test]
fn work_order_schema_compiles() {
    assert_schema_compiles(&schema_value::<WorkOrder>());
}

#[test]
fn work_order_schema_is_object_type() {
    let s = schema_value::<WorkOrder>();
    assert_eq!(s["type"], "object");
}

#[test]
fn work_order_schema_has_required_fields() {
    let req = get_required(&schema_value::<WorkOrder>());
    for field in [
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
fn work_order_schema_has_all_properties() {
    let props = get_properties(&schema_value::<WorkOrder>());
    for field in [
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
fn work_order_schema_id_is_uuid() {
    let s = schema_value::<WorkOrder>();
    let id_prop = &s["properties"]["id"];
    assert_eq!(id_prop["type"], "string");
    assert_eq!(id_prop["format"], "uuid");
}

#[test]
fn work_order_schema_task_is_string() {
    let s = schema_value::<WorkOrder>();
    assert_eq!(s["properties"]["task"]["type"], "string");
}

#[test]
fn work_order_schema_has_defs() {
    let defs = get_defs(&schema_value::<WorkOrder>());
    assert!(!defs.is_empty(), "WorkOrder schema should have $defs");
}

// =========================================================================
// 2. Receipt schema generation
// =========================================================================

#[test]
fn receipt_schema_compiles() {
    assert_schema_compiles(&schema_value::<Receipt>());
}

#[test]
fn receipt_schema_is_object_type() {
    let s = schema_value::<Receipt>();
    assert_eq!(s["type"], "object");
}

#[test]
fn receipt_schema_has_required_fields() {
    let req = get_required(&schema_value::<Receipt>());
    for field in [
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
fn receipt_schema_outcome_ref_exists() {
    let s = schema_value::<Receipt>();
    let outcome_prop = &s["properties"]["outcome"];
    assert!(
        outcome_prop.get("$ref").is_some() || outcome_prop.get("oneOf").is_some(),
        "outcome should be a $ref or oneOf"
    );
}

#[test]
fn receipt_schema_trace_is_array() {
    let s = schema_value::<Receipt>();
    assert_eq!(s["properties"]["trace"]["type"], "array");
}

#[test]
fn receipt_schema_artifacts_is_array() {
    let s = schema_value::<Receipt>();
    assert_eq!(s["properties"]["artifacts"]["type"], "array");
}

#[test]
fn receipt_schema_has_defs() {
    let defs = get_defs(&schema_value::<Receipt>());
    assert!(!defs.is_empty());
}

// =========================================================================
// 3. AgentEvent schema generation
// =========================================================================

#[test]
fn agent_event_schema_compiles() {
    assert_schema_compiles(&schema_value::<AgentEvent>());
}

#[test]
fn agent_event_kind_schema_compiles() {
    assert_schema_compiles(&schema_value::<AgentEventKind>());
}

#[test]
fn agent_event_kind_has_variants() {
    let s = schema_value::<AgentEventKind>();
    // Tagged enum should have oneOf or anyOf
    let has_one_of = s.get("oneOf").is_some();
    let has_any_of = s.get("anyOf").is_some();
    assert!(
        has_one_of || has_any_of,
        "AgentEventKind should have oneOf or anyOf"
    );
}

#[test]
fn agent_event_schema_has_ts_field() {
    let s = schema_value::<AgentEvent>();
    let props = get_properties(&s);
    assert!(
        props.contains(&"ts".to_string()),
        "AgentEvent should have ts property"
    );
}

// =========================================================================
// 4. Envelope schema generation (protocol — not JsonSchema-derived, test serde)
// =========================================================================

#[test]
fn envelope_hello_round_trips() {
    let hello = json!({
        "t": "hello",
        "contract_version": "abp/v0.1",
        "backend": {"id": "test", "backend_version": null, "adapter_version": null},
        "capabilities": {},
        "mode": "mapped"
    });
    let encoded = serde_json::to_string(&hello).unwrap();
    let decoded: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded["t"], "hello");
}

#[test]
fn envelope_run_round_trips() {
    let wo = valid_wo_value();
    let run = json!({
        "t": "run",
        "id": "run-1",
        "work_order": wo
    });
    let encoded = serde_json::to_string(&run).unwrap();
    let decoded: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded["t"], "run");
}

#[test]
fn envelope_event_round_trips() {
    let evt = valid_event_value();
    let envelope = json!({
        "t": "event",
        "ref_id": "run-1",
        "event": evt
    });
    let encoded = serde_json::to_string(&envelope).unwrap();
    let decoded: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded["t"], "event");
}

#[test]
fn envelope_final_round_trips() {
    let receipt = valid_receipt_value();
    let envelope = json!({
        "t": "final",
        "ref_id": "run-1",
        "receipt": receipt
    });
    let encoded = serde_json::to_string(&envelope).unwrap();
    let decoded: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded["t"], "final");
}

#[test]
fn envelope_fatal_round_trips() {
    let envelope = json!({
        "t": "fatal",
        "ref_id": null,
        "error": "boom"
    });
    let encoded = serde_json::to_string(&envelope).unwrap();
    let decoded: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded["t"], "fatal");
}

// =========================================================================
// 5. Capability schema generation
// =========================================================================

#[test]
fn capability_schema_compiles() {
    assert_schema_compiles(&schema_value::<Capability>());
}

#[test]
fn capability_schema_is_enum() {
    let s = schema_value::<Capability>();
    // schemars renders string enums with oneOf containing const values
    let has_one_of = s.get("oneOf").is_some();
    let has_enum = s.get("enum").is_some();
    assert!(
        has_one_of || has_enum,
        "Capability should be enum-like schema"
    );
}

#[test]
fn support_level_schema_compiles() {
    assert_schema_compiles(&schema_value::<SupportLevel>());
}

#[test]
fn min_support_schema_compiles() {
    assert_schema_compiles(&schema_value::<MinSupport>());
}

#[test]
fn capability_requirement_schema_compiles() {
    assert_schema_compiles(&schema_value::<CapabilityRequirement>());
}

#[test]
fn capability_requirements_schema_compiles() {
    assert_schema_compiles(&schema_value::<CapabilityRequirements>());
}

// =========================================================================
// 6. PolicyProfile schema generation
// =========================================================================

#[test]
fn policy_profile_schema_compiles() {
    assert_schema_compiles(&schema_value::<PolicyProfile>());
}

#[test]
fn policy_profile_is_object() {
    let s = schema_value::<PolicyProfile>();
    assert_eq!(s["type"], "object");
}

#[test]
fn policy_profile_has_tool_fields() {
    let props = get_properties(&schema_value::<PolicyProfile>());
    assert!(props.contains(&"allowed_tools".to_string()));
    assert!(props.contains(&"disallowed_tools".to_string()));
}

#[test]
fn policy_profile_has_path_fields() {
    let props = get_properties(&schema_value::<PolicyProfile>());
    assert!(props.contains(&"deny_read".to_string()));
    assert!(props.contains(&"deny_write".to_string()));
}

#[test]
fn policy_profile_has_network_fields() {
    let props = get_properties(&schema_value::<PolicyProfile>());
    assert!(props.contains(&"allow_network".to_string()));
    assert!(props.contains(&"deny_network".to_string()));
}

#[test]
fn policy_decision_schema_compiles() {
    assert_schema_compiles(&schema_value::<Decision>());
}

// =========================================================================
// 7. Config schema generation
// =========================================================================

#[test]
fn backplane_config_schema_compiles() {
    assert_schema_compiles(&schema_value::<BackplaneConfig>());
}

#[test]
fn backplane_config_is_object() {
    let s = schema_value::<BackplaneConfig>();
    assert_eq!(s["type"], "object");
}

#[test]
fn backplane_config_has_backends_field() {
    let props = get_properties(&schema_value::<BackplaneConfig>());
    assert!(props.contains(&"backends".to_string()));
}

#[test]
fn backend_entry_schema_compiles() {
    assert_schema_compiles(&schema_value::<CfgBackendEntry>());
}

#[test]
fn runtime_config_schema_compiles() {
    assert_schema_compiles(&schema_value::<RuntimeConfig>());
}

#[test]
fn runtime_config_is_object() {
    let s = schema_value::<RuntimeConfig>();
    assert_eq!(s["type"], "object");
}

#[test]
fn runtime_config_has_model_property() {
    let props = get_properties(&schema_value::<RuntimeConfig>());
    assert!(props.contains(&"model".to_string()));
}

// =========================================================================
// 8. Schema titles and descriptions present
// =========================================================================

#[test]
fn work_order_schema_has_title() {
    assert_eq!(get_title(&schema_value::<WorkOrder>()), Some("WorkOrder"));
}

#[test]
fn receipt_schema_has_title() {
    assert_eq!(get_title(&schema_value::<Receipt>()), Some("Receipt"));
}

#[test]
fn work_order_schema_has_description() {
    let s = schema_value::<WorkOrder>();
    let desc = get_description(&s);
    assert!(desc.is_some(), "WorkOrder should have a description");
}

#[test]
fn receipt_schema_has_description() {
    let s = schema_value::<Receipt>();
    let desc = get_description(&s);
    assert!(desc.is_some(), "Receipt should have a description");
}

#[test]
fn policy_profile_schema_has_title() {
    assert_eq!(
        get_title(&schema_value::<PolicyProfile>()),
        Some("PolicyProfile")
    );
}

#[test]
fn capability_schema_has_title() {
    assert_eq!(get_title(&schema_value::<Capability>()), Some("Capability"));
}

#[test]
fn execution_mode_schema_has_title() {
    assert_eq!(
        get_title(&schema_value::<ExecutionMode>()),
        Some("ExecutionMode")
    );
}

#[test]
fn outcome_schema_has_title() {
    assert_eq!(get_title(&schema_value::<Outcome>()), Some("Outcome"));
}

#[test]
fn agent_event_schema_has_title() {
    assert_eq!(get_title(&schema_value::<AgentEvent>()), Some("AgentEvent"));
}

#[test]
fn workspace_spec_schema_has_title() {
    assert_eq!(
        get_title(&schema_value::<WorkspaceSpec>()),
        Some("WorkspaceSpec")
    );
}

#[test]
fn context_packet_schema_has_title() {
    assert_eq!(
        get_title(&schema_value::<ContextPacket>()),
        Some("ContextPacket")
    );
}

#[test]
fn backend_identity_schema_has_title() {
    assert_eq!(
        get_title(&schema_value::<BackendIdentity>()),
        Some("BackendIdentity")
    );
}

// =========================================================================
// 9. Schema $ref resolution
// =========================================================================

#[test]
fn work_order_refs_resolve_to_defs() {
    let s = schema_value::<WorkOrder>();
    let defs = get_defs(&s);
    // Check refs in properties point to existing defs
    if let Some(props) = s["properties"].as_object() {
        for (_key, prop) in props {
            if let Some(r) = prop.get("$ref").and_then(Value::as_str) {
                let def_name = r.strip_prefix("#/$defs/").unwrap_or(r);
                assert!(
                    defs.contains(&def_name.to_string()),
                    "$ref {r} not found in $defs"
                );
            }
        }
    }
}

#[test]
fn receipt_refs_resolve_to_defs() {
    let s = schema_value::<Receipt>();
    let defs = get_defs(&s);
    if let Some(props) = s["properties"].as_object() {
        for (_key, prop) in props {
            if let Some(r) = prop.get("$ref").and_then(Value::as_str) {
                let def_name = r.strip_prefix("#/$defs/").unwrap_or(r);
                assert!(
                    defs.contains(&def_name.to_string()),
                    "$ref {r} not found in $defs"
                );
            }
        }
    }
}

#[test]
fn work_order_execution_lane_ref() {
    let s = schema_value::<WorkOrder>();
    let lane = &s["properties"]["lane"];
    assert!(
        lane.get("$ref").is_some() || lane.get("oneOf").is_some(),
        "lane should reference ExecutionLane"
    );
}

#[test]
fn receipt_run_metadata_ref() {
    let s = schema_value::<Receipt>();
    let meta = &s["properties"]["meta"];
    assert!(
        meta.get("$ref").is_some(),
        "meta should be a $ref to RunMetadata"
    );
}

#[test]
fn receipt_backend_identity_ref() {
    let s = schema_value::<Receipt>();
    let backend = &s["properties"]["backend"];
    assert!(
        backend.get("$ref").is_some(),
        "backend should be a $ref to BackendIdentity"
    );
}

// =========================================================================
// 10. Schema validates known-good instances
// =========================================================================

#[test]
fn valid_work_order_passes_schema() {
    assert_valid(&schema_value::<WorkOrder>(), &valid_wo_value());
}

#[test]
fn valid_receipt_passes_schema() {
    assert_valid(&schema_value::<Receipt>(), &valid_receipt_value());
}

#[test]
fn valid_event_passes_schema() {
    assert_valid(&schema_value::<AgentEvent>(), &valid_event_value());
}

#[test]
fn valid_policy_profile_passes_schema() {
    let pp = serde_json::to_value(PolicyProfile::default()).unwrap();
    assert_valid(&schema_value::<PolicyProfile>(), &pp);
}

#[test]
fn valid_runtime_config_passes_schema() {
    let rc = serde_json::to_value(RuntimeConfig::default()).unwrap();
    assert_valid(&schema_value::<RuntimeConfig>(), &rc);
}

#[test]
fn valid_capability_requirements_passes_schema() {
    let cr = serde_json::to_value(CapabilityRequirements::default()).unwrap();
    assert_valid(&schema_value::<CapabilityRequirements>(), &cr);
}

#[test]
fn valid_context_packet_passes_schema() {
    let cp = serde_json::to_value(ContextPacket::default()).unwrap();
    assert_valid(&schema_value::<ContextPacket>(), &cp);
}

#[test]
fn valid_usage_normalized_passes_schema() {
    let u = serde_json::to_value(UsageNormalized::default()).unwrap();
    assert_valid(&schema_value::<UsageNormalized>(), &u);
}

#[test]
fn valid_verification_report_passes_schema() {
    let vr = serde_json::to_value(VerificationReport::default()).unwrap();
    assert_valid(&schema_value::<VerificationReport>(), &vr);
}

#[test]
fn valid_backplane_config_passes_schema() {
    let cfg = serde_json::to_value(BackplaneConfig::default()).unwrap();
    assert_valid(&schema_value::<BackplaneConfig>(), &cfg);
}

#[test]
fn valid_work_order_with_all_fields_passes() {
    let wo = WorkOrderBuilder::new("full task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/ws")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .policy(PolicyProfile {
            disallowed_tools: vec!["Bash".into()],
            deny_write: vec!["**/.git/**".into()],
            ..PolicyProfile::default()
        })
        .build();
    let v = serde_json::to_value(wo).unwrap();
    assert_valid(&schema_value::<WorkOrder>(), &v);
}

#[test]
fn valid_receipt_with_hash_passes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let v = serde_json::to_value(r).unwrap();
    assert_valid(&schema_value::<Receipt>(), &v);
}

#[test]
fn valid_receipt_partial_outcome_passes() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let v = serde_json::to_value(r).unwrap();
    assert_valid(&schema_value::<Receipt>(), &v);
}

#[test]
fn valid_receipt_failed_outcome_passes() {
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let v = serde_json::to_value(r).unwrap();
    assert_valid(&schema_value::<Receipt>(), &v);
}

#[test]
fn valid_event_run_started_passes() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };
    assert_valid(
        &schema_value::<AgentEvent>(),
        &serde_json::to_value(e).unwrap(),
    );
}

#[test]
fn valid_event_tool_call_passes() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("tu-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "/tmp/foo"}),
        },
        ext: None,
    };
    assert_valid(
        &schema_value::<AgentEvent>(),
        &serde_json::to_value(e).unwrap(),
    );
}

#[test]
fn valid_event_tool_result_passes() {
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "Read".into(),
            tool_use_id: Some("tu-1".into()),
            output: json!("file contents"),
            is_error: false,
        },
        ext: None,
    };
    assert_valid(
        &schema_value::<AgentEvent>(),
        &serde_json::to_value(e).unwrap(),
    );
}

#[test]
fn valid_event_with_ext_passes() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"vendor": "data"}));
    let e = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: Some(ext),
    };
    assert_valid(
        &schema_value::<AgentEvent>(),
        &serde_json::to_value(e).unwrap(),
    );
}

// =========================================================================
// 11. Schema rejects known-bad instances
// =========================================================================

#[test]
fn work_order_rejects_missing_task() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("task");
    assert_invalid(&schema_value::<WorkOrder>(), &v);
}

#[test]
fn work_order_rejects_missing_id() {
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("id");
    assert_invalid(&schema_value::<WorkOrder>(), &v);
}

#[test]
fn work_order_rejects_wrong_type_for_task() {
    let mut v = valid_wo_value();
    v["task"] = json!(42);
    assert_invalid(&schema_value::<WorkOrder>(), &v);
}

#[test]
fn receipt_rejects_missing_outcome() {
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("outcome");
    assert_invalid(&schema_value::<Receipt>(), &v);
}

#[test]
fn receipt_rejects_missing_meta() {
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("meta");
    assert_invalid(&schema_value::<Receipt>(), &v);
}

#[test]
fn receipt_rejects_missing_backend() {
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("backend");
    assert_invalid(&schema_value::<Receipt>(), &v);
}

#[test]
fn receipt_rejects_wrong_type_for_trace() {
    let mut v = valid_receipt_value();
    v["trace"] = json!("not-an-array");
    assert_invalid(&schema_value::<Receipt>(), &v);
}

#[test]
fn work_order_rejects_empty_object() {
    assert_invalid(&schema_value::<WorkOrder>(), &json!({}));
}

#[test]
fn receipt_rejects_empty_object() {
    assert_invalid(&schema_value::<Receipt>(), &json!({}));
}

#[test]
fn work_order_rejects_null() {
    assert_invalid(&schema_value::<WorkOrder>(), &json!(null));
}

#[test]
fn receipt_rejects_null() {
    assert_invalid(&schema_value::<Receipt>(), &json!(null));
}

#[test]
fn work_order_rejects_array() {
    assert_invalid(&schema_value::<WorkOrder>(), &json!([]));
}

#[test]
fn policy_profile_rejects_wrong_tools_type() {
    let mut pp = serde_json::to_value(PolicyProfile::default()).unwrap();
    pp["allowed_tools"] = json!("not-an-array");
    assert_invalid(&schema_value::<PolicyProfile>(), &pp);
}

// =========================================================================
// 12. Schema versioning
// =========================================================================

#[test]
fn work_order_schema_has_schema_field() {
    let s = schema_value::<WorkOrder>();
    assert!(
        s.get("$schema").is_some(),
        "schema should include $schema meta-field"
    );
}

#[test]
fn receipt_schema_has_schema_field() {
    let s = schema_value::<Receipt>();
    assert!(s.get("$schema").is_some());
}

#[test]
fn work_order_schema_uses_draft_2020_12() {
    let s = schema_value::<WorkOrder>();
    let meta = s["$schema"].as_str().unwrap_or("");
    assert!(
        meta.contains("2020-12"),
        "expected Draft 2020-12, got: {meta}"
    );
}

#[test]
fn contract_version_is_present() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn receipt_meta_includes_contract_version() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

// =========================================================================
// 13. Schema backward compatibility (structural stability)
// =========================================================================

#[test]
fn work_order_required_fields_stable() {
    let req = get_required(&schema_value::<WorkOrder>());
    // These fields must always be required for backward compat
    let stable = ["id", "task", "lane", "workspace", "policy", "config"];
    for f in stable {
        assert!(
            req.contains(&f.to_string()),
            "stable required field '{f}' missing"
        );
    }
}

#[test]
fn receipt_required_fields_stable() {
    let req = get_required(&schema_value::<Receipt>());
    let stable = ["meta", "backend", "outcome", "trace"];
    for f in stable {
        assert!(
            req.contains(&f.to_string()),
            "stable required field '{f}' missing"
        );
    }
}

#[test]
fn outcome_variants_stable() {
    let s = schema_value::<Outcome>();
    let json_str = serde_json::to_string(&s).unwrap();
    for variant in ["complete", "partial", "failed"] {
        assert!(
            json_str.contains(variant),
            "Outcome should contain variant '{variant}'"
        );
    }
}

#[test]
fn execution_mode_variants_stable() {
    let s = schema_value::<ExecutionMode>();
    let json_str = serde_json::to_string(&s).unwrap();
    for variant in ["passthrough", "mapped"] {
        assert!(
            json_str.contains(variant),
            "ExecutionMode should contain '{variant}'"
        );
    }
}

#[test]
fn execution_lane_variants_stable() {
    let s = schema_value::<ExecutionLane>();
    let json_str = serde_json::to_string(&s).unwrap();
    for variant in ["patch_first", "workspace_first"] {
        assert!(
            json_str.contains(variant),
            "ExecutionLane should contain '{variant}'"
        );
    }
}

#[test]
fn workspace_mode_variants_stable() {
    let s = schema_value::<WorkspaceMode>();
    let json_str = serde_json::to_string(&s).unwrap();
    for variant in ["pass_through", "staged"] {
        assert!(
            json_str.contains(variant),
            "WorkspaceMode should contain '{variant}'"
        );
    }
}

// =========================================================================
// 14. All public types have schemas
// =========================================================================

#[test]
fn work_order_generates_schema() {
    let _ = schema_for!(WorkOrder);
}

#[test]
fn receipt_generates_schema() {
    let _ = schema_for!(Receipt);
}

#[test]
fn agent_event_generates_schema() {
    let _ = schema_for!(AgentEvent);
}

#[test]
fn agent_event_kind_generates_schema() {
    let _ = schema_for!(AgentEventKind);
}

#[test]
fn capability_generates_schema() {
    let _ = schema_for!(Capability);
}

#[test]
fn support_level_generates_schema() {
    let _ = schema_for!(SupportLevel);
}

#[test]
fn min_support_generates_schema() {
    let _ = schema_for!(MinSupport);
}

#[test]
fn capability_requirement_generates_schema() {
    let _ = schema_for!(CapabilityRequirement);
}

#[test]
fn capability_requirements_generates_schema() {
    let _ = schema_for!(CapabilityRequirements);
}

#[test]
fn execution_lane_generates_schema() {
    let _ = schema_for!(ExecutionLane);
}

#[test]
fn execution_mode_generates_schema() {
    let _ = schema_for!(ExecutionMode);
}

#[test]
fn workspace_spec_generates_schema() {
    let _ = schema_for!(WorkspaceSpec);
}

#[test]
fn workspace_mode_generates_schema() {
    let _ = schema_for!(WorkspaceMode);
}

#[test]
fn context_packet_generates_schema() {
    let _ = schema_for!(ContextPacket);
}

#[test]
fn context_snippet_generates_schema() {
    let _ = schema_for!(ContextSnippet);
}

#[test]
fn runtime_config_generates_schema() {
    let _ = schema_for!(RuntimeConfig);
}

#[test]
fn policy_profile_generates_schema() {
    let _ = schema_for!(PolicyProfile);
}

#[test]
fn backend_identity_generates_schema() {
    let _ = schema_for!(BackendIdentity);
}

#[test]
fn run_metadata_generates_schema() {
    let _ = schema_for!(RunMetadata);
}

#[test]
fn usage_normalized_generates_schema() {
    let _ = schema_for!(UsageNormalized);
}

#[test]
fn outcome_generates_schema() {
    let _ = schema_for!(Outcome);
}

#[test]
fn artifact_ref_generates_schema() {
    let _ = schema_for!(ArtifactRef);
}

#[test]
fn verification_report_generates_schema() {
    let _ = schema_for!(VerificationReport);
}

#[test]
fn backplane_config_generates_schema() {
    let _ = schema_for!(BackplaneConfig);
}

#[test]
fn cfg_backend_entry_generates_schema() {
    let _ = schema_for!(CfgBackendEntry);
}

#[test]
fn error_code_generates_schema() {
    let _ = schema_for!(ErrorCode);
}

#[test]
fn policy_decision_generates_schema() {
    let _ = schema_for!(Decision);
}

#[test]
fn mapping_error_kind_generates_schema() {
    let _ = schema_for!(MappingErrorKind);
}

#[test]
fn ir_role_generates_schema() {
    let _ = schema_for!(IrRole);
}

#[test]
fn ir_content_block_generates_schema() {
    let _ = schema_for!(IrContentBlock);
}

#[test]
fn ir_message_generates_schema() {
    let _ = schema_for!(IrMessage);
}

#[test]
fn ir_tool_definition_generates_schema() {
    let _ = schema_for!(IrToolDefinition);
}

#[test]
fn ir_conversation_generates_schema() {
    let _ = schema_for!(IrConversation);
}

#[test]
fn ir_usage_generates_schema() {
    let _ = schema_for!(IrUsage);
}

// =========================================================================
// 15. Schema determinism (same type → same schema)
// =========================================================================

#[test]
fn work_order_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(WorkOrder)).unwrap();
    let b = serde_json::to_string(&schema_for!(WorkOrder)).unwrap();
    assert_eq!(a, b, "WorkOrder schema should be deterministic");
}

#[test]
fn receipt_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(Receipt)).unwrap();
    let b = serde_json::to_string(&schema_for!(Receipt)).unwrap();
    assert_eq!(a, b, "Receipt schema should be deterministic");
}

#[test]
fn agent_event_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(AgentEvent)).unwrap();
    let b = serde_json::to_string(&schema_for!(AgentEvent)).unwrap();
    assert_eq!(a, b, "AgentEvent schema should be deterministic");
}

#[test]
fn capability_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(Capability)).unwrap();
    let b = serde_json::to_string(&schema_for!(Capability)).unwrap();
    assert_eq!(a, b, "Capability schema should be deterministic");
}

#[test]
fn policy_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(PolicyProfile)).unwrap();
    let b = serde_json::to_string(&schema_for!(PolicyProfile)).unwrap();
    assert_eq!(a, b, "PolicyProfile schema should be deterministic");
}

#[test]
fn config_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(BackplaneConfig)).unwrap();
    let b = serde_json::to_string(&schema_for!(BackplaneConfig)).unwrap();
    assert_eq!(a, b, "BackplaneConfig schema should be deterministic");
}

#[test]
fn execution_mode_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(ExecutionMode)).unwrap();
    let b = serde_json::to_string(&schema_for!(ExecutionMode)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn outcome_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(Outcome)).unwrap();
    let b = serde_json::to_string(&schema_for!(Outcome)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn ir_conversation_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(IrConversation)).unwrap();
    let b = serde_json::to_string(&schema_for!(IrConversation)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn error_code_schema_deterministic() {
    let a = serde_json::to_string(&schema_for!(ErrorCode)).unwrap();
    let b = serde_json::to_string(&schema_for!(ErrorCode)).unwrap();
    assert_eq!(a, b);
}

// =========================================================================
// Additional coverage: IR type schemas compile and validate
// =========================================================================

#[test]
fn ir_role_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrRole>());
}

#[test]
fn ir_content_block_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrContentBlock>());
}

#[test]
fn ir_message_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrMessage>());
}

#[test]
fn ir_conversation_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrConversation>());
}

#[test]
fn ir_tool_definition_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrToolDefinition>());
}

#[test]
fn ir_usage_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrUsage>());
}

#[test]
fn ir_message_validates_good_instance() {
    let msg = IrMessage::text(IrRole::User, "hello world");
    let v = serde_json::to_value(msg).unwrap();
    assert_valid(&schema_value::<IrMessage>(), &v);
}

#[test]
fn ir_conversation_validates_good_instance() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are a helper."))
        .push(IrMessage::text(IrRole::User, "Hello"));
    let v = serde_json::to_value(conv).unwrap();
    assert_valid(&schema_value::<IrConversation>(), &v);
}

#[test]
fn ir_usage_validates_good_instance() {
    let u = IrUsage::from_io(100, 50);
    let v = serde_json::to_value(u).unwrap();
    assert_valid(&schema_value::<IrUsage>(), &v);
}

// =========================================================================
// Additional coverage: Error and mapping types
// =========================================================================

#[test]
fn error_code_schema_compiles() {
    assert_schema_compiles(&schema_value::<ErrorCode>());
}

#[test]
fn mapping_error_kind_schema_compiles() {
    assert_schema_compiles(&schema_value::<MappingErrorKind>());
}

#[test]
fn error_code_schema_has_title() {
    assert_eq!(get_title(&schema_value::<ErrorCode>()), Some("ErrorCode"));
}

#[test]
fn mapping_error_kind_has_title() {
    assert_eq!(
        get_title(&schema_value::<MappingErrorKind>()),
        Some("MappingErrorKind")
    );
}

#[test]
fn error_code_variants_present() {
    let s = schema_value::<ErrorCode>();
    let json_str = serde_json::to_string(&s).unwrap();
    for variant in [
        "backend_not_found",
        "policy_denied",
        "receipt_hash_mismatch",
    ] {
        assert!(
            json_str.contains(variant),
            "ErrorCode should contain '{variant}'"
        );
    }
}

#[test]
fn mapping_error_kind_variants_present() {
    let s = schema_value::<MappingErrorKind>();
    let json_str = serde_json::to_string(&s).unwrap();
    for variant in ["fatal", "degraded", "emulated"] {
        assert!(
            json_str.contains(variant),
            "MappingErrorKind should contain '{variant}'"
        );
    }
}

// =========================================================================
// Additional: on-disk schema files are valid
// =========================================================================

#[test]
fn on_disk_work_order_schema_valid() {
    let content = std::fs::read_to_string("contracts/schemas/work_order.schema.json").unwrap();
    let schema: Value = serde_json::from_str(&content).unwrap();
    assert_schema_compiles(&schema);
}

#[test]
fn on_disk_receipt_schema_valid() {
    let content = std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap();
    let schema: Value = serde_json::from_str(&content).unwrap();
    assert_schema_compiles(&schema);
}

#[test]
fn on_disk_backplane_config_schema_valid() {
    let content =
        std::fs::read_to_string("contracts/schemas/backplane_config.schema.json").unwrap();
    let schema: Value = serde_json::from_str(&content).unwrap();
    assert_schema_compiles(&schema);
}

#[test]
fn on_disk_work_order_validates_instance() {
    let content = std::fs::read_to_string("contracts/schemas/work_order.schema.json").unwrap();
    let schema: Value = serde_json::from_str(&content).unwrap();
    assert_valid(&schema, &valid_wo_value());
}

#[test]
fn on_disk_receipt_validates_instance() {
    let content = std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap();
    let schema: Value = serde_json::from_str(&content).unwrap();
    assert_valid(&schema, &valid_receipt_value());
}
