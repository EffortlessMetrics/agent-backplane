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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive receipt JSON schema validation tests.
//!
//! Verifies that receipts conform to the JSON schema, maintain structural
//! invariants, and round-trip correctly through serialization.

use abp_core::{
    receipt_hash, AgentEvent, AgentEventKind, ArtifactRef, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, SupportLevel, UsageNormalized, VerificationReport,
    CONTRACT_VERSION,
};
use abp_receipt::ReceiptBuilder;
use chrono::{DateTime, Utc};
use schemars::schema_for;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────

fn receipt_schema() -> Value {
    serde_json::to_value(schema_for!(Receipt)).unwrap()
}

fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
}

fn full_receipt() -> Receipt {
    let started = Utc::now();
    let finished = started + chrono::Duration::milliseconds(1234);
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    ReceiptBuilder::new("sidecar:node")
        .backend_version("1.2.3")
        .adapter_version("0.5.0")
        .outcome(Outcome::Complete)
        .work_order_id(Uuid::new_v4())
        .started_at(started)
        .finished_at(finished)
        .capabilities(caps)
        .mode(ExecutionMode::Mapped)
        .usage_raw(json!({"model": "gpt-4", "tokens": 1000}))
        .usage(UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(300),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(10),
            request_units: Some(1),
            estimated_cost_usd: Some(0.05),
        })
        .verification(VerificationReport {
            git_diff: Some("diff --git a/foo.rs b/foo.rs\n".into()),
            git_status: Some("M foo.rs\n".into()),
            harness_ok: true,
        })
        .add_trace_event(AgentEvent {
            ts: started,
            kind: AgentEventKind::RunStarted {
                message: "Starting run".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: started + chrono::Duration::milliseconds(100),
            kind: AgentEventKind::AssistantMessage {
                text: "Hello world".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: finished,
            kind: AgentEventKind::RunCompleted {
                message: "Done".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
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

// ═══════════════════════════════════════════════════════════════════════
// 1. Minimal receipt validates against schema
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn minimal_receipt_validates_against_schema() {
    let schema = receipt_schema();
    let instance = serde_json::to_value(&minimal_receipt()).unwrap();
    assert_valid(&schema, &instance);
}

#[test]
fn minimal_receipt_has_no_hash() {
    let r = minimal_receipt();
    assert!(r.receipt_sha256.is_none());
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_null());
}

#[test]
fn minimal_receipt_has_empty_trace() {
    let r = minimal_receipt();
    assert!(r.trace.is_empty());
}

#[test]
fn minimal_receipt_has_empty_artifacts() {
    let r = minimal_receipt();
    assert!(r.artifacts.is_empty());
}

#[test]
fn minimal_receipt_has_contract_version() {
    let r = minimal_receipt();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Full receipt with all fields validates
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn full_receipt_validates_against_schema() {
    let schema = receipt_schema();
    let instance = serde_json::to_value(&full_receipt()).unwrap();
    assert_valid(&schema, &instance);
}

#[test]
fn full_receipt_with_hash_validates() {
    let schema = receipt_schema();
    let r = full_receipt().with_hash().unwrap();
    let instance = serde_json::to_value(&r).unwrap();
    assert_valid(&schema, &instance);
}

#[test]
fn full_receipt_has_backend_version() {
    let r = full_receipt();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
}

#[test]
fn full_receipt_has_adapter_version() {
    let r = full_receipt();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
}

#[test]
fn full_receipt_has_capabilities() {
    let r = full_receipt();
    assert!(r.capabilities.contains_key(&Capability::ToolRead));
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn full_receipt_has_usage_tokens() {
    let r = full_receipt();
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(300));
}

#[test]
fn full_receipt_has_verification() {
    let r = full_receipt();
    assert!(r.verification.git_diff.is_some());
    assert!(r.verification.harness_ok);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Receipt with each outcome variant validates
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_outcome_complete_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_outcome_partial_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_outcome_failed_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_outcome_complete_serializes_correctly() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["outcome"], "complete");
}

#[test]
fn receipt_outcome_partial_serializes_correctly() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Partial)
        .build();
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["outcome"], "partial");
}

#[test]
fn receipt_outcome_failed_serializes_correctly() {
    let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["outcome"], "failed");
}

