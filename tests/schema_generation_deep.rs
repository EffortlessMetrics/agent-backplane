// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for JSON schema generation covering validity, structure,
//! required fields, type mappings, enum variants, $ref usage, canonical
//! serialization, roundtrips, backward compatibility, and diff detection.

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane,
    ExecutionMode, MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata,
    RuntimeConfig, SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, WorkspaceSpec,
};
use chrono::Utc;
use schemars::schema_for;
use serde_json::{Value, json};
use std::collections::BTreeMap;

// ── helpers ──────────────────────────────────────────────────────────

fn wo_schema() -> Value {
    serde_json::to_value(schema_for!(WorkOrder)).unwrap()
}

fn receipt_schema() -> Value {
    serde_json::to_value(schema_for!(Receipt)).unwrap()
}

fn event_schema() -> Value {
    serde_json::to_value(schema_for!(AgentEvent)).unwrap()
}

fn event_kind_schema() -> Value {
    serde_json::to_value(schema_for!(AgentEventKind)).unwrap()
}

fn capability_schema() -> Value {
    serde_json::to_value(schema_for!(Capability)).unwrap()
}

fn outcome_schema() -> Value {
    serde_json::to_value(schema_for!(Outcome)).unwrap()
}

fn support_level_schema() -> Value {
    serde_json::to_value(schema_for!(SupportLevel)).unwrap()
}

fn execution_mode_schema() -> Value {
    serde_json::to_value(schema_for!(ExecutionMode)).unwrap()
}

fn policy_schema() -> Value {
    serde_json::to_value(schema_for!(PolicyProfile)).unwrap()
}

fn runtime_config_schema() -> Value {
    serde_json::to_value(schema_for!(RuntimeConfig)).unwrap()
}

fn workspace_spec_schema() -> Value {
    serde_json::to_value(schema_for!(WorkspaceSpec)).unwrap()
}

fn backplane_config_schema() -> Value {
    serde_json::to_value(schema_for!(abp_cli::config::BackplaneConfig)).unwrap()
}

fn valid_wo() -> Value {
    serde_json::to_value(WorkOrderBuilder::new("test task").build()).unwrap()
}

