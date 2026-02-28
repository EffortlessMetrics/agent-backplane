// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive JSON Schema validation tests for ABP contract types.
//!
//! These tests ensure that:
//! - Generated schemas are valid JSON Schema (Draft 2020-12)
//! - Serialized Rust types validate against their schemas
//! - Invalid data is correctly rejected
//! - Enum variants, required/optional fields are properly represented
//! - Schemas stay in sync with Rust types (regression guard)

use abp_core::{
    AgentEvent, AgentEventKind, ExecutionLane, ExecutionMode, Outcome, ReceiptBuilder,
    WorkOrderBuilder,
};
use schemars::schema_for;
use serde_json::{json, Value};

// ── helpers ──────────────────────────────────────────────────────────

fn work_order_schema() -> Value {
    serde_json::to_value(schema_for!(abp_core::WorkOrder)).unwrap()
}

fn receipt_schema() -> Value {
    serde_json::to_value(schema_for!(abp_core::Receipt)).unwrap()
}

fn backplane_config_schema() -> Value {
    serde_json::to_value(schema_for!(abp_cli::config::BackplaneConfig)).unwrap()
}

fn valid_work_order_json() -> Value {
    let wo = WorkOrderBuilder::new("Fix the bug").build();
    serde_json::to_value(&wo).unwrap()
}

fn valid_receipt_json() -> Value {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    serde_json::to_value(&receipt).unwrap()
}

fn assert_valid(schema: &Value, instance: &Value) {
    let validator = jsonschema::validator_for(schema)
        .expect("schema should compile into a validator");
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
    let validator = jsonschema::validator_for(schema)
        .expect("schema should compile into a validator");
    assert!(
        !validator.is_valid(instance),
        "Instance should NOT validate, but it did"
    );
}

// ── 1. Work order schema is valid JSON Schema draft ──────────────────

#[test]
fn work_order_schema_is_valid_draft_2020_12() {
    let schema = work_order_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema",
        "WorkOrder schema must declare Draft 2020-12"
    );
    // Compiling the schema itself validates it is well-formed.
    jsonschema::validator_for(&schema).expect("WorkOrder schema must be valid JSON Schema");
}

// ── 2. Receipt schema is valid JSON Schema draft ─────────────────────

#[test]
fn receipt_schema_is_valid_draft_2020_12() {
    let schema = receipt_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema",
        "Receipt schema must declare Draft 2020-12"
    );
    jsonschema::validator_for(&schema).expect("Receipt schema must be valid JSON Schema");
}

// ── 3. Valid WorkOrder validates against schema ──────────────────────

#[test]
fn valid_work_order_passes_validation() {
    let schema = work_order_schema();
    let instance = valid_work_order_json();
    assert_valid(&schema, &instance);
}

// ── 4. Invalid WorkOrder fails validation ────────────────────────────

#[test]
fn invalid_work_order_missing_task_fails() {
    let schema = work_order_schema();
    let mut instance = valid_work_order_json();
    instance.as_object_mut().unwrap().remove("task");
    assert_invalid(&schema, &instance);
}

#[test]
fn invalid_work_order_wrong_lane_type_fails() {
    let schema = work_order_schema();
    let mut instance = valid_work_order_json();
    instance["lane"] = json!(42);
    assert_invalid(&schema, &instance);
}

// ── 5. Schema includes all required fields ───────────────────────────

#[test]
fn work_order_schema_required_fields() {
    let schema = work_order_schema();
    let required: Vec<String> = schema["required"]
        .as_array()
        .expect("WorkOrder schema must have 'required'")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let expected = [
        "id", "task", "lane", "workspace", "context", "policy", "requirements", "config",
    ];
    for field in &expected {
        assert!(
            required.contains(&field.to_string()),
            "WorkOrder schema missing required field: {field}"
        );
    }
}

#[test]
fn receipt_schema_required_fields() {
    let schema = receipt_schema();
    let required: Vec<String> = schema["required"]
        .as_array()
        .expect("Receipt schema must have 'required'")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    let expected = [
        "meta",
        "backend",
        "capabilities",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
    ];
    for field in &expected {
        assert!(
            required.contains(&field.to_string()),
            "Receipt schema missing required field: {field}"
        );
    }
}

// ── 6. Schema allows optional fields ─────────────────────────────────

#[test]
fn work_order_optional_fields_accepted() {
    let schema = work_order_schema();

    // Build a work order with optional fields populated.
    let wo = WorkOrderBuilder::new("Test")
        .model("gpt-4")
        .max_turns(10)
        .max_budget_usd(5.0)
        .build();
    let instance = serde_json::to_value(&wo).unwrap();
    assert_valid(&schema, &instance);

    // Also valid without optional fields (model = null, etc.).
    let wo_minimal = WorkOrderBuilder::new("Test").build();
    let minimal_val = serde_json::to_value(&wo_minimal).unwrap();
    assert_valid(&schema, &minimal_val);
}

