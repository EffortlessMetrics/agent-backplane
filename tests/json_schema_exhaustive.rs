#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive JSON schema validation tests.
//!
//! Ensures all generated schemas are valid, complete, contain expected
//! properties, mark required fields, handle enums correctly, reference
//! nested types, and remain stable across calls.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityRequirement,
    CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionLane, ExecutionMode,
    MinSupport, Outcome, PolicyProfile, Receipt, ReceiptBuilder, RunMetadata, RuntimeConfig,
    SupportLevel, UsageNormalized, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_error::ErrorCode;
use abp_policy::Decision;
use abp_policy::composed::{ComposedResult, CompositionStrategy};
use abp_policy::rate_limit::{RateLimitPolicy, RateLimitResult};
use chrono::Utc;
use schemars::schema_for;
use serde_json::{Value, json};
use std::collections::BTreeMap;

// ── helpers ──────────────────────────────────────────────────────────────

fn schema_value<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap()
}

fn assert_schema_compiles(schema: &Value) {
    jsonschema::validator_for(schema).expect("schema must compile");
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

fn get_title(schema: &Value) -> Option<&str> {
    schema.get("title").and_then(Value::as_str)
}

/// Extract enum variant string values from a schema.
/// schemars v1 may use either `"enum": [...]` or `"oneOf": [{"const": ...}, ...]`.
fn get_enum_values(schema: &Value) -> Vec<String> {
    if let Some(arr) = schema.get("enum").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    if let Some(arr) = schema.get("oneOf").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(|v| v.get("const").and_then(Value::as_str).map(String::from))
            .collect();
    }
    vec![]
}

/// Check if a schema represents a string-like enum (either via enum or oneOf const).
fn is_string_enum_schema(schema: &Value) -> bool {
    schema.get("enum").is_some()
        || schema
            .get("oneOf")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().any(|v| v.get("const").is_some()))
            .unwrap_or(false)
}

/// Get the effective type, handling oneOf const patterns.
fn get_effective_type(schema: &Value) -> Option<&str> {
    if let Some(t) = schema.get("type").and_then(Value::as_str) {
        return Some(t);
    }
    // oneOf with const string values → effectively string
    if is_string_enum_schema(schema) {
        return Some("string");
    }
    None
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
// 1. All major types produce valid JSON schemas
// =========================================================================

#[test]
fn work_order_schema_compiles() {
    assert_schema_compiles(&schema_value::<WorkOrder>());
}

#[test]
fn receipt_schema_compiles() {
    assert_schema_compiles(&schema_value::<Receipt>());
}

#[test]
fn agent_event_schema_compiles() {
    assert_schema_compiles(&schema_value::<AgentEvent>());
}

#[test]
fn agent_event_kind_schema_compiles() {
    assert_schema_compiles(&schema_value::<AgentEventKind>());
}

#[test]
fn capability_schema_compiles() {
    assert_schema_compiles(&schema_value::<Capability>());
}

#[test]
fn policy_profile_schema_compiles() {
    assert_schema_compiles(&schema_value::<PolicyProfile>());
}

#[test]
fn execution_lane_schema_compiles() {
    assert_schema_compiles(&schema_value::<ExecutionLane>());
}

#[test]
fn execution_mode_schema_compiles() {
    assert_schema_compiles(&schema_value::<ExecutionMode>());
}

#[test]
fn workspace_spec_schema_compiles() {
    assert_schema_compiles(&schema_value::<WorkspaceSpec>());
}

#[test]
fn workspace_mode_schema_compiles() {
    assert_schema_compiles(&schema_value::<WorkspaceMode>());
}

#[test]
fn context_packet_schema_compiles() {
    assert_schema_compiles(&schema_value::<ContextPacket>());
}

#[test]
fn context_snippet_schema_compiles() {
    assert_schema_compiles(&schema_value::<ContextSnippet>());
}

#[test]
fn runtime_config_schema_compiles() {
    assert_schema_compiles(&schema_value::<RuntimeConfig>());
}

#[test]
fn capability_requirements_schema_compiles() {
    assert_schema_compiles(&schema_value::<CapabilityRequirements>());
}

#[test]
fn capability_requirement_schema_compiles() {
    assert_schema_compiles(&schema_value::<CapabilityRequirement>());
}

#[test]
fn min_support_schema_compiles() {
    assert_schema_compiles(&schema_value::<MinSupport>());
}

#[test]
fn support_level_schema_compiles() {
    assert_schema_compiles(&schema_value::<SupportLevel>());
}

#[test]
fn backend_identity_schema_compiles() {
    assert_schema_compiles(&schema_value::<BackendIdentity>());
}

#[test]
fn run_metadata_schema_compiles() {
    assert_schema_compiles(&schema_value::<RunMetadata>());
}

#[test]
fn usage_normalized_schema_compiles() {
    assert_schema_compiles(&schema_value::<UsageNormalized>());
}

#[test]
fn outcome_schema_compiles() {
    assert_schema_compiles(&schema_value::<Outcome>());
}

#[test]
fn artifact_ref_schema_compiles() {
    assert_schema_compiles(&schema_value::<ArtifactRef>());
}

#[test]
fn verification_report_schema_compiles() {
    assert_schema_compiles(&schema_value::<VerificationReport>());
}

#[test]
fn decision_schema_compiles() {
    assert_schema_compiles(&schema_value::<Decision>());
}

#[test]
fn rate_limit_policy_schema_compiles() {
    assert_schema_compiles(&schema_value::<RateLimitPolicy>());
}

#[test]
fn rate_limit_result_schema_compiles() {
    assert_schema_compiles(&schema_value::<RateLimitResult>());
}

#[test]
fn composition_strategy_schema_compiles() {
    assert_schema_compiles(&schema_value::<CompositionStrategy>());
}

#[test]
fn composed_result_schema_compiles() {
    assert_schema_compiles(&schema_value::<ComposedResult>());
}

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
fn ir_tool_definition_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrToolDefinition>());
}