fn valid_receipt() -> Value {
    serde_json::to_value(
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build(),
    )
    .unwrap()
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

// ── 1. WorkOrder schema is valid JSON Schema ─────────────────────────

#[test]
fn work_order_schema_declares_draft_2020_12() {
    let s = wo_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn work_order_schema_compiles() {
    jsonschema::validator_for(&wo_schema()).expect("WorkOrder schema must compile");
}

#[test]
fn work_order_schema_title_matches_type() {
    assert_eq!(wo_schema()["title"], "WorkOrder");
}

#[test]
fn work_order_schema_top_level_type_is_object() {
    assert_eq!(wo_schema()["type"], "object");
}

// ── 2. Receipt schema is valid JSON Schema ───────────────────────────

#[test]
fn receipt_schema_declares_draft_2020_12() {
    let s = receipt_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn receipt_schema_compiles() {
    jsonschema::validator_for(&receipt_schema()).expect("Receipt schema must compile");
}

#[test]
fn receipt_schema_title_matches_type() {
    assert_eq!(receipt_schema()["title"], "Receipt");
}

#[test]
fn receipt_schema_top_level_type_is_object() {
    assert_eq!(receipt_schema()["type"], "object");
}

// ── 3. AgentEvent schema is valid JSON Schema ────────────────────────

#[test]
fn agent_event_schema_declares_draft_2020_12() {
    let s = event_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

#[test]
fn agent_event_schema_compiles() {
    jsonschema::validator_for(&event_schema()).expect("AgentEvent schema must compile");
}

#[test]
fn agent_event_schema_title() {
    assert_eq!(event_schema()["title"], "AgentEvent");
}

// ── 4. Schema has required fields correct ────────────────────────────

#[test]
fn work_order_required_fields_match_struct() {
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
    assert_eq!(required.len(), 8, "unexpected number of required fields");
}

#[test]
fn receipt_required_fields_include_meta_and_outcome() {
    let required = get_required(&receipt_schema());
    for f in &[
        "meta",
        "backend",
        "outcome",
        "trace",
        "usage",
        "artifacts",
        "verification",
    ] {
        assert!(required.contains(&f.to_string()), "missing required: {f}");
    }
}

#[test]
fn receipt_sha256_not_required() {
    let required = get_required(&receipt_schema());
    assert!(
        !required.contains(&"receipt_sha256".to_string()),
        "receipt_sha256 should be optional"
    );
}

#[test]
fn run_metadata_required_fields() {
    let defs = &receipt_schema()["$defs"]["RunMetadata"];
    let required = get_required(defs);
    for f in &[
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(
            required.contains(&f.to_string()),
            "RunMetadata missing required: {f}"
        );
    }
}

#[test]
fn policy_profile_required_fields() {
    let required = get_required(&policy_schema());
    for f in &[
        "allowed_tools",
        "disallowed_tools",
        "deny_read",
        "deny_write",
        "allow_network",
        "deny_network",
        "require_approval_for",
    ] {
        assert!(
            required.contains(&f.to_string()),
            "PolicyProfile missing required: {f}"
        );
    }
}

#[test]
fn workspace_spec_required_fields() {
    let required = get_required(&workspace_spec_schema());
    for f in &["root", "mode", "include", "exclude"] {
        assert!(
            required.contains(&f.to_string()),
            "WorkspaceSpec missing required: {f}"
        );
    }
}

#[test]
fn backend_identity_required_fields() {
    let defs = &receipt_schema()["$defs"]["BackendIdentity"];
    let required = get_required(defs);
    assert!(required.contains(&"id".to_string()));
    // backend_version and adapter_version are optional
    assert!(!required.contains(&"backend_version".to_string()));
    assert!(!required.contains(&"adapter_version".to_string()));
}

#[test]
fn capability_requirement_required_fields() {
    let defs = &wo_schema()["$defs"]["CapabilityRequirement"];
    let required = get_required(defs);
    assert!(required.contains(&"capability".to_string()));
    assert!(required.contains(&"min_support".to_string()));
}

// ── 5. Schema types match Rust types ─────────────────────────────────

#[test]
fn work_order_id_has_uuid_format() {
    let props = &wo_schema()["properties"]["id"];
    assert_eq!(props["type"], "string");
    assert_eq!(props["format"], "uuid");
}

#[test]
fn work_order_task_is_string() {
    assert_eq!(wo_schema()["properties"]["task"]["type"], "string");
}

#[test]
fn runtime_config_model_is_nullable_string() {
    let s = runtime_config_schema();
    let model_type = &s["properties"]["model"]["type"];
    let arr = model_type
        .as_array()
        .expect("model type should be an array");
    let strs: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    assert!(strs.contains(&"string"));
    assert!(strs.contains(&"null"));
}

#[test]
fn runtime_config_max_budget_is_nullable_number() {
    let s = runtime_config_schema();
    let ty = &s["properties"]["max_budget_usd"]["type"];
    let arr = ty.as_array().expect("max_budget_usd type should be array");
    let strs: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    assert!(strs.contains(&"number"));
    assert!(strs.contains(&"null"));
}

#[test]
fn runtime_config_max_turns_is_nullable_integer() {
    let s = runtime_config_schema();
    let ty = &s["properties"]["max_turns"]["type"];
    let arr = ty.as_array().expect("max_turns type should be array");
    let strs: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    assert!(strs.contains(&"integer"));
    assert!(strs.contains(&"null"));
}

#[test]
fn run_metadata_duration_ms_is_uint64() {
    let defs = &receipt_schema()["$defs"]["RunMetadata"];
    let dur = &defs["properties"]["duration_ms"];
    assert_eq!(dur["type"], "integer");
    assert_eq!(dur["format"], "uint64");
}

#[test]
fn run_metadata_timestamps_are_datetime() {
    let defs = &receipt_schema()["$defs"]["RunMetadata"];
    assert_eq!(defs["properties"]["started_at"]["format"], "date-time");
    assert_eq!(defs["properties"]["finished_at"]["format"], "date-time");
}

#[test]
fn agent_event_ts_is_datetime() {
    let s = event_schema();
    assert_eq!(s["properties"]["ts"]["format"], "date-time");
}

#[test]
fn usage_normalized_tokens_are_nullable_uint64() {
    let defs = &receipt_schema()["$defs"]["UsageNormalized"];
    for field in &[
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
    ] {
        let ty = &defs["properties"][field]["type"];
        let arr = ty
            .as_array()
            .unwrap_or_else(|| panic!("{field} type should be array"));
        let strs: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"integer"), "{field} should include integer");
        assert!(strs.contains(&"null"), "{field} should be nullable");
    }
}

#[test]
fn receipt_sha256_is_nullable_string() {
    let s = receipt_schema();
    let ty = &s["properties"]["receipt_sha256"]["type"];
    let arr = ty.as_array().expect("receipt_sha256 type should be array");
    let strs: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    assert!(strs.contains(&"string"));
    assert!(strs.contains(&"null"));
}

#[test]
fn context_packet_files_is_string_array() {
    let defs = &wo_schema()["$defs"]["ContextPacket"];
    let files = &defs["properties"]["files"];
    assert_eq!(files["type"], "array");
    assert_eq!(files["items"]["type"], "string");
}

// ── 6. Schema descriptions from doc comments ─────────────────────────

#[test]
fn work_order_has_description() {
    let s = wo_schema();
    let desc = s["description"].as_str().unwrap();
    assert!(
        desc.contains("single unit of work"),
        "WorkOrder description should mention 'single unit of work'"
    );
}

#[test]
fn receipt_has_description() {
    let s = receipt_schema();
    let desc = s["description"].as_str().unwrap();
    assert!(
        desc.contains("outcome of a completed run"),
        "Receipt description should mention 'outcome of a completed run'"
    );
}

#[test]
fn agent_event_has_description() {
    let s = event_schema();
    let desc = s["description"].as_str().unwrap();
    assert!(
        desc.contains("timestamped event"),
        "AgentEvent description should mention 'timestamped event'"
    );
}

#[test]
fn policy_profile_has_description() {
    let s = policy_schema();
    let desc = s["description"].as_str().unwrap();
    assert!(desc.contains("Security policy"));
}

#[test]
fn runtime_config_has_description() {
    let s = runtime_config_schema();
    let desc = s["description"].as_str().unwrap();
    assert!(desc.contains("Runtime-level knobs"));
}

#[test]
fn field_descriptions_present_in_work_order() {
    let s = wo_schema();
    let props = s["properties"].as_object().unwrap();
    for (key, val) in props {
        assert!(
            val.get("description").is_some() || val.get("$ref").is_some(),
            "field '{key}' should have description or $ref"
        );
    }
}

#[test]
fn capability_variants_have_descriptions() {
    let s = capability_schema();
    let variants = s["oneOf"].as_array().expect("Capability should use oneOf");
    for v in variants {
        assert!(
            v.get("description").is_some(),
            "each Capability variant should have a description"
        );
    }
}

// ── 7. Schema enum variants listed ───────────────────────────────────

#[test]
fn execution_lane_variants() {
    let defs = &wo_schema()["$defs"]["ExecutionLane"];
    let variants = defs["oneOf"]
        .as_array()
        .expect("ExecutionLane should use oneOf");
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"patch_first"));
    assert!(consts.contains(&"workspace_first"));
    assert_eq!(consts.len(), 2);
}

