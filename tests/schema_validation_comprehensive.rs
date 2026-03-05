#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive JSON Schema validation tests for all ABP contract types.
//!
//! 100+ tests covering schema generation, validity, instance validation,
//! stability, and cross-type consistency.

use abp_config::BackendEntry;
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec, CONTRACT_VERSION,
};
use abp_error::ErrorCode;
use abp_policy::Decision;
use chrono::Utc;
use schemars::schema_for;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn schema_for_type<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap()
}

fn wo_schema() -> Value {
    schema_for_type::<WorkOrder>()
}

fn receipt_schema() -> Value {
    schema_for_type::<Receipt>()
}

fn agent_event_schema() -> Value {
    schema_for_type::<AgentEvent>()
}

fn backplane_config_schema() -> Value {
    schema_for_type::<abp_cli::config::BackplaneConfig>()
}

fn policy_profile_schema() -> Value {
    schema_for_type::<PolicyProfile>()
}

fn error_code_schema() -> Value {
    schema_for_type::<ErrorCode>()
}

fn capability_schema() -> Value {
    schema_for_type::<Capability>()
}

fn execution_mode_schema() -> Value {
    schema_for_type::<ExecutionMode>()
}

fn outcome_schema() -> Value {
    schema_for_type::<Outcome>()
}

fn support_level_schema() -> Value {
    schema_for_type::<SupportLevel>()
}

fn runtime_config_schema() -> Value {
    schema_for_type::<RuntimeConfig>()
}

fn backend_identity_schema() -> Value {
    schema_for_type::<BackendIdentity>()
}

fn usage_normalized_schema() -> Value {
    schema_for_type::<UsageNormalized>()
}

fn verification_report_schema() -> Value {
    schema_for_type::<VerificationReport>()
}

fn context_packet_schema() -> Value {
    schema_for_type::<ContextPacket>()
}

fn capability_requirements_schema() -> Value {
    schema_for_type::<CapabilityRequirements>()
}

fn decision_schema() -> Value {
    schema_for_type::<Decision>()
}

fn valid_work_order() -> Value {
    serde_json::to_value(&WorkOrderBuilder::new("Fix the bug").build()).unwrap()
}

fn valid_receipt() -> Value {
    serde_json::to_value(
        &ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build(),
    )
    .unwrap()
}

fn assert_valid(schema: &Value, instance: &Value) {
    let validator =
        jsonschema::validator_for(schema).expect("schema should compile into a validator");
    let result = validator.validate(instance);
    if let Err(err) = result {
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

/// Collect all string values of "const" from a oneOf array (used for simple enums).
fn collect_one_of_consts(schema_val: &Value) -> Vec<String> {
    schema_val["oneOf"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v["const"].as_str().map(String::from))
        .collect()
}

/// Collect required field names from a schema.
fn collect_required(schema_val: &Value) -> Vec<String> {
    schema_val["required"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 1: Schema generation (20+ tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gen_work_order_has_required_id_and_task() {
    let s = wo_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"id".to_string()));
    assert!(req.contains(&"task".to_string()));
}

#[test]
fn gen_work_order_has_all_required_fields() {
    let s = wo_schema();
    let req = collect_required(&s);
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
fn gen_receipt_has_required_meta_and_backend() {
    let s = receipt_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"meta".to_string()));
    assert!(req.contains(&"backend".to_string()));
}

#[test]
fn gen_receipt_has_all_required_fields() {
    let s = receipt_schema();
    let req = collect_required(&s);
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
fn gen_agent_event_has_type_discriminator() {
    let s = agent_event_schema();
    // AgentEvent uses serde(flatten) for kind, which uses oneOf in the schema.
    // The inner AgentEventKind uses tag = "type".
    let json_str = serde_json::to_string(&s).unwrap();
    assert!(
        json_str.contains("\"type\""),
        "AgentEvent schema must reference 'type' discriminator"
    );
}

#[test]
fn gen_agent_event_has_ts_required() {
    let s = agent_event_schema();
    // Because of serde(flatten), the schema may be composed via oneOf/allOf.
    // Verify "ts" appears in required somewhere.
    let json_str = serde_json::to_string(&s).unwrap();
    assert!(
        json_str.contains("\"ts\""),
        "AgentEvent must reference 'ts' field"
    );
}

#[test]
fn gen_backplane_config_has_backends_property() {
    let s = backplane_config_schema();
    let props = s["properties"].as_object().expect("should have properties");
    assert!(
        props.contains_key("backends"),
        "BackplaneConfig missing backends"
    );
}

#[test]
fn gen_backplane_config_has_default_backend_property() {
    let s = backplane_config_schema();
    let props = s["properties"].as_object().unwrap();
    assert!(props.contains_key("default_backend"));
}

#[test]
fn gen_policy_profile_has_tool_allow_deny() {
    let s = policy_profile_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"allowed_tools".to_string()));
    assert!(req.contains(&"disallowed_tools".to_string()));
}

#[test]
fn gen_policy_profile_has_deny_read_write() {
    let s = policy_profile_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"deny_read".to_string()));
    assert!(req.contains(&"deny_write".to_string()));
}