#[test]
fn ir_conversation_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrConversation>());
}

#[test]
fn ir_usage_schema_compiles() {
    assert_schema_compiles(&schema_value::<IrUsage>());
}

#[test]
fn error_code_schema_compiles() {
    assert_schema_compiles(&schema_value::<ErrorCode>());
}

// =========================================================================
// 2. WorkOrder schema contains expected properties
// =========================================================================

#[test]
fn work_order_has_id_property() {
    let s = schema_value::<WorkOrder>();
    let props = get_properties(&s);
    assert!(props.contains(&"id".to_string()), "missing id: {props:?}");
}

#[test]
fn work_order_has_task_property() {
    let s = schema_value::<WorkOrder>();
    assert!(get_properties(&s).contains(&"task".to_string()));
}

#[test]
fn work_order_has_lane_property() {
    let s = schema_value::<WorkOrder>();
    assert!(get_properties(&s).contains(&"lane".to_string()));
}

#[test]
fn work_order_has_workspace_property() {
    let s = schema_value::<WorkOrder>();
    assert!(get_properties(&s).contains(&"workspace".to_string()));
}

#[test]
fn work_order_has_context_property() {
    let s = schema_value::<WorkOrder>();
    assert!(get_properties(&s).contains(&"context".to_string()));
}

#[test]
fn work_order_has_policy_property() {
    let s = schema_value::<WorkOrder>();
    assert!(get_properties(&s).contains(&"policy".to_string()));
}

#[test]
fn work_order_has_requirements_property() {
    let s = schema_value::<WorkOrder>();
    assert!(get_properties(&s).contains(&"requirements".to_string()));
}

#[test]
fn work_order_has_config_property() {
    let s = schema_value::<WorkOrder>();
    assert!(get_properties(&s).contains(&"config".to_string()));
}

#[test]
fn work_order_has_exactly_eight_properties() {
    let s = schema_value::<WorkOrder>();
    assert_eq!(get_properties(&s).len(), 8);
}

// =========================================================================
// 3. Receipt schema contains expected properties
// =========================================================================

#[test]
fn receipt_has_meta_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"meta".to_string()));
}

#[test]
fn receipt_has_backend_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"backend".to_string()));
}

#[test]
fn receipt_has_capabilities_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"capabilities".to_string()));
}

#[test]
fn receipt_has_mode_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"mode".to_string()));
}

#[test]
fn receipt_has_usage_raw_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"usage_raw".to_string()));
}

#[test]
fn receipt_has_usage_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"usage".to_string()));
}

#[test]
fn receipt_has_trace_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"trace".to_string()));
}

#[test]
fn receipt_has_artifacts_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"artifacts".to_string()));
}

#[test]
fn receipt_has_verification_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"verification".to_string()));
}

#[test]
fn receipt_has_outcome_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"outcome".to_string()));
}

#[test]
fn receipt_has_receipt_sha256_property() {
    let s = schema_value::<Receipt>();
    assert!(get_properties(&s).contains(&"receipt_sha256".to_string()));
}

// =========================================================================
// 4. Required fields are marked required
// =========================================================================

#[test]
fn work_order_all_fields_required() {
    let s = schema_value::<WorkOrder>();
    let req = get_required(&s);
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
            "WorkOrder missing required field: {field}"
        );
    }
}

#[test]
fn receipt_required_fields() {
    let s = schema_value::<Receipt>();
    let req = get_required(&s);
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
            "Receipt missing required field: {field}"
        );
    }
}

#[test]
fn receipt_sha256_not_required() {
    let s = schema_value::<Receipt>();
    let req = get_required(&s);
    assert!(
        !req.contains(&"receipt_sha256".to_string()),
        "receipt_sha256 should be optional"
    );
}

#[test]
fn run_metadata_all_fields_required() {
    let s = schema_value::<RunMetadata>();
    let req = get_required(&s);
    for field in &[
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(
            req.contains(&field.to_string()),
            "RunMetadata missing required: {field}"
        );
    }
}

#[test]
fn usage_normalized_no_required_fields() {
    let s = schema_value::<UsageNormalized>();
    let req = get_required(&s);
    assert!(
        req.is_empty(),
        "UsageNormalized should have no required fields, got: {req:?}"
    );
}