#[test]
fn receipt_invalid_outcome_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["outcome"] = json!("cancelled");
    assert_invalid(&schema, &v);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Receipt JSON has required fields
// ═══════════════════════════════════════════════════════════════════════

static RECEIPT_REQUIRED_FIELDS: &[&str] = &[
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

#[test]
fn receipt_schema_declares_all_required_fields() {
    let schema = receipt_schema();
    let required: Vec<String> = schema["required"]
        .as_array()
        .expect("Receipt schema must have 'required'")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    for field in RECEIPT_REQUIRED_FIELDS {
        assert!(
            required.contains(&field.to_string()),
            "Receipt schema missing required field: {field}"
        );
    }
}

#[test]
fn receipt_missing_meta_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("meta");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_backend_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("backend");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_outcome_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("outcome");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_trace_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("trace");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_usage_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("usage");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_usage_raw_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("usage_raw");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_capabilities_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("capabilities");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_artifacts_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("artifacts");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_missing_verification_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("verification");
    assert_invalid(&schema, &v);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Receipt timestamp format (RFC 3339)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_started_at_is_rfc3339() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    let ts = v["meta"]["started_at"].as_str().unwrap();
    // Must parse as RFC 3339
    DateTime::parse_from_rfc3339(ts).expect("started_at should be RFC 3339");
}

#[test]
fn receipt_finished_at_is_rfc3339() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    let ts = v["meta"]["finished_at"].as_str().unwrap();
    DateTime::parse_from_rfc3339(ts).expect("finished_at should be RFC 3339");
}

#[test]
fn receipt_trace_event_ts_is_rfc3339() {
    let r = full_receipt();
    let v = serde_json::to_value(&r).unwrap();
    for event in v["trace"].as_array().unwrap() {
        let ts = event["ts"].as_str().unwrap();
        DateTime::parse_from_rfc3339(ts)
            .unwrap_or_else(|_| panic!("trace event ts should be RFC 3339: {ts}"));
    }
}

#[test]
fn receipt_invalid_timestamp_format_is_string_type() {
    // jsonschema does not enforce "format" by default, so we verify the
    // schema declares the correct format and the Rust type enforces it.
    let schema = receipt_schema();
    let meta_started = &schema["$defs"]["RunMetadata"]["properties"]["started_at"];
    assert_eq!(meta_started["type"], "string");
    assert_eq!(meta_started["format"], "date-time");
    // Rust serde will reject non-RFC3339 strings at deserialization time
    let bad_json = r#"{"started_at":"not-a-timestamp"}"#;
    let result: Result<abp_core::RunMetadata, _> = serde_json::from_str(bad_json);
    assert!(result.is_err());
}

#[test]
fn receipt_timestamp_schema_declares_date_time_format() {
    let schema = receipt_schema();
    let meta_started = &schema["$defs"]["RunMetadata"]["properties"]["started_at"];
    assert_eq!(meta_started["format"], "date-time");
    let meta_finished = &schema["$defs"]["RunMetadata"]["properties"]["finished_at"];
    assert_eq!(meta_finished["format"], "date-time");
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Receipt sha256 field format (hex string)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_sha256_is_64_char_hex() {
    let r = minimal_receipt().with_hash().unwrap();
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "SHA-256 should be hex only"
    );
}

#[test]
fn receipt_sha256_is_lowercase_hex() {
    let r = minimal_receipt().with_hash().unwrap();
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash, &hash.to_lowercase(), "SHA-256 hex must be lowercase");
}

#[test]
fn receipt_sha256_null_validates() {
    let schema = receipt_schema();
    let r = minimal_receipt(); // no hash
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_null());
    assert_valid(&schema, &v);
}