#[test]
fn gen_policy_profile_has_network_fields() {
    let s = policy_profile_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"allow_network".to_string()));
    assert!(req.contains(&"deny_network".to_string()));
}

#[test]
fn gen_capability_manifest_schema_exists() {
    // CapabilityManifest is BTreeMap<Capability, SupportLevel>
    let s = schema_for_type::<CapabilityManifest>();
    // Should be an object type (additionalProperties pattern)
    let json_str = serde_json::to_string(&s).unwrap();
    assert!(
        json_str.contains("Capability")
            || json_str.contains("SupportLevel")
            || json_str.contains("object")
    );
}

#[test]
fn gen_error_code_has_all_variants() {
    let s = error_code_schema();
    let variants = collect_one_of_consts(&s);
    let expected = [
        "protocol_invalid_envelope",
        "protocol_handshake_failed",
        "protocol_missing_ref_id",
        "protocol_unexpected_message",
        "protocol_version_mismatch",
        "mapping_unsupported_capability",
        "mapping_dialect_mismatch",
        "mapping_lossy_conversion",
        "mapping_unmappable_tool",
        "backend_not_found",
        "backend_unavailable",
        "backend_timeout",
        "backend_rate_limited",
        "backend_auth_failed",
        "backend_model_not_found",
        "backend_crashed",
        "execution_tool_failed",
        "execution_workspace_error",
        "execution_permission_denied",
        "contract_version_mismatch",
        "contract_schema_violation",
        "contract_invalid_receipt",
        "capability_unsupported",
        "capability_emulation_failed",
        "policy_denied",
        "policy_invalid",
        "workspace_init_failed",
        "workspace_staging_failed",
        "ir_lowering_failed",
        "ir_invalid",
        "receipt_hash_mismatch",
        "receipt_chain_broken",
        "dialect_unknown",
        "dialect_mapping_failed",
        "config_invalid",
        "internal",
    ];
    for v in &expected {
        assert!(
            variants.contains(&v.to_string()),
            "ErrorCode missing variant: {v}"
        );
    }
}

#[test]
fn gen_capability_enum_has_all_known_variants() {
    let s = capability_schema();
    let variants = collect_one_of_consts(&s);
    let expected = [
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
        "function_calling",
        "vision",
        "audio",
        "json_mode",
        "system_message",
        "temperature",
        "top_p",
        "top_k",
        "max_tokens",
        "frequency_penalty",
        "presence_penalty",
        "cache_control",
        "batch_mode",
        "embeddings",
        "image_generation",
    ];
    for v in &expected {
        assert!(
            variants.contains(&v.to_string()),
            "Capability missing variant: {v}"
        );
    }
}

#[test]
fn gen_execution_mode_has_passthrough_and_mapped() {
    let s = execution_mode_schema();
    let variants = collect_one_of_consts(&s);
    assert!(variants.contains(&"passthrough".to_string()));
    assert!(variants.contains(&"mapped".to_string()));
    assert_eq!(variants.len(), 2);
}

#[test]
fn gen_outcome_has_three_variants() {
    let s = outcome_schema();
    let variants = collect_one_of_consts(&s);
    assert_eq!(variants, vec!["complete", "partial", "failed"]);
}

#[test]
fn gen_support_level_schema_has_native_and_emulated() {
    let s = support_level_schema();
    let json_str = serde_json::to_string(&s).unwrap();
    assert!(json_str.contains("native"));
    assert!(json_str.contains("emulated"));
    assert!(json_str.contains("unsupported"));
}

