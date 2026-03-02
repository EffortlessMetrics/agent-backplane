// SPDX-License-Identifier: MIT OR Apache-2.0
//! Snapshot tests for the receipt system (abp-receipt and abp-core receipts).

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, Capability, CapabilityManifest,
    ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized,
    VerificationReport,
};
use abp_receipt::{
    ReceiptBuilder, ReceiptChain, ReceiptDiff, canonicalize, compute_hash, diff_receipts,
    verify_hash,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_ts2() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 5, 0).unwrap()
}

fn fixed_ts3() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 10, 0).unwrap()
}

fn fixed_uuid() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap()
}

fn fixed_uuid2() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap()
}

fn fixed_uuid3() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000003").unwrap()
}

fn fixed_uuid4() -> Uuid {
    Uuid::parse_str("00000000-0000-4000-8000-000000000004").unwrap()
}

fn sample_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps
}

fn full_capabilities() -> CapabilityManifest {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps.insert(Capability::CodeExecution, SupportLevel::Unsupported);
    caps.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "requires explicit opt-in".into(),
        },
    );
    caps
}

/// Build a fully-populated receipt with all fields set.
fn full_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: Some("3.5.0".into()),
            adapter_version: Some("0.2.1".into()),
        },
        capabilities: full_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({
            "input_tokens": 1500,
            "output_tokens": 3200,
            "cache_creation_input_tokens": 200,
            "cache_read_input_tokens": 50
        }),
        usage: UsageNormalized {
            input_tokens: Some(1500),
            output_tokens: Some(3200),
            cache_read_tokens: Some(50),
            cache_write_tokens: Some(200),
            request_units: None,
            estimated_cost_usd: Some(0.042),
        },
        trace: vec![
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "Initializing agent run".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tu_001".into()),
                    parent_tool_use_id: None,
                    input: json!({"path": "src/lib.rs"}),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::ToolResult {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tu_001".into()),
                    output: json!("fn main() {}"),
                    is_error: false,
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts2(),
                kind: AgentEventKind::FileChanged {
                    path: "src/lib.rs".into(),
                    summary: "added new function".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts2(),
                kind: AgentEventKind::RunCompleted {
                    message: "Task completed successfully".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![
            ArtifactRef {
                kind: "file".into(),
                path: "src/lib.rs".into(),
            },
            ArtifactRef {
                kind: "patch".into(),
                path: "changes.patch".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n fn main() {}\n+fn helper() {}".into()),
            git_status: Some("M src/lib.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

/// Build a minimal receipt with only required fields.
fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::default(),
        usage_raw: json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ===========================================================================
// 1. Receipt with all fields populated → snapshot JSON
// ===========================================================================

#[test]
fn receipt_full_snapshot() {
    let r = full_receipt();
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!(json_str);
}

// ===========================================================================
// 2. Receipt with minimal fields → snapshot
// ===========================================================================

#[test]
fn receipt_minimal_snapshot() {
    let r = minimal_receipt();
    insta::assert_json_snapshot!(r);
}

// ===========================================================================
// 3. Receipt hash determinism: same inputs always produce same hash
// ===========================================================================

#[test]
fn receipt_hash_determinism() {
    let r = full_receipt();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    // Snapshot the hash itself so any canonical form change is caught.
    insta::assert_snapshot!(h1);
}

#[test]
fn receipt_hash_determinism_minimal() {
    let r = minimal_receipt();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    insta::assert_snapshot!(h1);
}

// ===========================================================================
// 4. Receipt chain with 3 sequential receipts → snapshot chain state
// ===========================================================================

#[test]
fn receipt_chain_three_sequential() {
    let mut chain = ReceiptChain::new();

    let r1 = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Complete)
        .backend_version("3.5.0")
        .mode(ExecutionMode::Mapped)
        .usage(UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(1200),
            ..Default::default()
        })
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();

    let r2 = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid3())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts2())
        .finished_at(fixed_ts2())
        .outcome(Outcome::Partial)
        .backend_version("3.5.0")
        .mode(ExecutionMode::Mapped)
        .usage(UsageNormalized {
            input_tokens: Some(800),
            output_tokens: Some(100),
            ..Default::default()
        })
        .with_hash()
        .unwrap();
    chain.push(r2).unwrap();

    let r3 = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid4())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts3())
        .finished_at(fixed_ts3())
        .outcome(Outcome::Complete)
        .backend_version("3.5.0")
        .mode(ExecutionMode::Mapped)
        .usage(UsageNormalized {
            input_tokens: Some(300),
            output_tokens: Some(900),
            ..Default::default()
        })
        .with_hash()
        .unwrap();
    chain.push(r3).unwrap();

    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());

    // Snapshot the chain as a JSON array of receipts.
    let receipts: Vec<&Receipt> = chain.iter().collect();
    insta::assert_json_snapshot!(receipts);
}