#[test]
fn receipt_sha256_string_validates() {
    let schema = receipt_schema();
    let r = minimal_receipt().with_hash().unwrap();
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["receipt_sha256"].is_string());
    assert_valid(&schema, &v);
}

#[test]
fn receipt_sha256_deterministic_for_same_receipt() {
    let now = Utc::now();
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(now)
        .finished_at(now)
        .build()
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(now)
        .finished_at(now)
        .build()
        .with_hash()
        .unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn receipt_sha256_ignores_stored_hash_in_computation() {
    let r = minimal_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("deadbeef".into());
    let h2 = receipt_hash(&r2).unwrap();
    assert_eq!(h1, h2, "Hash must ignore receipt_sha256 field");
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Receipt backend identity structure
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backend_identity_has_required_id() {
    let schema = receipt_schema();
    let backend_def = &schema["$defs"]["BackendIdentity"];
    let required: Vec<&str> = backend_def["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(required.contains(&"id"));
}

#[test]
fn backend_identity_id_is_string() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    assert!(v["backend"]["id"].is_string());
}

#[test]
fn backend_identity_versions_optional() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    // backend_version and adapter_version can be null
    assert!(
        v["backend"]["backend_version"].is_null() || v["backend"]["backend_version"].is_string()
    );
    assert!(
        v["backend"]["adapter_version"].is_null() || v["backend"]["adapter_version"].is_string()
    );
}

#[test]
fn backend_identity_with_all_versions_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("test-backend")
        .backend_version("2.0.0")
        .adapter_version("1.0.0")
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn backend_identity_missing_id_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["backend"].as_object_mut().unwrap().remove("id");
    assert_invalid(&schema, &v);
}

#[test]
fn backend_identity_numeric_id_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["backend"]["id"] = json!(42);
    assert_invalid(&schema, &v);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Receipt trace events structure
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn trace_event_run_started_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "begin".into(),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_run_completed_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_assistant_message_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hello".into(),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_assistant_delta_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "tok".into(),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_tool_call_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "foo.rs"}),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_tool_result_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc-1".into()),
            output: json!("file contents"),
            is_error: false,
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_file_changed_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::FileChanged {
            path: "src/lib.rs".into(),
            summary: "Added function".into(),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_command_executed_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("All tests passed".into()),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_warning_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::Warning {
            message: "something odd".into(),
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_error_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::Error {
            message: "something broke".into(),
            error_code: None,
        }))
        .build();
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn trace_event_has_ts_field() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .build();
    let v = serde_json::to_value(&r).unwrap();
    let event = &v["trace"][0];
    assert!(event["ts"].is_string(), "trace event must have ts field");
}

#[test]
fn trace_event_has_type_discriminator() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hi".into(),
        }))
        .build();
    let v = serde_json::to_value(&r).unwrap();
    let event = &v["trace"][0];
    assert_eq!(event["type"], "assistant_message");
}

#[test]
fn trace_event_tool_call_has_required_fields() {
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        }))
        .build();
    let v = serde_json::to_value(&r).unwrap();
    let event = &v["trace"][0];
    assert_eq!(event["type"], "tool_call");
    assert!(event["tool_name"].is_string());
    assert!(!event["input"].is_null());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Receipt meta.run_id is valid UUID
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_run_id_is_valid_uuid() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    let run_id = v["meta"]["run_id"].as_str().unwrap();
    Uuid::parse_str(run_id).expect("run_id must be valid UUID");
}

#[test]
fn receipt_work_order_id_is_valid_uuid() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    let wo_id = v["meta"]["work_order_id"].as_str().unwrap();
    Uuid::parse_str(wo_id).expect("work_order_id must be valid UUID");
}

#[test]
fn receipt_run_id_schema_has_uuid_format() {
    let schema = receipt_schema();
    let run_id_schema = &schema["$defs"]["RunMetadata"]["properties"]["run_id"];
    assert_eq!(run_id_schema["format"], "uuid");
}