#[test]
fn workspace_mode_variants() {
    let defs = &wo_schema()["$defs"]["WorkspaceMode"];
    let variants = defs["oneOf"]
        .as_array()
        .expect("WorkspaceMode should use oneOf");
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"pass_through"));
    assert!(consts.contains(&"staged"));
    assert_eq!(consts.len(), 2);
}

#[test]
fn outcome_variants() {
    let s = outcome_schema();
    let variants = s["oneOf"].as_array().expect("Outcome should use oneOf");
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
fn execution_mode_variants() {
    let s = execution_mode_schema();
    let variants = s["oneOf"]
        .as_array()
        .expect("ExecutionMode should use oneOf");
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"passthrough"));
    assert!(consts.contains(&"mapped"));
    assert_eq!(consts.len(), 2);
}

#[test]
fn min_support_variants() {
    let defs = &wo_schema()["$defs"]["MinSupport"];
    let variants = defs["oneOf"]
        .as_array()
        .expect("MinSupport should use oneOf");
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"native"));
    assert!(consts.contains(&"emulated"));
    assert_eq!(consts.len(), 2);
}

#[test]
fn support_level_variants() {
    let s = support_level_schema();
    let variants = s["oneOf"]
        .as_array()
        .expect("SupportLevel should use oneOf");
    assert!(
        variants.len() >= 4,
        "SupportLevel should have at least 4 variants"
    );
    // Check simple string variants
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    assert!(consts.contains(&"native"));
    assert!(consts.contains(&"emulated"));
    assert!(consts.contains(&"unsupported"));
    // Restricted is an object variant
    let has_restricted = variants.iter().any(|v| {
        v["properties"]
            .as_object()
            .is_some_and(|m| m.contains_key("restricted"))
    });
    assert!(
        has_restricted,
        "SupportLevel should have 'restricted' object variant"
    );
}

#[test]
fn capability_enum_has_all_variants() {
    let s = capability_schema();
    let variants = s["oneOf"].as_array().expect("Capability should use oneOf");
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
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
    ];
    for e in &expected {
        assert!(consts.contains(e), "Capability missing variant: {e}");
    }
    assert_eq!(consts.len(), expected.len());
}