// ===========================================================================
// 5. ReceiptBuilder step-by-step construction → snapshot at each stage
// ===========================================================================

#[test]
fn receipt_builder_stage_minimal() {
    let r = ReceiptBuilder::new("mock")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .build();
    insta::assert_json_snapshot!("builder_stage_minimal", r);
}

#[test]
fn receipt_builder_stage_with_backend() {
    let r = ReceiptBuilder::new("sidecar:node")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .backend_version("1.0.0")
        .adapter_version("0.1.0")
        .build();
    insta::assert_json_snapshot!("builder_stage_with_backend", r);
}

#[test]
fn receipt_builder_stage_with_capabilities() {
    let r = ReceiptBuilder::new("sidecar:node")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .backend_version("1.0.0")
        .adapter_version("0.1.0")
        .capabilities(sample_capabilities())
        .build();
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!("builder_stage_with_capabilities", json_str);
}

#[test]
fn receipt_builder_stage_with_usage() {
    let r = ReceiptBuilder::new("sidecar:node")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .backend_version("1.0.0")
        .adapter_version("0.1.0")
        .capabilities(sample_capabilities())
        .usage_raw(json!({"input_tokens": 500, "output_tokens": 1000}))
        .usage(UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(1000),
            ..Default::default()
        })
        .build();
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!("builder_stage_with_usage", json_str);
}

#[test]
fn receipt_builder_stage_with_trace_and_artifacts() {
    let r = ReceiptBuilder::new("sidecar:node")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts2())
        .backend_version("1.0.0")
        .adapter_version("0.1.0")
        .capabilities(sample_capabilities())
        .usage_raw(json!({"input_tokens": 500, "output_tokens": 1000}))
        .usage(UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(1000),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: fixed_ts2(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "file".into(),
            path: "output.txt".into(),
        })
        .outcome(Outcome::Complete)
        .verification(VerificationReport {
            git_diff: Some("diff --git a/output.txt".into()),
            git_status: Some("A output.txt".into()),
            harness_ok: true,
        })
        .build();
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!("builder_stage_full", json_str);
}

#[test]
fn receipt_builder_with_hash_snapshot() {
    let r = ReceiptBuilder::new("sidecar:node")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    // The hash should be populated.
    assert!(r.receipt_sha256.is_some());
    insta::assert_json_snapshot!("builder_with_hash", r);
}

// ===========================================================================
// 6. ReceiptDiff between two receipts → snapshot diff output
// ===========================================================================

#[test]
fn receipt_diff_identical() {
    let r = full_receipt();
    let diff = diff_receipts(&r, &r.clone());
    assert!(diff.is_empty());
    insta::assert_snapshot!(format_diff(&diff));
}

#[test]
fn receipt_diff_multiple_changes() {
    let a = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Complete)
        .backend_version("3.5.0")
        .mode(ExecutionMode::Mapped)
        .usage_raw(json!({"input_tokens": 500}))
        .build();

    let mut b = a.clone();
    b.backend.id = "sidecar:gemini".into();
    b.backend.backend_version = Some("2.0.0".into());
    b.outcome = Outcome::Failed;
    b.mode = ExecutionMode::Passthrough;
    b.usage_raw = json!({"input_tokens": 900});

    let diff = diff_receipts(&a, &b);
    assert!(!diff.is_empty());
    insta::assert_snapshot!(format_diff(&diff));
}