#[test]
fn receipt_optional_receipt_sha256_accepted() {
    let schema = receipt_schema();

    // Without hash.
    let receipt = ReceiptBuilder::new("mock").build();
    let instance = serde_json::to_value(&receipt).unwrap();
    assert_valid(&schema, &instance);

    // With hash.
    let receipt_hashed = ReceiptBuilder::new("mock").build().with_hash().unwrap();
    let hashed_val = serde_json::to_value(&receipt_hashed).unwrap();
    assert_valid(&schema, &hashed_val);
}

// ── 7. Schema rejects unknown required fields ────────────────────────

#[test]
fn work_order_rejects_wrong_type_for_required_field() {
    let schema = work_order_schema();

    // `id` must be a string (uuid), not a number.
    let mut instance = valid_work_order_json();
    instance["id"] = json!(12345);
    assert_invalid(&schema, &instance);

    // `workspace` must be an object, not a string.
    let mut instance2 = valid_work_order_json();
    instance2["workspace"] = json!("not an object");
    assert_invalid(&schema, &instance2);
}

#[test]
fn receipt_rejects_invalid_outcome() {
    let schema = receipt_schema();
    let mut instance = valid_receipt_json();
    instance["outcome"] = json!("unknown_outcome");
    assert_invalid(&schema, &instance);
}

// ── 8. All enum variants are present in schema ───────────────────────

#[test]
fn capability_enum_variants_in_schema() {
    let schema = work_order_schema();
    let cap_def = &schema["$defs"]["Capability"];
    let variants: Vec<&str> = cap_def["enum"]
        .as_array()
        .expect("Capability should be an enum in schema")
        .iter()
        .map(|v| v.as_str().unwrap())
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
    ];
    for variant in &expected {
        assert!(
            variants.contains(variant),
            "Capability enum missing variant: {variant}"
        );
    }
    assert_eq!(
        variants.len(),
        expected.len(),
        "Unexpected extra Capability variants in schema"
    );
}

#[test]
fn execution_lane_variants_in_schema() {
    let schema = work_order_schema();
    let lane_def = &schema["$defs"]["ExecutionLane"];
    let one_of = lane_def["oneOf"]
        .as_array()
        .expect("ExecutionLane should use oneOf");

    let consts: Vec<&str> = one_of
        .iter()
        .map(|v| v["const"].as_str().unwrap())
        .collect();

    assert!(consts.contains(&"patch_first"));
    assert!(consts.contains(&"workspace_first"));
    assert_eq!(consts.len(), 2);
}

#[test]
fn outcome_enum_variants_in_schema() {
    let schema = receipt_schema();
    let outcome_def = &schema["$defs"]["Outcome"];
    let variants: Vec<&str> = outcome_def["enum"]
        .as_array()
        .expect("Outcome should be an enum")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    assert_eq!(variants, vec!["complete", "partial", "failed"]);
}

#[test]
fn execution_mode_variants_in_schema() {
    let schema = receipt_schema();
    let mode_def = &schema["$defs"]["ExecutionMode"];
    let one_of = mode_def["oneOf"]
        .as_array()
        .expect("ExecutionMode should use oneOf");

    let consts: Vec<&str> = one_of
        .iter()
        .map(|v| v["const"].as_str().unwrap())
        .collect();

    assert!(consts.contains(&"passthrough"));
    assert!(consts.contains(&"mapped"));
    assert_eq!(consts.len(), 2);
}

#[test]
fn workspace_mode_variants_in_schema() {
    let schema = work_order_schema();
    let mode_def = &schema["$defs"]["WorkspaceMode"];
    let one_of = mode_def["oneOf"]
        .as_array()
        .expect("WorkspaceMode should use oneOf");

    let consts: Vec<&str> = one_of
        .iter()
        .map(|v| v["const"].as_str().unwrap())
        .collect();

    assert!(consts.contains(&"pass_through"));
    assert!(consts.contains(&"staged"));
    assert_eq!(consts.len(), 2);
}

// ── 9. Generated schemas match current types (regression guard) ──────

#[test]
fn work_order_schema_matches_committed_file() {
    let generated = work_order_schema();
    let committed: Value =
        serde_json::from_str(include_str!("../contracts/schemas/work_order.schema.json"))
            .expect("committed work_order schema must be valid JSON");
    assert_eq!(
        generated, committed,
        "Generated WorkOrder schema differs from contracts/schemas/work_order.schema.json. \
         Run `cargo run -p xtask -- schema` to regenerate."
    );
}

#[test]
fn receipt_schema_matches_committed_file() {
    let generated = receipt_schema();
    let committed: Value =
        serde_json::from_str(include_str!("../contracts/schemas/receipt.schema.json"))
            .expect("committed receipt schema must be valid JSON");
    assert_eq!(
        generated, committed,
        "Generated Receipt schema differs from contracts/schemas/receipt.schema.json. \
         Run `cargo run -p xtask -- schema` to regenerate."
    );
}