#[test]
fn agent_event_kind_variants_in_schema() {
    let s = event_kind_schema();
    let variants = s["oneOf"]
        .as_array()
        .expect("AgentEventKind should use oneOf");
    let type_consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["properties"]["type"]["const"].as_str())
        .collect();
    let expected = [
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
    ];
    for e in &expected {
        assert!(
            type_consts.contains(e),
            "AgentEventKind missing variant: {e}"
        );
    }
    assert_eq!(type_consts.len(), expected.len());
}

#[test]
fn error_code_enum_in_receipt_schema() {
    let defs = &receipt_schema()["$defs"];
    assert!(
        defs.get("ErrorCode").is_some(),
        "Receipt schema $defs should contain ErrorCode"
    );
    let error_code = &defs["ErrorCode"];
    let variants = error_code["oneOf"]
        .as_array()
        .expect("ErrorCode should use oneOf");
    let consts: Vec<&str> = variants
        .iter()
        .filter_map(|v| v["const"].as_str())
        .collect();
    // Just check a few key codes
    assert!(consts.contains(&"BACKEND_TIMEOUT"));
    assert!(consts.contains(&"POLICY_DENIED"));
    assert!(consts.contains(&"INTERNAL"));
    assert!(consts.contains(&"PROTOCOL_INVALID_ENVELOPE"));
}

// ── 8. Schema additionalProperties settings ──────────────────────────

#[test]
fn runtime_config_vendor_allows_additional_properties() {
    let s = runtime_config_schema();
    let vendor = &s["properties"]["vendor"];
    assert_eq!(
        vendor["additionalProperties"], true,
        "vendor map should allow additionalProperties"
    );
}

#[test]
fn runtime_config_env_maps_string_to_string() {
    let s = runtime_config_schema();
    let env = &s["properties"]["env"];
    assert_eq!(env["type"], "object");
    assert_eq!(env["additionalProperties"]["type"], "string");
}

#[test]
fn receipt_capabilities_has_additional_properties() {
    let s = receipt_schema();
    let caps = &s["properties"]["capabilities"];
    assert_eq!(caps["type"], "object");
    // additionalProperties should $ref SupportLevel
    let ap = &caps["additionalProperties"];
    assert!(
        ap.get("$ref").is_some(),
        "capabilities additionalProperties should use $ref"
    );
}

#[test]
fn agent_event_ext_allows_additional_properties() {
    let s = event_schema();
    let ext = &s["properties"]["ext"];
    assert_eq!(ext["additionalProperties"], true);
}

#[test]
fn support_level_restricted_disallows_additional_properties() {
    let s = support_level_schema();
    let variants = s["oneOf"].as_array().unwrap();
    let restricted = variants
        .iter()
        .find(|v| {
            v["properties"]
                .as_object()
                .is_some_and(|m| m.contains_key("restricted"))
        })
        .expect("restricted variant exists");
    assert_eq!(restricted["additionalProperties"], false);
}

// ── 9. Schema $ref usage for shared types ────────────────────────────

#[test]
fn work_order_uses_refs_for_nested_types() {
    let s = wo_schema();
    let refs = collect_refs(&s["properties"]);
    assert!(
        refs.iter().any(|r| r.contains("RuntimeConfig")),
        "config should $ref RuntimeConfig"
    );
    assert!(
        refs.iter().any(|r| r.contains("PolicyProfile")),
        "policy should $ref PolicyProfile"
    );
    assert!(
        refs.iter().any(|r| r.contains("WorkspaceSpec")),
        "workspace should $ref WorkspaceSpec"
    );
    assert!(
        refs.iter().any(|r| r.contains("ContextPacket")),
        "context should $ref ContextPacket"
    );
    assert!(
        refs.iter().any(|r| r.contains("ExecutionLane")),
        "lane should $ref ExecutionLane"
    );
}

#[test]
fn receipt_uses_refs_for_nested_types() {
    let s = receipt_schema();
    let refs = collect_refs(&s["properties"]);
    assert!(refs.iter().any(|r| r.contains("BackendIdentity")));
    assert!(refs.iter().any(|r| r.contains("RunMetadata")));
    assert!(refs.iter().any(|r| r.contains("Outcome")));
    assert!(refs.iter().any(|r| r.contains("UsageNormalized")));
    assert!(refs.iter().any(|r| r.contains("VerificationReport")));
    assert!(refs.iter().any(|r| r.contains("AgentEvent")));
}