#[test]
fn receipt_work_order_id_schema_has_uuid_format() {
    let schema = receipt_schema();
    let wo_id_schema = &schema["$defs"]["RunMetadata"]["properties"]["work_order_id"];
    assert_eq!(wo_id_schema["format"], "uuid");
}

#[test]
fn receipt_custom_run_id_preserved() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock").run_id(id).build();
    assert_eq!(r.meta.run_id, id);
}

#[test]
fn receipt_custom_work_order_id_preserved() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("mock").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn receipt_invalid_uuid_rejected_by_serde() {
    // jsonschema does not enforce "format: uuid" by default, so we verify
    // the schema declares it and Rust serde enforces at deserialization.
    let schema = receipt_schema();
    let run_id_schema = &schema["$defs"]["RunMetadata"]["properties"]["run_id"];
    assert_eq!(run_id_schema["type"], "string");
    assert_eq!(run_id_schema["format"], "uuid");
    // Rust serde rejects invalid UUIDs
    let bad_json = r#"{"run_id":"not-a-uuid","work_order_id":"00000000-0000-0000-0000-000000000000","contract_version":"abp/v0.1","started_at":"2025-01-01T00:00:00Z","finished_at":"2025-01-01T00:00:00Z","duration_ms":0}"#;
    let result: Result<abp_core::RunMetadata, _> = serde_json::from_str(bad_json);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Receipt with empty trace validates
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_empty_trace_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.trace.is_empty());
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_empty_trace_serializes_to_empty_array() {
    let r = ReceiptBuilder::new("mock").build();
    let v = serde_json::to_value(&r).unwrap();
    let trace = v["trace"].as_array().unwrap();
    assert!(trace.is_empty());
}