/// Format a ReceiptDiff as a human-readable string for snapshot comparison.
fn format_diff(diff: &ReceiptDiff) -> String {
    if diff.is_empty() {
        return "(no differences)".to_string();
    }
    let mut lines = Vec::new();
    lines.push(format!("--- receipt diff ({} changes) ---", diff.len()));
    for change in &diff.changes {
        lines.push(format!(
            "  {}: {} → {}",
            change.field, change.old, change.new
        ));
    }
    lines.join("\n")
}

// ===========================================================================
// 7. Receipt from passthrough mode → snapshot
// ===========================================================================

#[test]
fn receipt_passthrough_mode_snapshot() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: Some("0.1.0".into()),
        },
        capabilities: {
            let mut caps = BTreeMap::new();
            caps.insert(Capability::Streaming, SupportLevel::Native);
            caps
        },
        mode: ExecutionMode::Passthrough,
        usage_raw: json!({"prompt_tokens": 200, "completion_tokens": 400}),
        usage: UsageNormalized {
            input_tokens: Some(200),
            output_tokens: Some(400),
            ..Default::default()
        },
        trace: vec![
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "passthrough run".into(),
                },
                ext: Some({
                    let mut ext = BTreeMap::new();
                    ext.insert(
                        "raw_message".into(),
                        json!({"role": "system", "content": "You are a helpful assistant."}),
                    );
                    ext
                }),
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantMessage {
                    text: "Hello from passthrough!".into(),
                },
                ext: Some({
                    let mut ext = BTreeMap::new();
                    ext.insert(
                        "raw_message".into(),
                        json!({"role": "assistant", "content": "Hello from passthrough!"}),
                    );
                    ext
                }),
            },
        ],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!(json_str);
}

// ===========================================================================
// 8. Receipt from mapped mode → snapshot
// ===========================================================================

#[test]
fn receipt_mapped_mode_snapshot() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: fixed_uuid(),
            work_order_id: fixed_uuid2(),
            contract_version: "abp/v0.1".into(),
            started_at: fixed_ts(),
            finished_at: fixed_ts2(),
            duration_ms: 300_000,
        },
        backend: BackendIdentity {
            id: "sidecar:claude".into(),
            backend_version: Some("3.5.0".into()),
            adapter_version: Some("0.2.0".into()),
        },
        capabilities: sample_capabilities(),
        mode: ExecutionMode::Mapped,
        usage_raw: json!({"input_tokens": 800, "output_tokens": 1600}),
        usage: UsageNormalized {
            input_tokens: Some(800),
            output_tokens: Some(1600),
            ..Default::default()
        },
        trace: vec![
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::RunStarted {
                    message: "mapped run".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantDelta {
                    text: "I'll help you ".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts(),
                kind: AgentEventKind::AssistantDelta {
                    text: "with that task.".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: fixed_ts2(),
                kind: AgentEventKind::RunCompleted {
                    message: "completed".into(),
                },
                ext: None,
            },
        ],
        artifacts: vec![ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        }],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: false,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!(json_str);
}

// ===========================================================================
// 9. Receipt with error outcomes → snapshot
// ===========================================================================

#[test]
fn receipt_outcome_failed() {
    let r = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts2())
        .outcome(Outcome::Failed)
        .backend_version("3.5.0")
        .add_trace_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Error {
                message: "rate limit exceeded, retries exhausted".into(),
                error_code: None,
            },
            ext: None,
        })
        .build();
    insta::assert_json_snapshot!(r);
}

#[test]
fn receipt_outcome_partial() {
    let r = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts2())
        .outcome(Outcome::Partial)
        .backend_version("3.5.0")
        .usage(UsageNormalized {
            input_tokens: Some(5000),
            output_tokens: Some(10000),
            estimated_cost_usd: Some(0.15),
            ..Default::default()
        })
        .add_trace_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::Warning {
                message: "token budget 95% consumed, stopping early".into(),
            },
            ext: None,
        })
        .build();
    insta::assert_json_snapshot!(r);
}