#[test]
fn all_refs_resolve_to_existing_defs_in_work_order() {
    let s = wo_schema();
    let all_refs = collect_refs(&s);
    let defs = get_defs(&s);
    for r in &all_refs {
        let name = r
            .strip_prefix("#/$defs/")
            .expect("$ref should start with #/$defs/");
        assert!(defs.contains(&name.to_string()), "unresolved $ref: {r}");
    }
}

#[test]
fn all_refs_resolve_to_existing_defs_in_receipt() {
    let s = receipt_schema();
    let all_refs = collect_refs(&s);
    let defs = get_defs(&s);
    for r in &all_refs {
        let name = r
            .strip_prefix("#/$defs/")
            .expect("$ref should start with #/$defs/");
        assert!(defs.contains(&name.to_string()), "unresolved $ref: {r}");
    }
}

#[test]
fn work_order_defs_not_empty() {
    let defs = get_defs(&wo_schema());
    assert!(
        defs.len() >= 8,
        "WorkOrder should have at least 8 definitions"
    );
}

#[test]
fn receipt_defs_not_empty() {
    let defs = get_defs(&receipt_schema());
    assert!(
        defs.len() >= 8,
        "Receipt should have at least 8 definitions"
    );
}

// ── 10. Schema version/id field ──────────────────────────────────────

#[test]
fn all_schemas_use_draft_2020_12() {
    let schemas = vec![wo_schema(), receipt_schema(), event_schema()];
    for s in &schemas {
        assert_eq!(
            s["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "all schemas must use draft 2020-12"
        );
    }
}

#[test]
fn run_metadata_contract_version_is_string() {
    let defs = &receipt_schema()["$defs"]["RunMetadata"];
    assert_eq!(defs["properties"]["contract_version"]["type"], "string");
}

#[test]
fn contract_version_validates_in_receipt() {
    let r = valid_receipt();
    let cv = r["meta"]["contract_version"].as_str().unwrap();
    assert_eq!(cv, CONTRACT_VERSION);
}

#[test]
fn backplane_config_schema_has_draft() {
    let s = backplane_config_schema();
    assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
}

// ── 11. Canonical JSON serialization of schema ───────────────────────

#[test]
fn schema_serialization_is_deterministic() {
    let s1 = serde_json::to_string(&schema_for!(WorkOrder)).unwrap();
    let s2 = serde_json::to_string(&schema_for!(WorkOrder)).unwrap();
    assert_eq!(s1, s2, "schema serialization must be deterministic");
}

#[test]
fn receipt_schema_serialization_deterministic() {
    let s1 = serde_json::to_string(&schema_for!(Receipt)).unwrap();
    let s2 = serde_json::to_string(&schema_for!(Receipt)).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn schema_keys_are_sorted_in_defs() {
    let s = wo_schema();
    let defs = s["$defs"].as_object().unwrap();
    let keys: Vec<&String> = defs.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "$defs keys should be sorted (BTreeMap)");
}

#[test]
fn schema_pretty_print_matches_compact_roundtrip() {
    let schema = schema_for!(WorkOrder);
    let pretty = serde_json::to_string_pretty(&schema).unwrap();
    let compact = serde_json::to_string(&schema).unwrap();
    let from_pretty: Value = serde_json::from_str(&pretty).unwrap();
    let from_compact: Value = serde_json::from_str(&compact).unwrap();
    assert_eq!(from_pretty, from_compact);
}

// ── 12. Schema roundtrip (generate → validate → generate) ───────────

#[test]
fn work_order_roundtrip_generate_validate_generate() {
    let schema1 = wo_schema();
    let wo = WorkOrderBuilder::new("roundtrip test").build();
    let instance = serde_json::to_value(&wo).unwrap();
    assert_valid(&schema1, &instance);

    let deserialized: WorkOrder = serde_json::from_value(instance.clone()).unwrap();
    let re_serialized = serde_json::to_value(&deserialized).unwrap();
    let schema2 = wo_schema();
    assert_valid(&schema2, &re_serialized);
    assert_eq!(schema1, schema2, "re-generated schema must match");
}

#[test]
fn receipt_roundtrip_generate_validate_generate() {
    let schema1 = receipt_schema();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let instance = serde_json::to_value(&receipt).unwrap();
    assert_valid(&schema1, &instance);

    let deserialized: Receipt = serde_json::from_value(instance).unwrap();
    let re_serialized = serde_json::to_value(&deserialized).unwrap();
    let schema2 = receipt_schema();
    assert_valid(&schema2, &re_serialized);
    assert_eq!(schema1, schema2);
}

#[test]
fn agent_event_roundtrip() {
    let schema = event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    let val = serde_json::to_value(&event).unwrap();
    assert_valid(&schema, &val);

    let back: AgentEvent = serde_json::from_value(val.clone()).unwrap();
    let re_val = serde_json::to_value(&back).unwrap();
    assert_valid(&schema, &re_val);
}

#[test]
fn all_event_kinds_roundtrip_validate() {
    let schema = event_schema();
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "tok".into() },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "src/main.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                output: json!("file contents"),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "updated".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "watch out".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "boom".into(),
                error_code: None,
            },
            ext: None,
        },
    ];
    for (i, event) in events.iter().enumerate() {
        let val = serde_json::to_value(event).unwrap();
        assert_valid(&schema, &val);
        let back: AgentEvent = serde_json::from_value(val).unwrap();
        let re_val = serde_json::to_value(&back).unwrap();
        assert_valid(&schema, &re_val);
        // Verify the type tag survived
        let type_str = re_val["type"].as_str().unwrap_or("missing");
        assert_ne!(type_str, "missing", "event {i} should have 'type' field");
    }
}