#[test]
fn policy_profile_all_fields_required() {
    let s = schema_value::<PolicyProfile>();
    let req = get_required(&s);
    for field in &[
        "allowed_tools",
        "disallowed_tools",
        "deny_read",
        "deny_write",
        "allow_network",
        "deny_network",
        "require_approval_for",
    ] {
        assert!(
            req.contains(&field.to_string()),
            "PolicyProfile missing required: {field}"
        );
    }
}

#[test]
fn workspace_spec_all_fields_required() {
    let s = schema_value::<WorkspaceSpec>();
    let req = get_required(&s);
    for field in &["root", "mode", "include", "exclude"] {
        assert!(
            req.contains(&field.to_string()),
            "WorkspaceSpec missing required: {field}"
        );
    }
}

#[test]
fn runtime_config_optional_fields() {
    let s = schema_value::<RuntimeConfig>();
    let req = get_required(&s);
    assert!(
        !req.contains(&"model".to_string()),
        "model should be optional"
    );
    assert!(
        !req.contains(&"max_budget_usd".to_string()),
        "max_budget_usd should be optional"
    );
    assert!(
        !req.contains(&"max_turns".to_string()),
        "max_turns should be optional"
    );
}

#[test]
fn runtime_config_vendor_and_env_required() {
    let s = schema_value::<RuntimeConfig>();
    let req = get_required(&s);
    assert!(req.contains(&"vendor".to_string()));
    assert!(req.contains(&"env".to_string()));
}

#[test]
fn backend_identity_required_fields() {
    let s = schema_value::<BackendIdentity>();
    let req = get_required(&s);
    assert!(req.contains(&"id".to_string()));
    assert!(!req.contains(&"backend_version".to_string()));
    assert!(!req.contains(&"adapter_version".to_string()));
}

#[test]
fn context_packet_required_fields() {
    let s = schema_value::<ContextPacket>();
    let req = get_required(&s);
    assert!(req.contains(&"files".to_string()));
    assert!(req.contains(&"snippets".to_string()));
}

#[test]
fn context_snippet_required_fields() {
    let s = schema_value::<ContextSnippet>();
    let req = get_required(&s);
    assert!(req.contains(&"name".to_string()));
    assert!(req.contains(&"content".to_string()));
}

#[test]
fn artifact_ref_required_fields() {
    let s = schema_value::<ArtifactRef>();
    let req = get_required(&s);
    assert!(req.contains(&"kind".to_string()));
    assert!(req.contains(&"path".to_string()));
}

#[test]
fn verification_report_required_fields() {
    let s = schema_value::<VerificationReport>();
    let req = get_required(&s);
    assert!(req.contains(&"harness_ok".to_string()));
    assert!(!req.contains(&"git_diff".to_string()));
    assert!(!req.contains(&"git_status".to_string()));
}

#[test]
fn decision_required_fields() {
    let s = schema_value::<Decision>();
    let req = get_required(&s);
    assert!(req.contains(&"allowed".to_string()));
    assert!(!req.contains(&"reason".to_string()));
}

#[test]
fn rate_limit_policy_no_required_fields() {
    let s = schema_value::<RateLimitPolicy>();
    let req = get_required(&s);
    assert!(
        req.is_empty(),
        "RateLimitPolicy should have no required fields"
    );
}

#[test]
fn capability_requirement_required_fields() {
    let s = schema_value::<CapabilityRequirement>();
    let req = get_required(&s);
    assert!(req.contains(&"capability".to_string()));
    assert!(req.contains(&"min_support".to_string()));
}

#[test]
fn ir_message_required_fields() {
    let s = schema_value::<IrMessage>();
    let req = get_required(&s);
    assert!(req.contains(&"role".to_string()));
    assert!(req.contains(&"content".to_string()));
}

#[test]
fn ir_tool_definition_required_fields() {
    let s = schema_value::<IrToolDefinition>();
    let req = get_required(&s);
    assert!(req.contains(&"name".to_string()));
    assert!(req.contains(&"description".to_string()));
    assert!(req.contains(&"parameters".to_string()));
}

#[test]
fn ir_usage_all_fields_required() {
    let s = schema_value::<IrUsage>();
    let req = get_required(&s);
    for field in &[
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
    ] {
        assert!(
            req.contains(&field.to_string()),
            "IrUsage missing required: {field}"
        );
    }
}

// =========================================================================
// 5. Enum schemas use oneOf/enum correctly
// =========================================================================

#[test]
fn execution_lane_is_string_enum() {
    let s = schema_value::<ExecutionLane>();
    let values = get_enum_values(&s);
    assert!(values.contains(&"patch_first".to_string()));
    assert!(values.contains(&"workspace_first".to_string()));
}

#[test]
fn workspace_mode_is_string_enum() {
    let s = schema_value::<WorkspaceMode>();
    let values = get_enum_values(&s);
    assert!(values.contains(&"pass_through".to_string()));
    assert!(values.contains(&"staged".to_string()));
}

#[test]
fn outcome_is_string_enum() {
    let s = schema_value::<Outcome>();
    let values = get_enum_values(&s);
    assert!(values.contains(&"complete".to_string()));
    assert!(values.contains(&"partial".to_string()));
    assert!(values.contains(&"failed".to_string()));
}