#[test]
fn receipt_empty_artifacts_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.artifacts.is_empty());
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_empty_capabilities_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock").build();
    assert!(r.capabilities.is_empty());
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Receipt with many events validates
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_with_100_events_validates() {
    let schema = receipt_schema();
    let mut builder = ReceiptBuilder::new("mock");
    for i in 0..100 {
        builder = builder.add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: format!("token-{i}"),
        }));
    }
    let r = builder.build();
    assert_eq!(r.trace.len(), 100);
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_with_mixed_event_types_validates() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "hello ".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "world".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("tc-1".into()),
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        }))
        .add_trace_event(make_event(AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("tc-1".into()),
            output: json!("file1.rs\nfile2.rs"),
            is_error: false,
        }))
        .add_trace_event(make_event(AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "modified".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::CommandExecuted {
            command: "cargo build".into(),
            exit_code: Some(0),
            output_preview: None,
        }))
        .add_trace_event(make_event(AgentEventKind::Warning {
            message: "low budget".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .build();
    assert_eq!(r.trace.len(), 10);
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_with_many_artifacts_validates() {
    let schema = receipt_schema();
    let mut builder = ReceiptBuilder::new("mock");
    for i in 0..20 {
        builder = builder.add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: format!("output_{i}.patch"),
        });
    }
    let r = builder.build();
    assert_eq!(r.artifacts.len(), 20);
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

#[test]
fn receipt_with_many_capabilities_validates() {
    let schema = receipt_schema();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Native);
    caps.insert(Capability::ToolGlob, SupportLevel::Emulated);
    caps.insert(Capability::ToolGrep, SupportLevel::Emulated);
    caps.insert(Capability::McpClient, SupportLevel::Unsupported);
    let r = ReceiptBuilder::new("mock").capabilities(caps).build();
    assert_eq!(r.capabilities.len(), 8);
    assert_valid(&schema, &serde_json::to_value(&r).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Receipt schema evolution (required vs optional fields)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_sha256_is_optional_in_schema() {
    let schema = receipt_schema();
    let required: Vec<String> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        !required.contains(&"receipt_sha256".to_string()),
        "receipt_sha256 must NOT be required"
    );
}

#[test]
fn receipt_mode_is_optional_in_schema() {
    let schema = receipt_schema();
    let required: Vec<String> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        !required.contains(&"mode".to_string()),
        "mode should be optional (has default)"
    );
}

#[test]
fn receipt_mode_has_default_value() {
    let schema = receipt_schema();
    let mode_prop = &schema["properties"]["mode"];
    assert_eq!(mode_prop["default"], "mapped");
}

#[test]
fn receipt_verification_harness_ok_is_required() {
    let schema = receipt_schema();
    let vr_def = &schema["$defs"]["VerificationReport"];
    let required: Vec<&str> = vr_def["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(required.contains(&"harness_ok"));
}

#[test]
fn receipt_verification_git_fields_optional() {
    let schema = receipt_schema();
    let vr_def = &schema["$defs"]["VerificationReport"];
    let required: Vec<&str> = vr_def["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(!required.contains(&"git_diff"));
    assert!(!required.contains(&"git_status"));
}

#[test]
fn run_metadata_all_fields_required() {
    let schema = receipt_schema();
    let meta_def = &schema["$defs"]["RunMetadata"];
    let required: Vec<&str> = meta_def["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let expected = [
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ];
    for field in &expected {
        assert!(
            required.contains(field),
            "RunMetadata missing required field: {field}"
        );
    }
}

#[test]
fn usage_normalized_all_fields_optional() {
    let schema = receipt_schema();
    let usage_def = &schema["$defs"]["UsageNormalized"];
    // UsageNormalized should NOT have a required array, or it should be empty
    let required = usage_def.get("required");
    match required {
        None => {} // no required = all optional, correct
        Some(arr) => {
            assert!(
                arr.as_array().is_none_or(|a| a.is_empty()),
                "UsageNormalized should have no required fields"
            );
        }
    }
}

#[test]
fn receipt_without_optional_mode_field_still_validates() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v.as_object_mut().unwrap().remove("mode");
    assert_valid(&schema, &v);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Receipt canonical JSON ordering (BTreeMap)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_json_keys_are_sorted() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    let obj = v.as_object().unwrap();
    let keys: Vec<&String> = obj.keys().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys, "Top-level receipt keys must be sorted");
}

#[test]
fn receipt_meta_keys_are_sorted() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    let meta = v["meta"].as_object().unwrap();
    let keys: Vec<&String> = meta.keys().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys, "meta keys must be sorted");
}

#[test]
fn receipt_capabilities_keys_sorted() {
    let mut caps = CapabilityManifest::new();
    // Insert in non-alphabetical order
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::McpClient, SupportLevel::Emulated);
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let r = ReceiptBuilder::new("mock").capabilities(caps).build();
    let v = serde_json::to_value(&r).unwrap();
    let cap_obj = v["capabilities"].as_object().unwrap();
    let keys: Vec<&String> = cap_obj.keys().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(
        keys, sorted_keys,
        "capabilities keys must be sorted (BTreeMap)"
    );
}

#[test]
fn canonical_json_is_deterministic() {
    let now = Utc::now();
    let make = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(now)
            .finished_at(now)
            .build()
    };
    let json1 = serde_json::to_string(&make()).unwrap();
    let json2 = serde_json::to_string(&make()).unwrap();
    assert_eq!(json1, json2, "Serialization must be deterministic");
}