#[test]
fn policy_profile_roundtrip() {
    let schema = policy_schema();
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into()],
        disallowed_tools: vec!["bash".into()],
        deny_read: vec!["*.env".into()],
        deny_write: vec!["*.lock".into()],
        allow_network: vec!["*.github.com".into()],
        deny_network: vec![],
        require_approval_for: vec!["bash".into()],
    };
    let val = serde_json::to_value(&policy).unwrap();
    assert_valid(&schema, &val);
    let back: PolicyProfile = serde_json::from_value(val).unwrap();
    let re_val = serde_json::to_value(&back).unwrap();
    assert_valid(&schema, &re_val);
}

// ── 13. Schema backward compatibility ────────────────────────────────

#[test]
fn schema_on_disk_matches_generated_work_order() {
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/work_order.schema.json").unwrap(),
    )
    .unwrap();
    let generated = wo_schema();
    assert_eq!(
        on_disk, generated,
        "on-disk work_order.schema.json must match freshly generated schema"
    );
}

#[test]
fn schema_on_disk_matches_generated_receipt() {
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/receipt.schema.json").unwrap(),
    )
    .unwrap();
    let generated = receipt_schema();
    assert_eq!(
        on_disk, generated,
        "on-disk receipt.schema.json must match freshly generated schema"
    );
}

#[test]
fn schema_on_disk_matches_generated_config() {
    let on_disk: Value = serde_json::from_str(
        &std::fs::read_to_string("contracts/schemas/backplane_config.schema.json").unwrap(),
    )
    .unwrap();
    let generated = backplane_config_schema();
    assert_eq!(
        on_disk, generated,
        "on-disk backplane_config.schema.json must match freshly generated schema"
    );
}

#[test]
fn old_valid_work_order_still_validates() {
    // Simulate a minimal v0.1 work order from a previous client
    let schema = wo_schema();
    let instance = valid_wo();
    assert_valid(&schema, &instance);
}

#[test]
fn receipt_with_empty_trace_validates() {
    let schema = receipt_schema();
    let mut r = valid_receipt();
    r["trace"] = json!([]);
    assert_valid(&schema, &r);
}

#[test]
fn receipt_with_null_receipt_sha256_validates() {
    let schema = receipt_schema();
    let mut r = valid_receipt();
    r["receipt_sha256"] = Value::Null;
    assert_valid(&schema, &r);
}

// ── 14. Schema covers all public types ───────────────────────────────

#[test]
fn work_order_schema_defs_cover_all_sub_types() {
    let defs = get_defs(&wo_schema());
    let expected = [
        "RuntimeConfig",
        "PolicyProfile",
        "WorkspaceSpec",
        "WorkspaceMode",
        "ContextPacket",
        "ContextSnippet",
        "ExecutionLane",
        "CapabilityRequirements",
        "CapabilityRequirement",
        "Capability",
        "MinSupport",
    ];
    for e in &expected {
        assert!(
            defs.contains(&e.to_string()),
            "WorkOrder $defs missing: {e}"
        );
    }
}

#[test]
fn receipt_schema_defs_cover_all_sub_types() {
    let defs = get_defs(&receipt_schema());
    let expected = [
        "RunMetadata",
        "BackendIdentity",
        "UsageNormalized",
        "VerificationReport",
        "AgentEvent",
        "ArtifactRef",
        "Outcome",
        "ExecutionMode",
        "SupportLevel",
    ];
    for e in &expected {
        assert!(defs.contains(&e.to_string()), "Receipt $defs missing: {e}");
    }
}