#[test]
fn execution_mode_is_string_enum() {
    let s = schema_value::<ExecutionMode>();
    let values = get_enum_values(&s);
    assert!(values.contains(&"passthrough".to_string()));
    assert!(values.contains(&"mapped".to_string()));
}

#[test]
fn min_support_is_string_enum() {
    let s = schema_value::<MinSupport>();
    let values = get_enum_values(&s);
    assert!(values.contains(&"native".to_string()));
    assert!(values.contains(&"emulated".to_string()));
}

#[test]
fn ir_role_is_string_enum() {
    let s = schema_value::<IrRole>();
    let values = get_enum_values(&s);
    assert!(values.contains(&"system".to_string()));
    assert!(values.contains(&"user".to_string()));
    assert!(values.contains(&"assistant".to_string()));
    assert!(values.contains(&"tool".to_string()));
}

#[test]
fn composition_strategy_is_string_enum() {
    let s = schema_value::<CompositionStrategy>();
    let values = get_enum_values(&s);
    assert!(values.contains(&"all_must_allow".to_string()));
    assert!(values.contains(&"any_must_allow".to_string()));
    assert!(values.contains(&"first_match".to_string()));
}

#[test]
fn capability_enum_has_all_variants() {
    let s = schema_value::<Capability>();
    let values = get_enum_values(&s);
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
            values.contains(&v.to_string()),
            "Capability missing variant: {v}"
        );
    }
    assert_eq!(values.len(), expected.len());
}

#[test]
fn agent_event_kind_uses_one_of() {
    let s = schema_value::<AgentEventKind>();
    assert!(
        s.get("oneOf").is_some(),
        "AgentEventKind should use oneOf for tagged enum"
    );
}

#[test]
fn support_level_uses_one_of() {
    let s = schema_value::<SupportLevel>();
    assert!(
        s.get("oneOf").is_some(),
        "SupportLevel should use oneOf (has Restricted variant with data)"
    );
}

#[test]
fn ir_content_block_uses_one_of() {
    let s = schema_value::<IrContentBlock>();
    assert!(
        s.get("oneOf").is_some(),
        "IrContentBlock should use oneOf for tagged enum"
    );
}

#[test]
fn rate_limit_result_uses_one_of() {
    let s = schema_value::<RateLimitResult>();
    assert!(
        s.get("oneOf").is_some(),
        "RateLimitResult should use oneOf for tagged enum"
    );
}

#[test]
fn composed_result_uses_one_of() {
    let s = schema_value::<ComposedResult>();
    assert!(
        s.get("oneOf").is_some(),
        "ComposedResult should use oneOf for tagged enum"
    );
}

#[test]
fn agent_event_kind_one_of_count() {
    let s = schema_value::<AgentEventKind>();
    let variants = s["oneOf"].as_array().expect("should have oneOf");
    assert_eq!(variants.len(), 10, "AgentEventKind should have 10 variants");
}

#[test]
fn ir_content_block_one_of_count() {
    let s = schema_value::<IrContentBlock>();
    let variants = s["oneOf"].as_array().expect("should have oneOf");
    assert_eq!(variants.len(), 5, "IrContentBlock should have 5 variants");
}

// =========================================================================
// 6. Nested types are properly referenced ($defs)
// =========================================================================

#[test]
fn work_order_schema_has_defs() {
    let s = schema_value::<WorkOrder>();
    let defs = get_defs(&s);
    assert!(
        !defs.is_empty(),
        "WorkOrder should have $defs for nested types"
    );
}

#[test]
fn work_order_references_execution_lane() {
    let s = schema_value::<WorkOrder>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"ExecutionLane".to_string()),
        "WorkOrder $defs should contain ExecutionLane: {defs:?}"
    );
}

#[test]
fn work_order_references_workspace_spec() {
    let s = schema_value::<WorkOrder>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"WorkspaceSpec".to_string()),
        "WorkOrder $defs should contain WorkspaceSpec: {defs:?}"
    );
}

#[test]
fn work_order_references_context_packet() {
    let s = schema_value::<WorkOrder>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"ContextPacket".to_string()),
        "WorkOrder $defs should contain ContextPacket: {defs:?}"
    );
}

#[test]
fn work_order_references_policy_profile() {
    let s = schema_value::<WorkOrder>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"PolicyProfile".to_string()),
        "WorkOrder $defs should contain PolicyProfile: {defs:?}"
    );
}

#[test]
fn work_order_references_runtime_config() {
    let s = schema_value::<WorkOrder>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"RuntimeConfig".to_string()),
        "WorkOrder $defs should contain RuntimeConfig: {defs:?}"
    );
}

#[test]
fn receipt_schema_has_defs() {
    let s = schema_value::<Receipt>();
    let defs = get_defs(&s);
    assert!(
        !defs.is_empty(),
        "Receipt should have $defs for nested types"
    );
}

#[test]
fn receipt_references_run_metadata() {
    let s = schema_value::<Receipt>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"RunMetadata".to_string()),
        "Receipt $defs should contain RunMetadata: {defs:?}"
    );
}

#[test]
fn receipt_references_backend_identity() {
    let s = schema_value::<Receipt>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"BackendIdentity".to_string()),
        "Receipt $defs should contain BackendIdentity: {defs:?}"
    );
}