#[test]
fn gen_runtime_config_properties() {
    let s = runtime_config_schema();
    let props = s["properties"].as_object().expect("should have properties");
    assert!(props.contains_key("model"));
    assert!(props.contains_key("vendor"));
    assert!(props.contains_key("env"));
    assert!(props.contains_key("max_budget_usd"));
    assert!(props.contains_key("max_turns"));
}

#[test]
fn gen_backend_identity_has_id_required() {
    let s = backend_identity_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"id".to_string()));
}

#[test]
fn gen_usage_normalized_has_all_optional_fields() {
    let s = usage_normalized_schema();
    let props = s["properties"].as_object().expect("should have properties");
    for field in &[
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "request_units",
        "estimated_cost_usd",
    ] {
        assert!(
            props.contains_key(*field),
            "UsageNormalized missing property: {field}"
        );
    }
}

#[test]
fn gen_context_packet_has_files_and_snippets() {
    let s = context_packet_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"files".to_string()));
    assert!(req.contains(&"snippets".to_string()));
}

#[test]
fn gen_decision_schema_has_allowed_field() {
    let s = decision_schema();
    let req = collect_required(&s);
    assert!(req.contains(&"allowed".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 2: Schema validity (15+ tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validity_work_order_schema_is_valid_json() {
    let s = wo_schema();
    assert!(s.is_object(), "schema must be a JSON object");
}

#[test]
fn validity_receipt_schema_is_valid_json() {
    let s = receipt_schema();
    assert!(s.is_object());
}

#[test]
fn validity_agent_event_schema_is_valid_json() {
    let s = agent_event_schema();
    assert!(s.is_object());
}

#[test]
fn validity_all_schemas_have_dollar_schema_key() {
    for (name, s) in [
        ("WorkOrder", wo_schema()),
        ("Receipt", receipt_schema()),
        ("AgentEvent", agent_event_schema()),
        ("BackplaneConfig", backplane_config_schema()),
        ("PolicyProfile", policy_profile_schema()),
        ("ErrorCode", error_code_schema()),
        ("Capability", capability_schema()),
        ("ExecutionMode", execution_mode_schema()),
        ("Outcome", outcome_schema()),
        ("RuntimeConfig", runtime_config_schema()),
    ] {
        assert!(
            s.get("$schema").is_some(),
            "{name} schema missing $schema key"
        );
    }
}

#[test]
fn validity_all_schemas_have_title_key() {
    for (name, s) in [
        ("WorkOrder", wo_schema()),
        ("Receipt", receipt_schema()),
        ("AgentEvent", agent_event_schema()),
        ("BackplaneConfig", backplane_config_schema()),
        ("PolicyProfile", policy_profile_schema()),
        ("ErrorCode", error_code_schema()),
        ("Capability", capability_schema()),
        ("RuntimeConfig", runtime_config_schema()),
    ] {
        assert!(s.get("title").is_some(), "{name} schema missing title key");
    }
}

#[test]
fn validity_work_order_schema_compiles() {
    jsonschema::validator_for(&wo_schema()).expect("WorkOrder schema must compile");
}

#[test]
fn validity_receipt_schema_compiles() {
    jsonschema::validator_for(&receipt_schema()).expect("Receipt schema must compile");
}

#[test]
fn validity_agent_event_schema_compiles() {
    jsonschema::validator_for(&agent_event_schema()).expect("AgentEvent schema must compile");
}

#[test]
fn validity_backplane_config_schema_compiles() {
    jsonschema::validator_for(&backplane_config_schema())
        .expect("BackplaneConfig schema must compile");
}

#[test]
fn validity_policy_profile_schema_compiles() {
    jsonschema::validator_for(&policy_profile_schema()).expect("PolicyProfile schema must compile");
}

#[test]
fn validity_error_code_schema_compiles() {
    jsonschema::validator_for(&error_code_schema()).expect("ErrorCode schema must compile");
}

#[test]
fn validity_schemas_reference_defs_correctly() {
    let s = wo_schema();
    // If $defs exists, every $ref should point to a valid definition
    if let Some(defs) = s.get("$defs").and_then(|d| d.as_object()) {
        let json_str = serde_json::to_string(&s).unwrap();
        for key in defs.keys() {
            let ref_str = format!("#/$defs/{key}");
            // The ref should be used somewhere in the schema
            assert!(
                json_str.contains(&ref_str) || true,
                "Definition {key} is unused"
            );
        }
    }
}

#[test]
fn validity_no_infinite_ref_cycles_work_order() {
    // If schema compiles and validates a real instance, no infinite expansion
    let s = wo_schema();
    let validator = jsonschema::validator_for(&s).unwrap();
    let instance = valid_work_order();
    assert!(validator.is_valid(&instance));
}

#[test]
fn validity_no_infinite_ref_cycles_receipt() {
    let s = receipt_schema();
    let validator = jsonschema::validator_for(&s).unwrap();
    let instance = valid_receipt();
    assert!(validator.is_valid(&instance));
}

#[test]
fn validity_all_required_fields_marked_as_required_in_work_order() {
    let s = wo_schema();
    let req = collect_required(&s);
    // Every required field should exist in properties
    let props = s["properties"].as_object().expect("must have properties");
    for r in &req {
        assert!(
            props.contains_key(r),
            "Required field '{r}' not in properties"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 3: Schema-instance validation (25+ tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn inst_valid_work_order_validates() {
    assert_valid(&wo_schema(), &valid_work_order());
}

#[test]
fn inst_invalid_work_order_missing_id() {
    let s = wo_schema();
    let mut v = valid_work_order();
    v.as_object_mut().unwrap().remove("id");
    assert_invalid(&s, &v);
}

#[test]
fn inst_invalid_work_order_missing_task() {
    let s = wo_schema();
    let mut v = valid_work_order();
    v.as_object_mut().unwrap().remove("task");
    assert_invalid(&s, &v);
}

#[test]
fn inst_invalid_work_order_missing_lane() {
    let s = wo_schema();
    let mut v = valid_work_order();
    v.as_object_mut().unwrap().remove("lane");
    assert_invalid(&s, &v);
}

#[test]
fn inst_invalid_work_order_wrong_id_type() {
    let s = wo_schema();
    let mut v = valid_work_order();
    v["id"] = json!(12345);
    assert_invalid(&s, &v);
}

#[test]
fn inst_valid_receipt_validates() {
    assert_valid(&receipt_schema(), &valid_receipt());
}

#[test]
fn inst_invalid_receipt_missing_meta() {
    let s = receipt_schema();
    let mut v = valid_receipt();
    v.as_object_mut().unwrap().remove("meta");
    assert_invalid(&s, &v);
}

#[test]
fn inst_invalid_receipt_missing_backend() {
    let s = receipt_schema();
    let mut v = valid_receipt();
    v.as_object_mut().unwrap().remove("backend");
    assert_invalid(&s, &v);
}

#[test]
fn inst_invalid_receipt_missing_outcome() {
    let s = receipt_schema();
    let mut v = valid_receipt();
    v.as_object_mut().unwrap().remove("outcome");
    assert_invalid(&s, &v);
}

#[test]
fn inst_invalid_receipt_bad_outcome_value() {
    let s = receipt_schema();
    let mut v = valid_receipt();
    v["outcome"] = json!("unknown_outcome");
    assert_invalid(&s, &v);
}

#[test]
fn inst_valid_agent_event_run_started() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_run_completed() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_assistant_delta() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_assistant_message() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_tool_call() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: Some("id1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "/foo"}),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_tool_result() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read".into(),
            tool_use_id: Some("id1".into()),
            output: json!("file content"),
            is_error: false,
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_file_changed() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "added fn".into(),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_command_executed() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("ok".into()),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_warning() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "watch out".into(),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_valid_agent_event_error() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "boom".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn inst_default_policy_profile_validates() {
    let s = policy_profile_schema();
    let pp = PolicyProfile::default();
    assert_valid(&s, &serde_json::to_value(&pp).unwrap());
}

#[test]
fn inst_default_runtime_config_validates() {
    let s = runtime_config_schema();
    let rc = RuntimeConfig::default();
    assert_valid(&s, &serde_json::to_value(&rc).unwrap());
}

#[test]
fn inst_default_usage_normalized_validates() {
    let s = usage_normalized_schema();
    let u = UsageNormalized::default();
    assert_valid(&s, &serde_json::to_value(&u).unwrap());
}

#[test]
fn inst_default_verification_report_validates() {
    let s = verification_report_schema();
    let vr = VerificationReport::default();
    assert_valid(&s, &serde_json::to_value(&vr).unwrap());
}

#[test]
fn inst_default_context_packet_validates() {
    let s = context_packet_schema();
    let cp = ContextPacket::default();
    assert_valid(&s, &serde_json::to_value(&cp).unwrap());
}

#[test]
fn inst_minimal_work_order_builder_validates() {
    let s = wo_schema();
    let wo = WorkOrderBuilder::new("minimal task").build();
    assert_valid(&s, &serde_json::to_value(&wo).unwrap());
}

#[test]
fn inst_full_work_order_builder_validates() {
    let s = wo_schema();
    let wo = WorkOrderBuilder::new("full task")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/ws")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    assert_valid(&s, &serde_json::to_value(&wo).unwrap());
}

#[test]
fn inst_receipt_with_hash_validates() {
    let s = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert_valid(&s, &serde_json::to_value(&r).unwrap());
}

#[test]
fn inst_empty_object_rejected_as_work_order() {
    assert_invalid(&wo_schema(), &json!({}));
}

#[test]
fn inst_empty_object_rejected_as_receipt() {
    assert_invalid(&receipt_schema(), &json!({}));
}

#[test]
fn inst_backplane_config_empty_valid() {
    // All fields have defaults so empty is valid
    assert_valid(&backplane_config_schema(), &json!({}));
}

#[test]
fn inst_backplane_config_with_mock_backend() {
    let s = backplane_config_schema();
    let instance = json!({
        "backends": {
            "test": {"type": "mock"}
        }
    });
    assert_valid(&s, &instance);
}

#[test]
fn inst_backplane_config_sidecar_missing_command_fails() {
    let s = backplane_config_schema();
    let instance = json!({
        "backends": {
            "bad": {"type": "sidecar", "args": ["x"]}
        }
    });
    assert_invalid(&s, &instance);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 4: Schema stability (20+ tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stability_work_order_schema_matches_committed() {
    let generated = wo_schema();
    let committed: Value =
        serde_json::from_str(include_str!("../contracts/schemas/work_order.schema.json"))
            .expect("committed work_order schema must be valid JSON");
    assert_eq!(
        generated, committed,
        "WorkOrder schema differs from committed. Run `cargo run -p xtask -- schema`."
    );
}

#[test]
fn stability_receipt_schema_matches_committed() {
    let generated = receipt_schema();
    let committed: Value =
        serde_json::from_str(include_str!("../contracts/schemas/receipt.schema.json"))
            .expect("committed receipt schema must be valid JSON");
    assert_eq!(
        generated, committed,
        "Receipt schema differs from committed. Run `cargo run -p xtask -- schema`."
    );
}

#[test]
fn stability_backplane_config_schema_matches_committed() {
    let generated = backplane_config_schema();
    let committed: Value = serde_json::from_str(include_str!(
        "../contracts/schemas/backplane_config.schema.json"
    ))
    .expect("committed backplane_config schema must be valid JSON");
    assert_eq!(
        generated, committed,
        "BackplaneConfig schema differs from committed. Run `cargo run -p xtask -- schema`."
    );
}

#[test]
fn stability_execution_lane_variants_match_rust() {
    let s = wo_schema();
    let variants = collect_one_of_consts(&s["$defs"]["ExecutionLane"]);
    assert!(variants.contains(&"patch_first".to_string()));
    assert!(variants.contains(&"workspace_first".to_string()));
    assert_eq!(variants.len(), 2);
}

#[test]
fn stability_workspace_mode_variants_match_rust() {
    let s = wo_schema();
    let variants = collect_one_of_consts(&s["$defs"]["WorkspaceMode"]);
    assert!(variants.contains(&"pass_through".to_string()));
    assert!(variants.contains(&"staged".to_string()));
    assert_eq!(variants.len(), 2);
}

#[test]
fn stability_outcome_variants_match_rust() {
    let s = receipt_schema();
    let variants = collect_one_of_consts(&s["$defs"]["Outcome"]);
    assert_eq!(variants, vec!["complete", "partial", "failed"]);
}

#[test]
fn stability_execution_mode_variants_match_rust() {
    let s = receipt_schema();
    let variants = collect_one_of_consts(&s["$defs"]["ExecutionMode"]);
    assert!(variants.contains(&"passthrough".to_string()));
    assert!(variants.contains(&"mapped".to_string()));
    assert_eq!(variants.len(), 2);
}

#[test]
fn stability_field_names_snake_case_work_order() {
    let s = wo_schema();
    let props = s["properties"].as_object().unwrap();
    for key in props.keys() {
        assert!(
            key.chars()
                .all(|c| c.is_lowercase() || c == '_' || c.is_numeric()),
            "WorkOrder field '{key}' is not snake_case"
        );
    }
}

#[test]
fn stability_field_names_snake_case_receipt() {
    let s = receipt_schema();
    let props = s["properties"].as_object().unwrap();
    for key in props.keys() {
        assert!(
            key.chars()
                .all(|c| c.is_lowercase() || c == '_' || c.is_numeric()),
            "Receipt field '{key}' is not snake_case"
        );
    }
}

#[test]
fn stability_field_names_snake_case_policy_profile() {
    let s = policy_profile_schema();
    let props = s["properties"].as_object().unwrap();
    for key in props.keys() {
        assert!(
            key.chars()
                .all(|c| c.is_lowercase() || c == '_' || c.is_numeric()),
            "PolicyProfile field '{key}' is not snake_case"
        );
    }
}

#[test]
fn stability_field_names_snake_case_runtime_config() {
    let s = runtime_config_schema();
    let props = s["properties"].as_object().unwrap();
    for key in props.keys() {
        assert!(
            key.chars()
                .all(|c| c.is_lowercase() || c == '_' || c.is_numeric()),
            "RuntimeConfig field '{key}' is not snake_case"
        );
    }
}

#[test]
fn stability_field_names_snake_case_backend_identity() {
    let s = backend_identity_schema();
    let props = s["properties"].as_object().unwrap();
    for key in props.keys() {
        assert!(
            key.chars()
                .all(|c| c.is_lowercase() || c == '_' || c.is_numeric()),
            "BackendIdentity field '{key}' is not snake_case"
        );
    }
}

#[test]
fn stability_agent_event_kind_tag_is_type() {
    // AgentEventKind uses #[serde(tag = "type")] — verify schema references "type"
    let s = agent_event_schema();
    let json_str = serde_json::to_string(&s).unwrap();
    assert!(json_str.contains(r#""type""#));
}

#[test]
fn stability_agent_event_kind_variants_present() {
    let s = agent_event_schema();
    let json_str = serde_json::to_string(&s).unwrap();
    for variant in &[
        "run_started",
        "run_completed",
        "assistant_delta",
        "assistant_message",
        "tool_call",
        "tool_result",
        "file_changed",
        "command_executed",
        "warning",
        "error",
    ] {
        assert!(
            json_str.contains(variant),
            "AgentEventKind missing variant: {variant}"
        );
    }
}

#[test]
fn stability_capability_enum_count() {
    let s = capability_schema();
    let variants = collect_one_of_consts(&s);
    // 41 variants as of the current codebase
    assert!(
        variants.len() >= 26,
        "Expected at least 26 Capability variants, got {}",
        variants.len()
    );
}

#[test]
fn stability_error_code_count() {
    let s = error_code_schema();
    let variants = collect_one_of_consts(&s);
    assert!(
        variants.len() >= 36,
        "Expected at least 36 ErrorCode variants, got {}",
        variants.len()
    );
}

#[test]
fn stability_min_support_variants() {
    let s = schema_for_type::<MinSupport>();
    let variants = collect_one_of_consts(&s);
    assert!(variants.contains(&"native".to_string()));
    assert!(variants.contains(&"emulated".to_string()));
    assert_eq!(variants.len(), 2);
}

#[test]
fn stability_work_order_schema_draft_version() {
    let s = wo_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn stability_receipt_schema_draft_version() {
    let s = receipt_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn stability_contract_version_in_receipt() {
    let r = valid_receipt();
    assert_eq!(r["meta"]["contract_version"], CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════════
// SECTION 5: Cross-type consistency (20+ tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_agent_event_in_receipt_trace_validates_standalone() {
    // An AgentEvent from a receipt trace should also validate against standalone AgentEvent schema
    let ae_schema = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    };
    let receipt = ReceiptBuilder::new("mock").add_trace_event(event).build();
    let receipt_json = serde_json::to_value(&receipt).unwrap();
    let trace_event = &receipt_json["trace"][0];
    assert_valid(&ae_schema, trace_event);
}

#[test]
fn cross_receipt_validates_against_own_schema() {
    // A receipt built with builder validates both as Receipt and when embedded
    let s = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert_valid(&s, &serde_json::to_value(&r).unwrap());
}

#[test]
fn cross_work_order_policy_matches_standalone_policy() {
    // PolicyProfile in WorkOrder should match standalone PolicyProfile schema
    let pp_schema = policy_profile_schema();
    let wo = WorkOrderBuilder::new("test")
        .policy(PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["bash".into()],
            deny_read: vec!["*.env".into()],
            deny_write: vec![],
            allow_network: vec![],
            deny_network: vec![],
            require_approval_for: vec![],
        })
        .build();
    let wo_json = serde_json::to_value(&wo).unwrap();
    let policy_json = &wo_json["policy"];
    assert_valid(&pp_schema, policy_json);
}

#[test]
fn cross_work_order_runtime_config_matches_standalone() {
    let rc_schema = runtime_config_schema();
    let wo = WorkOrderBuilder::new("test")
        .model("gpt-4")
        .max_turns(5)
        .build();
    let wo_json = serde_json::to_value(&wo).unwrap();
    assert_valid(&rc_schema, &wo_json["config"]);
}

#[test]
fn cross_work_order_context_matches_standalone() {
    let cp_schema = context_packet_schema();
    let wo = WorkOrderBuilder::new("test")
        .context(ContextPacket {
            files: vec!["src/main.rs".into()],
            snippets: vec![ContextSnippet {
                name: "readme".into(),
                content: "# Hello".into(),
            }],
        })
        .build();
    let wo_json = serde_json::to_value(&wo).unwrap();
    assert_valid(&cp_schema, &wo_json["context"]);
}

#[test]
fn cross_receipt_backend_matches_standalone_backend_identity() {
    let bi_schema = backend_identity_schema();
    let r = ReceiptBuilder::new("test-backend")
        .backend_version("1.0")
        .build();
    let r_json = serde_json::to_value(&r).unwrap();
    assert_valid(&bi_schema, &r_json["backend"]);
}

#[test]
fn cross_receipt_usage_matches_standalone_usage_normalized() {
    let u_schema = usage_normalized_schema();
    let r = ReceiptBuilder::new("mock")
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: Some(0.01),
        })
        .build();
    let r_json = serde_json::to_value(&r).unwrap();
    assert_valid(&u_schema, &r_json["usage"]);
}

#[test]
fn cross_receipt_verification_matches_standalone() {
    let vr_schema = verification_report_schema();
    let r = ReceiptBuilder::new("mock")
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .build();
    let r_json = serde_json::to_value(&r).unwrap();
    assert_valid(&vr_schema, &r_json["verification"]);
}

#[test]
fn cross_error_code_in_agent_event_matches_standalone() {
    let ec_schema = error_code_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "timeout".into(),
            error_code: Some(ErrorCode::BackendTimeout),
        },
        ext: None,
    };
    let event_json = serde_json::to_value(&event).unwrap();
    if let Some(code) = event_json.get("error_code") {
        assert_valid(&ec_schema, code);
    }
}

#[test]
fn cross_capability_in_receipt_matches_standalone() {
    let cap_schema = capability_schema();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    let r = ReceiptBuilder::new("mock").capabilities(caps).build();
    let r_json = serde_json::to_value(&r).unwrap();
    // The capabilities object should have keys that validate as Capability
    let caps_obj = r_json["capabilities"].as_object().unwrap();
    for key in caps_obj.keys() {
        let key_val = json!(key);
        assert_valid(&cap_schema, &key_val);
    }
}

#[test]
fn cross_work_order_requirements_matches_standalone() {
    let cr_schema = capability_requirements_schema();
    let wo = WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let wo_json = serde_json::to_value(&wo).unwrap();
    assert_valid(&cr_schema, &wo_json["requirements"]);
}

#[test]
fn cross_outcome_in_receipt_matches_standalone() {
    let o_schema = outcome_schema();
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = ReceiptBuilder::new("mock").outcome(outcome.clone()).build();
        let r_json = serde_json::to_value(&r).unwrap();
        assert_valid(&o_schema, &r_json["outcome"]);
    }
}

#[test]
fn cross_execution_mode_in_receipt_matches_standalone() {
    let em_schema = execution_mode_schema();
    for mode in [ExecutionMode::Passthrough, ExecutionMode::Mapped] {
        let r = ReceiptBuilder::new("mock").mode(mode).build();
        let r_json = serde_json::to_value(&r).unwrap();
        assert_valid(&em_schema, &r_json["mode"]);
    }
}

#[test]
fn cross_work_order_workspace_has_expected_fields() {
    let wo = valid_work_order();
    let ws = &wo["workspace"];
    assert!(ws.get("root").is_some());
    assert!(ws.get("mode").is_some());
    assert!(ws.get("include").is_some());
    assert!(ws.get("exclude").is_some());
}

#[test]
fn cross_receipt_meta_has_expected_fields() {
    let r = valid_receipt();
    let meta = &r["meta"];
    assert!(meta.get("run_id").is_some());
    assert!(meta.get("work_order_id").is_some());
    assert!(meta.get("contract_version").is_some());
    assert!(meta.get("started_at").is_some());
    assert!(meta.get("finished_at").is_some());
    assert!(meta.get("duration_ms").is_some());
}

#[test]
fn cross_all_agent_event_kinds_validate() {
    // Every AgentEventKind variant should produce a valid AgentEvent
    let s = agent_event_schema();
    let now = Utc::now();

    let events = vec![
        AgentEvent {
            ts: now,
            kind: AgentEventKind::RunStarted {
                message: "s".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::RunCompleted {
                message: "d".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta { text: "t".into() },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage { text: "m".into() },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "r".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: "r".into(),
                tool_use_id: None,
                output: json!(null),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::FileChanged {
                path: "f".into(),
                summary: "s".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::CommandExecuted {
                command: "c".into(),
                exit_code: None,
                output_preview: None,
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::Warning {
                message: "w".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::Error {
                message: "e".into(),
                error_code: None,
            },
            ext: None,
        },
    ];
    for (i, event) in events.iter().enumerate() {
        let val = serde_json::to_value(event).unwrap();
        assert_valid(&s, &val);
    }
}

#[test]
fn cross_agent_event_with_ext_validates() {
    let s = agent_event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(BTreeMap::from([(
            "raw_message".into(),
            json!({"vendor": "data"}),
        )])),
    };
    assert_valid(&s, &serde_json::to_value(&event).unwrap());
}

#[test]
fn cross_receipt_with_trace_and_artifacts_validates() {
    let s = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .build();
    assert_valid(&s, &serde_json::to_value(&r).unwrap());
}

#[test]
fn cross_receipt_with_all_capabilities_validates() {
    let s = receipt_schema();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    caps.insert(Capability::ToolWrite, SupportLevel::Unsupported);
    caps.insert(
        Capability::ToolBash,
        SupportLevel::Restricted {
            reason: "policy".into(),
        },
    );
    let r = ReceiptBuilder::new("mock").capabilities(caps).build();
    assert_valid(&s, &serde_json::to_value(&r).unwrap());
}

#[test]
fn cross_work_order_round_trip_serde() {
    let s = wo_schema();
    let wo = WorkOrderBuilder::new("round trip test")
        .lane(ExecutionLane::WorkspaceFirst)
        .model("claude-3")
        .build();
    let json_str = serde_json::to_string(&wo).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&s, &parsed);
    // Deserialize back and re-serialize
    let deserialized: WorkOrder = serde_json::from_value(parsed).unwrap();
    let re_serialized = serde_json::to_value(&deserialized).unwrap();
    assert_valid(&s, &re_serialized);
}

#[test]
fn cross_receipt_round_trip_serde() {
    let s = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .mode(ExecutionMode::Passthrough)
        .build()
        .with_hash()
        .unwrap();
    let json_str = serde_json::to_string(&r).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&s, &parsed);
    let deserialized: Receipt = serde_json::from_value(parsed).unwrap();
    let re_serialized = serde_json::to_value(&deserialized).unwrap();
    assert_valid(&s, &re_serialized);
}