#[test]
fn every_work_order_property_has_description_or_ref() {
    let s = wo_schema();
    let props = s["properties"].as_object().unwrap();
    for (key, val) in props {
        let has_desc = val.get("description").and_then(|d| d.as_str()).is_some();
        let has_ref = val.get("$ref").is_some();
        assert!(
            has_desc || has_ref,
            "WorkOrder property '{key}' lacks both description and $ref"
        );
    }
}

#[test]
fn every_receipt_property_has_description_or_ref() {
    let s = receipt_schema();
    let props = s["properties"].as_object().unwrap();
    for (key, val) in props {
        let has_desc = val.get("description").and_then(|d| d.as_str()).is_some();
        let has_ref = val.get("$ref").is_some();
        assert!(
            has_desc || has_ref,
            "Receipt property '{key}' lacks both description and $ref"
        );
    }
}

#[test]
fn all_schemas_generate_without_panic() {
    // Ensure none of these panic
    let _ = schema_for!(WorkOrder);
    let _ = schema_for!(Receipt);
    let _ = schema_for!(AgentEvent);
    let _ = schema_for!(AgentEventKind);
    let _ = schema_for!(Capability);
    let _ = schema_for!(SupportLevel);
    let _ = schema_for!(ExecutionMode);
    let _ = schema_for!(Outcome);
    let _ = schema_for!(PolicyProfile);
    let _ = schema_for!(RuntimeConfig);
    let _ = schema_for!(WorkspaceSpec);
    let _ = schema_for!(ContextPacket);
    let _ = schema_for!(CapabilityRequirements);
    let _ = schema_for!(BackendIdentity);
    let _ = schema_for!(RunMetadata);
    let _ = schema_for!(UsageNormalized);
    let _ = schema_for!(VerificationReport);
    let _ = schema_for!(ArtifactRef);
    let _ = schema_for!(MinSupport);
    let _ = schema_for!(ExecutionLane);
    let _ = schema_for!(WorkspaceMode);
    let _ = schema_for!(ContextSnippet);
    let _ = schema_for!(CapabilityRequirement);
    let _ = schema_for!(abp_cli::config::BackplaneConfig);
}

#[test]
fn standalone_type_schemas_are_valid_json_schema() {
    let types: Vec<Value> = vec![
        serde_json::to_value(schema_for!(Capability)).unwrap(),
        serde_json::to_value(schema_for!(SupportLevel)).unwrap(),
        serde_json::to_value(schema_for!(Outcome)).unwrap(),
        serde_json::to_value(schema_for!(ExecutionMode)).unwrap(),
        serde_json::to_value(schema_for!(PolicyProfile)).unwrap(),
        serde_json::to_value(schema_for!(RuntimeConfig)).unwrap(),
        serde_json::to_value(schema_for!(WorkspaceSpec)).unwrap(),
        serde_json::to_value(schema_for!(UsageNormalized)).unwrap(),
    ];
    for (i, s) in types.iter().enumerate() {
        jsonschema::validator_for(s)
            .unwrap_or_else(|e| panic!("standalone schema {i} failed to compile: {e}"));
    }
}

// ── 15. Schema diff detection ────────────────────────────────────────