// ===========================================================================
// 10. Receipt with capability negotiation results in metadata → snapshot
// ===========================================================================

#[test]
fn receipt_capability_negotiation_results() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolUse, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ExtendedThinking, SupportLevel::Emulated);
    caps.insert(
        Capability::StructuredOutputJsonSchema,
        SupportLevel::Emulated,
    );
    caps.insert(Capability::CodeExecution, SupportLevel::Unsupported);
    caps.insert(
        Capability::McpClient,
        SupportLevel::Restricted {
            reason: "MCP client disabled by policy".into(),
        },
    );

    let r = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Complete)
        .capabilities(caps)
        .build();
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!(json_str);
}

// ===========================================================================
// 11. Receipt hash null-before-hash invariant verification
// ===========================================================================

#[test]
fn receipt_hash_null_before_hash_invariant() {
    let r = full_receipt();

    // Canonical form always has receipt_sha256: null regardless of stored value.
    let canon1 = canonicalize(&r).unwrap();
    let parsed1: serde_json::Value = serde_json::from_str(&canon1).unwrap();
    assert_eq!(parsed1["receipt_sha256"], serde_json::Value::Null);

    // Even with a hash set, canonical form still has null.
    let mut r_with_hash = r.clone();
    r_with_hash.receipt_sha256 = Some("some_hash_value".into());
    let canon2 = canonicalize(&r_with_hash).unwrap();
    let parsed2: serde_json::Value = serde_json::from_str(&canon2).unwrap();
    assert_eq!(parsed2["receipt_sha256"], serde_json::Value::Null);

    // Both canonical forms must be identical.
    assert_eq!(canon1, canon2);

    // Therefore hashes must be identical.
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r_with_hash).unwrap();
    assert_eq!(h1, h2);

    // Snapshot the canonical form to catch any structural changes.
    insta::assert_snapshot!(canon1);
}

// ===========================================================================
// 12. Receipt canonical JSON format (BTreeMap ordering)
// ===========================================================================

#[test]
fn receipt_canonical_json_key_ordering() {
    let r = minimal_receipt();
    let canon = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&canon).unwrap();
    // Verify that top-level keys are sorted (BTreeMap ordering).
    if let serde_json::Value::Object(map) = &parsed {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted_keys = keys.clone();
        sorted_keys.sort();
        assert_eq!(keys, sorted_keys, "Top-level keys must be sorted");
    }
    insta::assert_snapshot!(canon);
}

#[test]
fn receipt_canonical_json_full_key_ordering() {
    let r = full_receipt();
    let canon = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&canon).unwrap();
    if let serde_json::Value::Object(map) = &parsed {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted_keys = keys.clone();
        sorted_keys.sort();
        assert_eq!(keys, sorted_keys, "Top-level keys must be sorted");
    }
    insta::assert_snapshot!(canon);
}

// ===========================================================================
// Additional: verify_hash round-trip with abp_receipt functions
// ===========================================================================

#[test]
fn receipt_verify_hash_roundtrip() {
    let r = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts2())
        .outcome(Outcome::Complete)
        .capabilities(sample_capabilities())
        .backend_version("3.5.0")
        .with_hash()
        .unwrap();

    assert!(verify_hash(&r));
    let json_str = serde_json::to_string_pretty(&r).unwrap();
    insta::assert_snapshot!(json_str);
}

#[test]
fn receipt_diff_passthrough_vs_mapped() {
    let passthrough = ReceiptBuilder::new("sidecar:node")
        .run_id(fixed_uuid())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .mode(ExecutionMode::Passthrough)
        .outcome(Outcome::Complete)
        .build();

    let mapped = ReceiptBuilder::new("sidecar:claude")
        .run_id(fixed_uuid3())
        .work_order_id(fixed_uuid2())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .mode(ExecutionMode::Mapped)
        .outcome(Outcome::Complete)
        .backend_version("3.5.0")
        .build();

    let diff = diff_receipts(&passthrough, &mapped);
    assert!(!diff.is_empty());
    insta::assert_snapshot!(format_diff(&diff));
}