#[test]
fn canonical_json_no_extra_whitespace() {
    let r = minimal_receipt();
    let json = serde_json::to_string(&r).unwrap();
    // compact JSON should have no newlines or multiple spaces
    assert!(
        !json.contains('\n'),
        "Compact JSON should not have newlines"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Receipt hash changes when schema changes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hash_changes_when_outcome_changes() {
    let now = Utc::now();
    let base = || {
        ReceiptBuilder::new("mock")
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(now)
            .finished_at(now)
    };
    let r1 = base().outcome(Outcome::Complete).build();
    let r2 = base().outcome(Outcome::Failed).build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_backend_changes() {
    let now = Utc::now();
    let base = |id: &str| {
        ReceiptBuilder::new(id)
            .outcome(Outcome::Complete)
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(now)
            .finished_at(now)
    };
    let r1 = base("mock-a").build();
    let r2 = base("mock-b").build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_trace_differs() {
    let now = Utc::now();
    let base = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(now)
            .finished_at(now)
    };
    let r1 = base().build();
    let r2 = base()
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::Warning {
                message: "w".into(),
            },
            ext: None,
        })
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_verification_changes() {
    let now = Utc::now();
    let base = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(now)
            .finished_at(now)
    };
    let r1 = base()
        .verification(VerificationReport {
            harness_ok: true,
            ..Default::default()
        })
        .build();
    let r2 = base()
        .verification(VerificationReport {
            harness_ok: false,
            ..Default::default()
        })
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_mode_changes() {
    let now = Utc::now();
    let base = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(now)
            .finished_at(now)
    };
    let r1 = base().mode(ExecutionMode::Mapped).build();
    let r2 = base().mode(ExecutionMode::Passthrough).build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_when_usage_changes() {
    let now = Utc::now();
    let base = || {
        ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(now)
            .finished_at(now)
    };
    let r1 = base().build();
    let r2 = base()
        .usage(UsageNormalized {
            input_tokens: Some(100),
            ..Default::default()
        })
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_stable_across_with_hash_calls() {
    let now = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(now)
        .finished_at(now)
        .build();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "Hash must be stable across calls");
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Round-trip: Receipt → JSON → schema validation → Receipt
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_minimal_receipt() {
    let schema = receipt_schema();
    let original = minimal_receipt();
    let json_str = serde_json::to_string(&original).unwrap();
    let value: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &value);
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.backend.id, original.backend.id);
    assert_eq!(deserialized.outcome, original.outcome);
    assert_eq!(
        deserialized.meta.contract_version,
        original.meta.contract_version
    );
}

#[test]
fn roundtrip_full_receipt() {
    let schema = receipt_schema();
    let original = full_receipt();
    let json_str = serde_json::to_string(&original).unwrap();
    let value: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &value);
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.backend.id, original.backend.id);
    assert_eq!(deserialized.trace.len(), original.trace.len());
    assert_eq!(deserialized.artifacts.len(), original.artifacts.len());
}

#[test]
fn roundtrip_hashed_receipt() {
    let schema = receipt_schema();
    let original = minimal_receipt().with_hash().unwrap();
    let json_str = serde_json::to_string(&original).unwrap();
    let value: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &value);
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.receipt_sha256, original.receipt_sha256);
}

#[test]
fn roundtrip_preserves_hash_integrity() {
    let original = minimal_receipt().with_hash().unwrap();
    let json_str = serde_json::to_string(&original).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    // Recompute hash on deserialized receipt should match
    let recomputed = receipt_hash(&deserialized).unwrap();
    assert_eq!(
        deserialized.receipt_sha256.as_ref().unwrap(),
        &recomputed,
        "Hash must survive round-trip"
    );
}

#[test]
fn roundtrip_receipt_with_all_event_types() {
    let schema = receipt_schema();
    let now = Utc::now();
    let r = ReceiptBuilder::new("mock")
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::AssistantDelta { text: "tok".into() },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("x".into()),
                parent_tool_use_id: None,
                input: json!({}),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("x".into()),
                output: json!("data"),
                is_error: false,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::FileChanged {
                path: "f.rs".into(),
                summary: "s".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::Warning {
                message: "w".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::Error {
                message: "e".into(),
                error_code: None,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: now,
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();

    let json_str = serde_json::to_string(&r).unwrap();
    let value: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &value);
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.trace.len(), 9);
}

#[test]
fn roundtrip_passthrough_mode() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Passthrough)
        .build();
    let json_str = serde_json::to_string(&r).unwrap();
    let value: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &value);
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.mode, ExecutionMode::Passthrough);
}