#[test]
fn receipt_references_outcome() {
    let s = schema_value::<Receipt>();
    let defs = get_defs(&s);
    assert!(
        defs.contains(&"Outcome".to_string()),
        "Receipt $defs should contain Outcome: {defs:?}"
    );
}

// =========================================================================
// 7. BTreeMap fields produce object schemas
// =========================================================================

#[test]
fn runtime_config_vendor_is_object_type() {
    let s = schema_value::<RuntimeConfig>();
    let vendor = &s["properties"]["vendor"];
    assert_eq!(
        vendor["type"].as_str(),
        Some("object"),
        "vendor BTreeMap should be object"
    );
}

#[test]
fn runtime_config_env_is_object_type() {
    let s = schema_value::<RuntimeConfig>();
    let env = &s["properties"]["env"];
    assert_eq!(
        env["type"].as_str(),
        Some("object"),
        "env BTreeMap should be object"
    );
}

#[test]
fn runtime_config_env_additional_properties_string() {
    let s = schema_value::<RuntimeConfig>();
    let env = &s["properties"]["env"];
    let ap = &env["additionalProperties"];
    assert_eq!(
        ap["type"].as_str(),
        Some("string"),
        "env BTreeMap<String, String> should have string additionalProperties"
    );
}

#[test]
fn receipt_capabilities_is_object_type() {
    let s = schema_value::<Receipt>();
    let caps = &s["properties"]["capabilities"];
    assert_eq!(
        caps["type"].as_str(),
        Some("object"),
        "capabilities BTreeMap should be object"
    );
}

// =========================================================================
// 8. Schema stability (same type → same schema across calls)
// =========================================================================

#[test]
fn work_order_schema_is_stable() {
    let a = schema_value::<WorkOrder>();
    let b = schema_value::<WorkOrder>();
    assert_eq!(a, b, "WorkOrder schema should be deterministic");
}

#[test]
fn receipt_schema_is_stable() {
    let a = schema_value::<Receipt>();
    let b = schema_value::<Receipt>();
    assert_eq!(a, b, "Receipt schema should be deterministic");
}

#[test]
fn agent_event_schema_is_stable() {
    let a = schema_value::<AgentEvent>();
    let b = schema_value::<AgentEvent>();
    assert_eq!(a, b, "AgentEvent schema should be deterministic");
}

#[test]
fn capability_schema_is_stable() {
    let a = schema_value::<Capability>();
    let b = schema_value::<Capability>();
    assert_eq!(a, b, "Capability schema should be deterministic");
}

#[test]
fn policy_profile_schema_is_stable() {
    let a = schema_value::<PolicyProfile>();
    let b = schema_value::<PolicyProfile>();
    assert_eq!(a, b, "PolicyProfile schema should be deterministic");
}

// =========================================================================
// 9. serde rename_all reflected in schema property/enum names
// =========================================================================

#[test]
fn execution_lane_uses_snake_case() {
    let s = schema_value::<ExecutionLane>();
    let values = get_enum_values(&s);
    for name in &values {
        assert_eq!(
            name.as_str(),
            name.to_lowercase().as_str(),
            "ExecutionLane variant should be snake_case: {name}"
        );
    }
}

#[test]
fn outcome_uses_snake_case() {
    let s = schema_value::<Outcome>();
    let values = get_enum_values(&s);
    for name in &values {
        assert!(
            !name.chars().any(|c| c.is_uppercase()),
            "Outcome variant should be snake_case: {name}"
        );
    }
}

#[test]
fn capability_variants_are_snake_case() {
    let s = schema_value::<Capability>();
    let values = get_enum_values(&s);
    for name in &values {
        assert!(
            !name.chars().any(|c| c.is_uppercase()),
            "Capability variant should be snake_case: {name}"
        );
    }
}

#[test]
fn agent_event_kind_variants_use_snake_case_type_tag() {
    let s = schema_value::<AgentEventKind>();
    let variants = s["oneOf"].as_array().unwrap();
    for variant in variants {
        let props = variant["properties"].as_object();
        if let Some(p) = props {
            if let Some(type_prop) = p.get("type") {
                if let Some(c) = type_prop.get("const") {
                    let tag = c.as_str().unwrap();
                    assert!(
                        !tag.chars().any(|ch| ch.is_uppercase()),
                        "AgentEventKind type tag should be snake_case: {tag}"
                    );
                }
            }
        }
    }
}

#[test]
fn composition_strategy_uses_snake_case() {
    let s = schema_value::<CompositionStrategy>();
    let values = get_enum_values(&s);
    for name in &values {
        assert!(
            !name.chars().any(|c| c.is_uppercase()),
            "CompositionStrategy variant should be snake_case: {name}"
        );
    }
}

// =========================================================================
// 10. Schema validates known-good instances
// =========================================================================

#[test]
fn work_order_schema_accepts_valid_instance() {
    let s = schema_value::<WorkOrder>();
    let v = valid_wo_value();
    assert_valid(&s, &v);
}

#[test]
fn receipt_schema_accepts_valid_instance() {
    let s = schema_value::<Receipt>();
    let v = valid_receipt_value();
    assert_valid(&s, &v);
}