#[test]
fn schema_detects_missing_required_field() {
    let schema = wo_schema();
    let mut instance = valid_wo();
    instance.as_object_mut().unwrap().remove("id");
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_detects_wrong_type_for_task() {
    let schema = wo_schema();
    let mut instance = valid_wo();
    instance["task"] = json!(42);
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_detects_invalid_lane_value() {
    let schema = wo_schema();
    let mut instance = valid_wo();
    instance["lane"] = json!("nonexistent_lane");
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_detects_invalid_outcome_in_receipt() {
    let schema = receipt_schema();
    let mut instance = valid_receipt();
    instance["outcome"] = json!("unknown_outcome");
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_rejects_extra_required_fields_missing() {
    let schema = receipt_schema();
    let mut instance = valid_receipt();
    instance.as_object_mut().unwrap().remove("meta");
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_has_uuid_format_annotation_for_id() {
    // jsonschema crate doesn't enforce `format: uuid` by default,
    // but the schema itself should declare the format
    let schema = wo_schema();
    assert_eq!(schema["properties"]["id"]["format"], "uuid");
    // A non-uuid string should still be a string but the format annotation is advisory
    let mut instance = valid_wo();
    instance["id"] = json!("not-a-uuid");
    // This may or may not fail depending on validator config, so just check the schema declares it
}

#[test]
fn schema_rejects_negative_duration() {
    let schema = receipt_schema();
    let mut instance = valid_receipt();
    instance["meta"]["duration_ms"] = json!(-1);
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_accepts_receipt_with_artifacts() {
    let schema = receipt_schema();
    let mut instance = valid_receipt();
    instance["artifacts"] = json!([{"kind": "patch", "path": "output.patch"}]);
    assert_valid(&schema, &instance);
}

#[test]
fn schema_rejects_artifact_missing_fields() {
    let schema = receipt_schema();
    let mut instance = valid_receipt();
    instance["artifacts"] = json!([{"kind": "patch"}]); // missing path
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_accepts_work_order_with_context_snippets() {
    let schema = wo_schema();
    let wo = WorkOrderBuilder::new("test")
        .context(ContextPacket {
            files: vec!["README.md".into()],
            snippets: vec![ContextSnippet {
                name: "hint".into(),
                content: "be careful".into(),
            }],
        })
        .build();
    let val = serde_json::to_value(&wo).unwrap();
    assert_valid(&schema, &val);
}

#[test]
fn schema_accepts_work_order_with_capability_requirements() {
    let schema = wo_schema();
    let wo = WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let val = serde_json::to_value(&wo).unwrap();
    assert_valid(&schema, &val);
}

#[test]
fn schema_accepts_work_order_with_runtime_config() {
    let schema = wo_schema();
    let wo = WorkOrderBuilder::new("test")
        .model("gpt-4")
        .max_turns(5)
        .max_budget_usd(1.0)
        .build();
    let val = serde_json::to_value(&wo).unwrap();
    assert_valid(&schema, &val);
}

#[test]
fn schema_rejects_empty_object_as_work_order() {
    let schema = wo_schema();
    assert_invalid(&schema, &json!({}));
}

#[test]
fn schema_rejects_empty_object_as_receipt() {
    let schema = receipt_schema();
    assert_invalid(&schema, &json!({}));
}

#[test]
fn schema_rejects_null_as_work_order() {
    let schema = wo_schema();
    assert_invalid(&schema, &Value::Null);
}

#[test]
fn schema_rejects_array_as_receipt() {
    let schema = receipt_schema();
    assert_invalid(&schema, &json!([]));
}

#[test]
fn schema_detects_workspace_mode_invalid() {
    let schema = wo_schema();
    let mut instance = valid_wo();
    instance["workspace"]["mode"] = json!("invalid_mode");
    assert_invalid(&schema, &instance);
}

#[test]
fn schema_accepts_event_with_ext() {
    let schema = event_schema();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(BTreeMap::from([(
            "raw_message".into(),
            json!({"role": "assistant"}),
        )])),
    };
    let val = serde_json::to_value(&event).unwrap();
    assert_valid(&schema, &val);
}

#[test]
fn schema_accepts_receipt_with_hash() {
    let schema = receipt_schema();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    let val = serde_json::to_value(&receipt).unwrap();
    assert_valid(&schema, &val);
    assert!(val["receipt_sha256"].is_string());
}

#[test]
fn schema_accepts_all_execution_modes() {
    let schema = receipt_schema();
    for mode_str in &["passthrough", "mapped"] {
        let mut r = valid_receipt();
        r["mode"] = json!(mode_str);
        assert_valid(&schema, &r);
    }
}

#[test]
fn schema_field_count_work_order() {
    let props = get_properties(&wo_schema());
    assert_eq!(props.len(), 8, "WorkOrder should have exactly 8 properties");
}

#[test]
fn schema_field_count_receipt() {
    let props = get_properties(&receipt_schema());
    // meta, backend, capabilities, mode, usage_raw, usage, trace, artifacts,
    // verification, outcome, receipt_sha256 = 11
    assert_eq!(props.len(), 11, "Receipt should have exactly 11 properties");
}

#[test]
fn schemas_contain_no_deprecated_fields() {
    // Ensure no property named "deprecated" or "obsolete" exists
    let check = |schema: &Value, name: &str| {
        if let Some(props) = schema["properties"].as_object() {
            for key in props.keys() {
                assert!(
                    !key.contains("deprecated") && !key.contains("obsolete"),
                    "{name} contains suspicious field: {key}"
                );
            }
        }
    };
    check(&wo_schema(), "WorkOrder");
    check(&receipt_schema(), "Receipt");
}

#[test]
fn work_order_schema_idempotent() {
    let s1 = wo_schema();
    let s2 = wo_schema();
    let s3 = wo_schema();
    assert_eq!(s1, s2);
    assert_eq!(s2, s3);
}

#[test]
fn receipt_schema_idempotent() {
    let s1 = receipt_schema();
    let s2 = receipt_schema();
    let s3 = receipt_schema();
    assert_eq!(s1, s2);
    assert_eq!(s2, s3);
}