#[test]
fn roundtrip_mapped_mode() {
    let schema = receipt_schema();
    let r = ReceiptBuilder::new("mock")
        .mode(ExecutionMode::Mapped)
        .build();
    let json_str = serde_json::to_string(&r).unwrap();
    let value: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &value);
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.mode, ExecutionMode::Mapped);
}

#[test]
fn roundtrip_receipt_with_ext_data() {
    let schema = receipt_schema();
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"role": "assistant"}));

    let r = ReceiptBuilder::new("mock")
        .add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        })
        .build();
    let json_str = serde_json::to_string(&r).unwrap();
    let value: Value = serde_json::from_str(&json_str).unwrap();
    assert_valid(&schema, &value);
    let deserialized: Receipt = serde_json::from_str(&json_str).unwrap();
    assert!(deserialized.trace[0].ext.is_some());
}

#[test]
fn roundtrip_committed_schema_file() {
    let committed: Value =
        serde_json::from_str(include_str!("../contracts/schemas/receipt.schema.json"))
            .expect("committed receipt schema must be valid JSON");
    let generated = receipt_schema();
    assert_eq!(
        generated, committed,
        "Generated Receipt schema differs from contracts/schemas/receipt.schema.json"
    );
}

#[test]
fn receipt_schema_is_valid_json_schema() {
    let schema = receipt_schema();
    jsonschema::validator_for(&schema).expect("Receipt schema must compile");
}

#[test]
fn receipt_schema_title_is_receipt() {
    let schema = receipt_schema();
    assert_eq!(schema["title"], "Receipt");
}

#[test]
fn receipt_schema_is_object_type() {
    let schema = receipt_schema();
    assert_eq!(schema["type"], "object");
}

#[test]
fn receipt_contract_version_matches_constant() {
    let r = minimal_receipt();
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn receipt_duration_ms_is_non_negative() {
    let r = minimal_receipt();
    let v = serde_json::to_value(&r).unwrap();
    let dur = v["meta"]["duration_ms"].as_u64().unwrap();
    assert!(dur < u64::MAX); // u64 is always >= 0
}

#[test]
fn receipt_duration_ms_schema_has_minimum() {
    let schema = receipt_schema();
    let dur_schema = &schema["$defs"]["RunMetadata"]["properties"]["duration_ms"];
    assert_eq!(dur_schema["minimum"], 0);
}

#[test]
fn receipt_artifact_ref_has_required_fields() {
    let schema = receipt_schema();
    let art_def = &schema["$defs"]["ArtifactRef"];
    let required: Vec<&str> = art_def["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(required.contains(&"kind"));
    assert!(required.contains(&"path"));
}

#[test]
fn receipt_outcome_schema_has_three_variants() {
    let schema = receipt_schema();
    let outcome_def = &schema["$defs"]["Outcome"];
    let variants = outcome_def["oneOf"].as_array().unwrap();
    assert_eq!(variants.len(), 3);
}

#[test]
fn receipt_execution_mode_schema_has_two_variants() {
    let schema = receipt_schema();
    let mode_def = &schema["$defs"]["ExecutionMode"];
    let variants = mode_def["oneOf"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
}

#[test]
fn receipt_wrong_type_for_trace_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["trace"] = json!("not an array");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_wrong_type_for_artifacts_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["artifacts"] = json!("not an array");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_wrong_type_for_capabilities_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["capabilities"] = json!("not an object");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_wrong_type_for_verification_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["verification"] = json!("not an object");
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_wrong_type_for_meta_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["meta"] = json!(42);
    assert_invalid(&schema, &v);
}

#[test]
fn receipt_wrong_type_for_backend_rejected() {
    let schema = receipt_schema();
    let mut v = serde_json::to_value(&minimal_receipt()).unwrap();
    v["backend"] = json!([]);
    assert_invalid(&schema, &v);
}