#[test]
fn agent_event_schema_accepts_assistant_message() {
    let s = schema_value::<AgentEvent>();
    let v = valid_event_value();
    assert_valid(&s, &v);
}

#[test]
fn policy_profile_schema_accepts_default() {
    let s = schema_value::<PolicyProfile>();
    let v = serde_json::to_value(PolicyProfile::default()).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn runtime_config_schema_accepts_default() {
    let s = schema_value::<RuntimeConfig>();
    let v = serde_json::to_value(RuntimeConfig::default()).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn usage_normalized_schema_accepts_default() {
    let s = schema_value::<UsageNormalized>();
    let v = serde_json::to_value(UsageNormalized::default()).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn verification_report_schema_accepts_default() {
    let s = schema_value::<VerificationReport>();
    let v = serde_json::to_value(VerificationReport::default()).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn context_packet_schema_accepts_default() {
    let s = schema_value::<ContextPacket>();
    let v = serde_json::to_value(ContextPacket::default()).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn capability_schema_accepts_streaming() {
    let s = schema_value::<Capability>();
    let v = serde_json::to_value(Capability::Streaming).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn outcome_schema_accepts_complete() {
    let s = schema_value::<Outcome>();
    let v = serde_json::to_value(Outcome::Complete).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn execution_mode_schema_accepts_mapped() {
    let s = schema_value::<ExecutionMode>();
    let v = serde_json::to_value(ExecutionMode::Mapped).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn ir_role_schema_accepts_system() {
    let s = schema_value::<IrRole>();
    let v = serde_json::to_value(IrRole::System).unwrap();
    assert_valid(&s, &v);
}

// =========================================================================
// 11. Schema rejects known-bad instances
// =========================================================================

#[test]
fn work_order_schema_rejects_empty_object() {
    let s = schema_value::<WorkOrder>();
    assert_invalid(&s, &json!({}));
}

#[test]
fn receipt_schema_rejects_empty_object() {
    let s = schema_value::<Receipt>();
    assert_invalid(&s, &json!({}));
}

#[test]
fn work_order_schema_rejects_missing_task() {
    let s = schema_value::<WorkOrder>();
    let mut v = valid_wo_value();
    v.as_object_mut().unwrap().remove("task");
    assert_invalid(&s, &v);
}

#[test]
fn receipt_schema_rejects_missing_outcome() {
    let s = schema_value::<Receipt>();
    let mut v = valid_receipt_value();
    v.as_object_mut().unwrap().remove("outcome");
    assert_invalid(&s, &v);
}

#[test]
fn outcome_schema_rejects_invalid_string() {
    let s = schema_value::<Outcome>();
    assert_invalid(&s, &json!("invalid_outcome"));
}

#[test]
fn execution_lane_schema_rejects_invalid_string() {
    let s = schema_value::<ExecutionLane>();
    assert_invalid(&s, &json!("not_a_lane"));
}

#[test]
fn capability_schema_rejects_invalid_string() {
    let s = schema_value::<Capability>();
    assert_invalid(&s, &json!("not_a_capability"));
}

#[test]
fn work_order_schema_rejects_number() {
    let s = schema_value::<WorkOrder>();
    assert_invalid(&s, &json!(42));
}

#[test]
fn policy_profile_schema_rejects_missing_allowed_tools() {
    let s = schema_value::<PolicyProfile>();
    let mut v = serde_json::to_value(PolicyProfile::default()).unwrap();
    v.as_object_mut().unwrap().remove("allowed_tools");
    assert_invalid(&s, &v);
}

// =========================================================================
// 12. Schema titles
// =========================================================================

#[test]
fn work_order_schema_has_title() {
    let s = schema_value::<WorkOrder>();
    assert_eq!(get_title(&s), Some("WorkOrder"));
}

#[test]
fn receipt_schema_has_title() {
    let s = schema_value::<Receipt>();
    assert_eq!(get_title(&s), Some("Receipt"));
}

#[test]
fn agent_event_schema_has_title() {
    let s = schema_value::<AgentEvent>();
    assert_eq!(get_title(&s), Some("AgentEvent"));
}

#[test]
fn capability_schema_has_title() {
    let s = schema_value::<Capability>();
    assert_eq!(get_title(&s), Some("Capability"));
}

#[test]
fn policy_profile_schema_has_title() {
    let s = schema_value::<PolicyProfile>();
    assert_eq!(get_title(&s), Some("PolicyProfile"));
}

// =========================================================================
// 13. Additional property-level checks
// =========================================================================

#[test]
fn usage_normalized_has_all_fields() {
    let s = schema_value::<UsageNormalized>();
    let props = get_properties(&s);
    for field in &[
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "request_units",
        "estimated_cost_usd",
    ] {
        assert!(
            props.contains(&field.to_string()),
            "UsageNormalized missing: {field}"
        );
    }
}

#[test]
fn policy_profile_has_all_seven_fields() {
    let s = schema_value::<PolicyProfile>();
    let props = get_properties(&s);
    assert_eq!(props.len(), 7);
}

#[test]
fn backend_identity_has_three_fields() {
    let s = schema_value::<BackendIdentity>();
    let props = get_properties(&s);
    assert_eq!(props.len(), 3);
    assert!(props.contains(&"id".to_string()));
    assert!(props.contains(&"backend_version".to_string()));
    assert!(props.contains(&"adapter_version".to_string()));
}

#[test]
fn rate_limit_policy_has_three_fields() {
    let s = schema_value::<RateLimitPolicy>();
    let props = get_properties(&s);
    assert_eq!(props.len(), 3);
    assert!(props.contains(&"max_requests_per_minute".to_string()));
    assert!(props.contains(&"max_tokens_per_minute".to_string()));
    assert!(props.contains(&"max_concurrent".to_string()));
}

#[test]
fn ir_conversation_has_messages_field() {
    let s = schema_value::<IrConversation>();
    let props = get_properties(&s);
    assert!(props.contains(&"messages".to_string()));
}

#[test]
fn ir_message_has_metadata_field() {
    let s = schema_value::<IrMessage>();
    let props = get_properties(&s);
    assert!(props.contains(&"metadata".to_string()));
}

#[test]
fn ir_usage_has_five_fields() {
    let s = schema_value::<IrUsage>();
    let props = get_properties(&s);
    assert_eq!(props.len(), 5);
}

// =========================================================================
// 14. Schema type field checks
// =========================================================================

#[test]
fn work_order_schema_type_is_object() {
    let s = schema_value::<WorkOrder>();
    assert_eq!(s["type"].as_str(), Some("object"));
}

#[test]
fn receipt_schema_type_is_object() {
    let s = schema_value::<Receipt>();
    assert_eq!(s["type"].as_str(), Some("object"));
}

#[test]
fn policy_profile_schema_type_is_object() {
    let s = schema_value::<PolicyProfile>();
    assert_eq!(s["type"].as_str(), Some("object"));
}

#[test]
fn runtime_config_schema_type_is_object() {
    let s = schema_value::<RuntimeConfig>();
    assert_eq!(s["type"].as_str(), Some("object"));
}

#[test]
fn capability_schema_type_is_string() {
    let s = schema_value::<Capability>();
    assert_eq!(get_effective_type(&s), Some("string"));
}

#[test]
fn outcome_schema_type_is_string() {
    let s = schema_value::<Outcome>();
    assert_eq!(get_effective_type(&s), Some("string"));
}

#[test]
fn execution_lane_schema_type_is_string() {
    let s = schema_value::<ExecutionLane>();
    assert_eq!(get_effective_type(&s), Some("string"));
}

#[test]
fn ir_role_schema_type_is_string() {
    let s = schema_value::<IrRole>();
    assert_eq!(get_effective_type(&s), Some("string"));
}

// =========================================================================
// 15. Array field checks
// =========================================================================

#[test]
fn policy_profile_allowed_tools_is_array() {
    let s = schema_value::<PolicyProfile>();
    assert_eq!(
        s["properties"]["allowed_tools"]["type"].as_str(),
        Some("array")
    );
}

#[test]
fn receipt_trace_is_array() {
    let s = schema_value::<Receipt>();
    assert_eq!(s["properties"]["trace"]["type"].as_str(), Some("array"));
}

#[test]
fn receipt_artifacts_is_array() {
    let s = schema_value::<Receipt>();
    assert_eq!(s["properties"]["artifacts"]["type"].as_str(), Some("array"));
}

#[test]
fn context_packet_files_is_array() {
    let s = schema_value::<ContextPacket>();
    assert_eq!(s["properties"]["files"]["type"].as_str(), Some("array"));
}

#[test]
fn context_packet_snippets_is_array() {
    let s = schema_value::<ContextPacket>();
    assert_eq!(s["properties"]["snippets"]["type"].as_str(), Some("array"));
}

#[test]
fn workspace_spec_include_is_array() {
    let s = schema_value::<WorkspaceSpec>();
    assert_eq!(s["properties"]["include"]["type"].as_str(), Some("array"));
}

#[test]
fn capability_requirements_required_is_array() {
    let s = schema_value::<CapabilityRequirements>();
    assert_eq!(s["properties"]["required"]["type"].as_str(), Some("array"));
}

#[test]
fn ir_conversation_messages_is_array() {
    let s = schema_value::<IrConversation>();
    assert_eq!(s["properties"]["messages"]["type"].as_str(), Some("array"));
}

// =========================================================================
// 16. Tagged enum discriminator checks
// =========================================================================

#[test]
fn agent_event_kind_variants_have_type_discriminator() {
    let s = schema_value::<AgentEventKind>();
    let variants = s["oneOf"].as_array().unwrap();
    for variant in variants {
        let has_type = variant["properties"]
            .as_object()
            .map(|p| p.contains_key("type"))
            .unwrap_or(false);
        assert!(
            has_type,
            "AgentEventKind variant missing 'type' discriminator"
        );
    }
}

#[test]
fn rate_limit_result_variants_have_type_discriminator() {
    let s = schema_value::<RateLimitResult>();
    let variants = s["oneOf"].as_array().unwrap();
    for variant in variants {
        let has_type = variant["properties"]
            .as_object()
            .map(|p| p.contains_key("type"))
            .unwrap_or(false);
        assert!(
            has_type,
            "RateLimitResult variant missing 'type' discriminator"
        );
    }
}

#[test]
fn composed_result_variants_have_type_discriminator() {
    let s = schema_value::<ComposedResult>();
    let variants = s["oneOf"].as_array().unwrap();
    for variant in variants {
        let has_type = variant["properties"]
            .as_object()
            .map(|p| p.contains_key("type"))
            .unwrap_or(false);
        assert!(
            has_type,
            "ComposedResult variant missing 'type' discriminator"
        );
    }
}

#[test]
fn ir_content_block_variants_have_type_discriminator() {
    let s = schema_value::<IrContentBlock>();
    let variants = s["oneOf"].as_array().unwrap();
    for variant in variants {
        let has_type = variant["properties"]
            .as_object()
            .map(|p| p.contains_key("type"))
            .unwrap_or(false);
        assert!(
            has_type,
            "IrContentBlock variant missing 'type' discriminator"
        );
    }
}

// =========================================================================
// 17. Complex validation: roundtrip serialize → validate
// =========================================================================

#[test]
fn agent_event_tool_call_validates() {
    let s = schema_value::<AgentEvent>();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "Read".into(),
            tool_use_id: Some("tc_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        },
        ext: None,
    };
    let v = serde_json::to_value(&event).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn agent_event_error_validates() {
    let s = schema_value::<AgentEvent>();
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "something broke".into(),
            error_code: Some(ErrorCode::BackendNotFound),
        },
        ext: None,
    };
    let v = serde_json::to_value(&event).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn agent_event_with_ext_validates() {
    let s = schema_value::<AgentEvent>();
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), json!({"foo": "bar"}));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: "heads up".into(),
        },
        ext: Some(ext),
    };
    let v = serde_json::to_value(&event).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn receipt_with_capabilities_validates() {
    let s = schema_value::<Receipt>();
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let receipt = ReceiptBuilder::new("test-backend")
        .capabilities(caps)
        .outcome(Outcome::Complete)
        .build();
    let v = serde_json::to_value(&receipt).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn work_order_with_full_config_validates() {
    let s = schema_value::<WorkOrder>();
    let wo = WorkOrderBuilder::new("full config test")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .lane(ExecutionLane::WorkspaceFirst)
        .build();
    let v = serde_json::to_value(&wo).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn ir_message_text_validates() {
    let s = schema_value::<IrMessage>();
    let msg = IrMessage::text(IrRole::User, "Hello world");
    let v = serde_json::to_value(&msg).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn ir_conversation_validates() {
    let s = schema_value::<IrConversation>();
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful"))
        .push(IrMessage::text(IrRole::User, "Hi"));
    let v = serde_json::to_value(&conv).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn ir_tool_definition_validates() {
    let s = schema_value::<IrToolDefinition>();
    let tool = IrToolDefinition {
        name: "Read".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let v = serde_json::to_value(&tool).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn decision_allow_validates() {
    let s = schema_value::<Decision>();
    let d = Decision::allow();
    let v = serde_json::to_value(&d).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn decision_deny_validates() {
    let s = schema_value::<Decision>();
    let d = Decision::deny("not permitted");
    let v = serde_json::to_value(&d).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn rate_limit_policy_validates() {
    let s = schema_value::<RateLimitPolicy>();
    let p = RateLimitPolicy {
        max_requests_per_minute: Some(60),
        max_tokens_per_minute: Some(100_000),
        max_concurrent: Some(5),
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_valid(&s, &v);
}

#[test]
fn composition_strategy_validates() {
    let s = schema_value::<CompositionStrategy>();
    for strategy in &[
        CompositionStrategy::AllMustAllow,
        CompositionStrategy::AnyMustAllow,
        CompositionStrategy::FirstMatch,
    ] {
        let v = serde_json::to_value(strategy).unwrap();
        assert_valid(&s, &v);
    }
}

// =========================================================================
// 18. Schema string format checks
// =========================================================================

#[test]
fn run_metadata_run_id_schema_is_uuid_format() {
    let s = schema_value::<RunMetadata>();
    let run_id = &s["properties"]["run_id"];
    assert_eq!(run_id["format"].as_str(), Some("uuid"));
}

#[test]
fn run_metadata_started_at_is_datetime_format() {
    let s = schema_value::<RunMetadata>();
    let started_at = &s["properties"]["started_at"];
    assert_eq!(started_at["format"].as_str(), Some("date-time"));
}

#[test]
fn work_order_id_is_uuid_format() {
    let s = schema_value::<WorkOrder>();
    let id = &s["properties"]["id"];
    assert_eq!(id["format"].as_str(), Some("uuid"));
}

// =========================================================================
// 19. Misc edge-case checks
// =========================================================================

#[test]
fn empty_usage_normalized_validates() {
    let s = schema_value::<UsageNormalized>();
    assert_valid(&s, &json!({}));
}

#[test]
fn rate_limit_policy_empty_validates() {
    let s = schema_value::<RateLimitPolicy>();
    assert_valid(&s, &json!({}));
}

#[test]
fn error_code_schema_is_string_enum() {
    let s = schema_value::<ErrorCode>();
    assert!(
        s.get("enum").is_some() || s.get("oneOf").is_some(),
        "ErrorCode should be an enum or oneOf schema"
    );
}