#[test]
fn backplane_config_schema_matches_committed_file() {
    let generated = backplane_config_schema();
    let committed: Value = serde_json::from_str(include_str!(
        "../contracts/schemas/backplane_config.schema.json"
    ))
    .expect("committed backplane_config schema must be valid JSON");
    assert_eq!(
        generated, committed,
        "Generated BackplaneConfig schema differs from contracts/schemas/backplane_config.schema.json. \
         Run `cargo run -p xtask -- schema` to regenerate."
    );
}

// ── 10. Schema round-trip: generate → parse → validate sample data ───

#[test]
fn work_order_round_trip() {
    // Generate schema from Rust types.
    let schema = work_order_schema();

    // Build a work order with various features.
    let wo = WorkOrderBuilder::new("Implement feature X")
        .lane(ExecutionLane::WorkspaceFirst)
        .root("/tmp/ws")
        .model("claude-3")
        .max_turns(5)
        .build();

    // Serialize → JSON string → parse back → validate.
    let json_str = serde_json::to_string_pretty(&wo).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &parsed);

    // Also round-trip through deserialization.
    let deserialized: abp_core::WorkOrder = serde_json::from_value(parsed.clone()).unwrap();
    let re_serialized = serde_json::to_value(&deserialized).unwrap();
    assert_valid(&schema, &re_serialized);
}

#[test]
fn receipt_round_trip() {
    let schema = receipt_schema();
    let now = chrono::Utc::now();
    let event = AgentEvent {
        ts: now,
        kind: AgentEventKind::RunStarted {
            message: "starting".into(),
        },
        ext: None,
    };

    let receipt = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Partial)
        .mode(ExecutionMode::Passthrough)
        .add_trace_event(event)
        .build()
        .with_hash()
        .unwrap();

    let json_str = serde_json::to_string_pretty(&receipt).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &parsed);

    let deserialized: abp_core::Receipt = serde_json::from_value(parsed.clone()).unwrap();
    let re_serialized = serde_json::to_value(&deserialized).unwrap();
    assert_valid(&schema, &re_serialized);
}

// ── 11. Backplane config schema validates example config ─────────────

#[test]
fn backplane_config_schema_is_valid() {
    let schema = backplane_config_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    jsonschema::validator_for(&schema).expect("BackplaneConfig schema must be valid");
}

#[test]
fn backplane_config_validates_example() {
    let schema = backplane_config_schema();

    // Construct a JSON equivalent of backplane.example.toml.
    let config = json!({
        "backends": {
            "mock": {
                "type": "mock"
            },
            "openai": {
                "type": "sidecar",
                "command": "node",
                "args": ["path/to/openai-sidecar.js"]
            },
            "anthropic": {
                "type": "sidecar",
                "command": "python3",
                "args": ["path/to/anthropic-sidecar.py"]
            }
        }
    });
    assert_valid(&schema, &config);
}

#[test]
fn backplane_config_empty_is_valid() {
    let schema = backplane_config_schema();
    let empty = json!({});
    assert_valid(&schema, &empty);
}

// ── 12. Schema evolution: adding a field doesn't break existing data ─

#[test]
fn schema_evolution_extra_fields_in_work_order() {
    let schema = work_order_schema();

    // A valid work order with an extra unknown field should still validate
    // (schemas generated by schemars don't set additionalProperties: false
    //  at the top level by default).
    let mut instance = valid_work_order_json();
    instance.as_object_mut().unwrap().insert(
        "future_field".to_string(),
        json!("some future data"),
    );
    assert_valid(&schema, &instance);
}

#[test]
fn schema_evolution_extra_fields_in_receipt() {
    let schema = receipt_schema();
    let mut instance = valid_receipt_json();
    instance.as_object_mut().unwrap().insert(
        "new_optional_field".to_string(),
        json!({"nested": true}),
    );
    assert_valid(&schema, &instance);
}

#[test]
fn schema_evolution_old_data_still_valid_after_optional_additions() {
    // Simulate "old" data by stripping optional fields that have defaults.
    let schema = receipt_schema();
    let mut instance = valid_receipt_json();

    // `mode` has `#[serde(default)]` so old data without it should validate.
    instance.as_object_mut().unwrap().remove("mode");
    // `receipt_sha256` is optional.
    instance.as_object_mut().unwrap().remove("receipt_sha256");

    assert_valid(&schema, &instance);
}

// ── Bonus: agent event kind variants all validate ────────────────────

#[test]
fn all_agent_event_kinds_validate_in_receipt() {
    let schema = receipt_schema();
    let now = chrono::Utc::now();

    let events = vec![
        AgentEvent {
            ts: now,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta {
                text: "hello".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantMessage {
                text: "full msg".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "/tmp"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                output: json!("file contents"),
                is_error: false,
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added fn".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::Warning {
                message: "watch out".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::Error {
                message: "oops".into(),
            },
            ext: None,
        },
    ];

    let mut receipt_builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
    for event in events {
        receipt_builder = receipt_builder.add_trace_event(event);
    }
    let receipt = receipt_builder.build();
    let instance = serde_json::to_value(&receipt).unwrap();
    assert_valid(&schema, &instance);
}
