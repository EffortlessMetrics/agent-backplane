// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deep tests for receipt construction, hashing, canonicalization, and verification.
//!
//! These complement `canonicalization_deep.rs` by focusing on construction
//! patterns, outcome variants, trace/artifact handling, serde roundtrips,
//! display formatting, timestamp handling, and usage token tracking.

use abp_core::ArtifactRef;
use abp_receipt::{
    AgentEvent, AgentEventKind, Outcome, Receipt, ReceiptBuilder, UsageNormalized,
    canonicalize, compute_hash, verify_hash,
};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<chrono::Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 10, 30, 0).unwrap()
}

fn base_builder() -> ReceiptBuilder {
    ReceiptBuilder::new("test-backend")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .outcome(Outcome::Complete)
}

// =========================================================================
// 1. Receipt construction: empty, with fields, with trace, with artifacts
// =========================================================================

#[test]
fn construct_minimal_receipt() {
    let r = ReceiptBuilder::new("minimal").build();
    assert_eq!(r.backend.id, "minimal");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn construct_receipt_with_all_fields() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("full")
        .run_id(Uuid::from_u128(42))
        .work_order_id(Uuid::from_u128(99))
        .started_at(ts)
        .finished_at(ts + chrono::Duration::seconds(5))
        .outcome(Outcome::Partial)
        .backend_version("2.1.0")
        .adapter_version("0.3.0")
        .model("gpt-4o")
        .dialect("openai")
        .usage_tokens(500, 1200)
        .build();
    assert_eq!(r.backend.id, "full");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.1.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.3.0"));
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.meta.run_id, Uuid::from_u128(42));
    assert_eq!(r.meta.work_order_id, Uuid::from_u128(99));
    assert_eq!(r.meta.duration_ms, 5000);
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(1200));
    assert_eq!(r.usage_raw["model"], "gpt-4o");
    assert_eq!(r.usage_raw["dialect"], "openai");
}

#[test]
fn construct_receipt_with_trace_events() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("traced")
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "begin".into(),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();
    assert_eq!(r.trace.len(), 3);
}

#[test]
fn construct_receipt_with_artifacts() {
    let r = ReceiptBuilder::new("artifacted")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 2);
    assert_eq!(r.artifacts[0].kind, "patch");
    assert_eq!(r.artifacts[1].path, "run.log");
}

// =========================================================================
// 2. Canonical JSON: BTreeMap keys sorted, deterministic output
// =========================================================================

#[test]
fn canonical_json_nested_keys_sorted() {
    let r = base_builder()
        .usage_raw(serde_json::json!({"zeta": 1, "alpha": 2, "mu": 3}))
        .build();
    let json = canonicalize(&r).unwrap();
    let alpha_pos = json.find("\"alpha\"").unwrap();
    let mu_pos = json.find("\"mu\"").unwrap();
    let zeta_pos = json.find("\"zeta\"").unwrap();
    assert!(alpha_pos < mu_pos);
    assert!(mu_pos < zeta_pos);
}

#[test]
fn canonical_json_deterministic_across_multiple_calls() {
    let r = base_builder()
        .model("claude-3")
        .usage_tokens(100, 200)
        .build();
    let results: Vec<String> = (0..10).map(|_| canonicalize(&r).unwrap()).collect();
    assert!(results.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn canonical_json_compact_no_whitespace() {
    let r = base_builder().build();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
    assert!(!json.contains('\t'));
    // Should not have indentation-style double spaces
    assert!(!json.contains("  "));
}

#[test]
fn canonical_json_meta_keys_sorted() {
    let r = base_builder().build();
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let meta = parsed["meta"].as_object().unwrap();
    let keys: Vec<&String> = meta.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

// =========================================================================
// 3. Hash computation: consistent SHA-256
// =========================================================================

#[test]
fn hash_is_64_char_lowercase_hex() {
    let r = base_builder().build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    // SHA-256 hex is lowercase
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn hash_matches_manual_sha256() {
    use sha2::{Digest, Sha256};

    let r = base_builder().build();
    let canonical = canonicalize(&r).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let manual = format!("{:x}", hasher.finalize());
    assert_eq!(compute_hash(&r).unwrap(), manual);
}

#[test]
fn hash_consistent_after_clone() {
    let r = base_builder().build();
    let cloned = r.clone();
    assert_eq!(compute_hash(&r).unwrap(), compute_hash(&cloned).unwrap());
}

// =========================================================================
// 4. Self-referential prevention: receipt_sha256 nullified before hashing
// =========================================================================

#[test]
fn hash_ignores_existing_receipt_sha256_none() {
    let r = base_builder().build();
    assert!(r.receipt_sha256.is_none());
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn hash_ignores_existing_receipt_sha256_some() {
    let mut r = base_builder().build();
    let h_before = compute_hash(&r).unwrap();
    r.receipt_sha256 = Some("0".repeat(64));
    let h_after = compute_hash(&r).unwrap();
    assert_eq!(h_before, h_after);
}

#[test]
fn canonical_form_never_contains_stored_hash() {
    let mut r = base_builder().build();
    r.receipt_sha256 = Some("abc123feedface".into());
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains("abc123feedface"));
    assert!(json.contains("\"receipt_sha256\":null"));
}

// =========================================================================
// 5. with_hash(): sets hash field, hash is valid
// =========================================================================

#[test]
fn builder_with_hash_sets_some() {
    let r = base_builder().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn builder_with_hash_value_is_64_hex() {
    let r = base_builder().with_hash().unwrap();
    let h = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn receipt_with_hash_method_equivalent() {
    let r1 = base_builder().build().with_hash().unwrap();
    let r2 = base_builder().with_hash().unwrap();
    // Both should produce the same hash for the same deterministic receipt
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

// =========================================================================
// 6. Hash verification: re-hash matches stored hash
// =========================================================================

#[test]
fn verify_hash_passes_with_correct_hash() {
    let r = base_builder().with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_passes_with_no_hash() {
    let r = base_builder().build();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_fails_with_tampered_outcome() {
    let mut r = base_builder().with_hash().unwrap();
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_with_tampered_backend() {
    let mut r = base_builder().with_hash().unwrap();
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_with_tampered_trace() {
    let mut r = base_builder().with_hash().unwrap();
    r.trace.push(AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::Warning {
            message: "injected".into(),
        },
        ext: None,
    });
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_with_garbage_hash() {
    let mut r = base_builder().build();
    r.receipt_sha256 = Some("not_a_valid_hash".into());
    assert!(!verify_hash(&r));
}

// =========================================================================
// 7. Hash stability: same receipt always produces same hash
// =========================================================================

#[test]
fn hash_stability_100_iterations() {
    let r = base_builder().model("stable-model").usage_tokens(10, 20).build();
    let reference = compute_hash(&r).unwrap();
    for _ in 0..100 {
        assert_eq!(compute_hash(&r).unwrap(), reference);
    }
}

#[test]
fn hash_stability_after_serde_roundtrip() {
    let r = base_builder().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

// =========================================================================
// 8. Different receipts: different content produces different hashes
// =========================================================================

#[test]
fn different_backend_ids_different_hash() {
    let r1 = base_builder().build();
    let r2 = ReceiptBuilder::new("other-backend")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_run_ids_different_hash() {
    let r1 = base_builder().build();
    let r2 = ReceiptBuilder::new("test-backend")
        .run_id(Uuid::from_u128(999))
        .work_order_id(Uuid::nil())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_usage_tokens_different_hash() {
    let r1 = base_builder().usage_tokens(100, 200).build();
    let r2 = base_builder().usage_tokens(100, 201).build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn with_vs_without_model_different_hash() {
    let r1 = base_builder().build();
    let r2 = base_builder().model("gpt-4").build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// =========================================================================
// 9. Receipt outcome variants: success, error, partial
// =========================================================================

#[test]
fn outcome_complete_builds_correctly() {
    let r = base_builder().outcome(Outcome::Complete).build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn outcome_failed_builds_correctly() {
    let r = base_builder().outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn outcome_partial_builds_correctly() {
    let r = base_builder().outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn all_three_outcomes_produce_distinct_hashes() {
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    let hashes: Vec<String> = outcomes
        .iter()
        .map(|o| {
            let r = base_builder().outcome(o.clone()).build();
            compute_hash(&r).unwrap()
        })
        .collect();
    let unique: std::collections::HashSet<&String> = hashes.iter().collect();
    assert_eq!(unique.len(), 3);
}

#[test]
fn outcome_serde_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let back: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, back);
    }
}

#[test]
fn error_helper_sets_outcome_and_trace() {
    let r = ReceiptBuilder::new("x").error("kaboom").build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(r.trace.len(), 1);
    assert!(matches!(
        &r.trace[0].kind,
        AgentEventKind::Error { message, .. } if message == "kaboom"
    ));
}

// =========================================================================
// 10. Trace events: ordered list of trace events in receipt
// =========================================================================

#[test]
fn trace_preserves_insertion_order() {
    let ts = fixed_ts();
    let r = base_builder()
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "first".into(),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta {
                text: "second".into(),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunCompleted {
                message: "third".into(),
            },
            ext: None,
        })
        .build();
    assert_eq!(r.trace.len(), 3);
    assert!(matches!(
        &r.trace[0].kind,
        AgentEventKind::RunStarted { message } if message == "first"
    ));
    assert!(matches!(
        &r.trace[2].kind,
        AgentEventKind::RunCompleted { message } if message == "third"
    ));
}

#[test]
fn trace_events_replace_with_events_method() {
    let ts = fixed_ts();
    let evt = AgentEvent {
        ts,
        kind: AgentEventKind::Warning {
            message: "warn".into(),
        },
        ext: None,
    };
    let r = base_builder()
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "will be replaced".into(),
            },
            ext: None,
        })
        .events(vec![evt])
        .build();
    assert_eq!(r.trace.len(), 1);
    assert!(matches!(&r.trace[0].kind, AgentEventKind::Warning { .. }));
}

#[test]
fn trace_with_tool_call_and_result() {
    let ts = fixed_ts();
    let r = base_builder()
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("tc-1".into()),
                output: serde_json::json!("fn main() {}"),
                is_error: false,
            },
            ext: None,
        })
        .build();
    assert_eq!(r.trace.len(), 2);
}

#[test]
fn trace_with_ext_data() {
    let ts = fixed_ts();
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor": "data"}),
    );
    let r = base_builder()
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta {
                text: "token".into(),
            },
            ext: Some(ext),
        })
        .build();
    assert!(r.trace[0].ext.is_some());
    let ext = r.trace[0].ext.as_ref().unwrap();
    assert!(ext.contains_key("raw_message"));
}

#[test]
fn trace_affects_hash() {
    let ts = fixed_ts();
    let r_no_trace = base_builder().build();
    let r_with_trace = base_builder()
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::AssistantDelta {
                text: "hi".into(),
            },
            ext: None,
        })
        .build();
    assert_ne!(
        compute_hash(&r_no_trace).unwrap(),
        compute_hash(&r_with_trace).unwrap()
    );
}

// =========================================================================
// 11. Artifacts: file artifacts in receipt
// =========================================================================

#[test]
fn artifacts_stored_in_order() {
    let r = base_builder()
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "first.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "second.log".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 2);
    assert_eq!(r.artifacts[0].path, "first.patch");
    assert_eq!(r.artifacts[1].path, "second.log");
}

#[test]
fn artifacts_affect_hash() {
    let r_no_art = base_builder().build();
    let r_with_art = base_builder()
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "file.patch".into(),
        })
        .build();
    assert_ne!(
        compute_hash(&r_no_art).unwrap(),
        compute_hash(&r_with_art).unwrap()
    );
}

#[test]
fn artifacts_survive_hash_and_verify() {
    let r = base_builder()
        .add_artifact(ArtifactRef {
            kind: "diff".into(),
            path: "changes.diff".into(),
        })
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.artifacts.len(), 1);
}

// =========================================================================
// 12. Usage tokens: token usage tracking
// =========================================================================

#[test]
fn usage_tokens_set_correctly() {
    let r = base_builder().usage_tokens(1000, 2000).build();
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(2000));
}

#[test]
fn usage_normalized_full_fields() {
    let u = UsageNormalized {
        input_tokens: Some(500),
        output_tokens: Some(1500),
        cache_read_tokens: Some(100),
        cache_write_tokens: Some(50),
        request_units: Some(3),
        estimated_cost_usd: Some(0.015),
    };
    let r = base_builder().usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(1500));
    assert_eq!(r.usage.cache_read_tokens, Some(100));
    assert_eq!(r.usage.cache_write_tokens, Some(50));
    assert_eq!(r.usage.request_units, Some(3));
    assert_eq!(r.usage.estimated_cost_usd, Some(0.015));
}

#[test]
fn usage_tokens_affect_hash() {
    let r_none = base_builder().build();
    let r_some = base_builder().usage_tokens(42, 84).build();
    assert_ne!(
        compute_hash(&r_none).unwrap(),
        compute_hash(&r_some).unwrap()
    );
}

#[test]
fn usage_default_is_all_none() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

// =========================================================================
// 13. Serde roundtrip: Receipt survives JSON roundtrip with hash intact
// =========================================================================

#[test]
fn serde_roundtrip_preserves_hash() {
    let r = base_builder().with_hash().unwrap();
    let json = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert!(verify_hash(&r2));
}

#[test]
fn serde_roundtrip_preserves_all_fields() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("roundtrip-be")
        .run_id(Uuid::from_u128(77))
        .work_order_id(Uuid::from_u128(88))
        .started_at(ts)
        .finished_at(ts + chrono::Duration::seconds(3))
        .outcome(Outcome::Partial)
        .backend_version("v1")
        .adapter_version("v2")
        .model("llama")
        .usage_tokens(300, 600)
        .add_event(AgentEvent {
            ts,
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "out.patch".into(),
        })
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.meta.work_order_id, r2.meta.work_order_id);
    assert_eq!(r.meta.duration_ms, r2.meta.duration_ms);
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.backend.backend_version, r2.backend.backend_version);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.usage.input_tokens, r2.usage.input_tokens);
    assert_eq!(r.trace.len(), r2.trace.len());
    assert_eq!(r.artifacts.len(), r2.artifacts.len());
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert!(verify_hash(&r2));
}

#[test]
fn serde_roundtrip_compact_bytes() {
    let r = base_builder()
        .usage_tokens(10, 20)
        .with_hash()
        .unwrap();
    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert!(verify_hash(&r2));
}

#[test]
fn serde_roundtrip_json_format() {
    let r = base_builder().with_hash().unwrap();
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert!(verify_hash(&r2));
}

#[test]
fn serde_roundtrip_hash_recompute_matches() {
    let r = base_builder()
        .model("test-model")
        .usage_tokens(50, 75)
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let recomputed = compute_hash(&r2).unwrap();
    assert_eq!(r2.receipt_sha256.as_ref().unwrap(), &recomputed);
}

// =========================================================================
// 14. Receipt display: Debug/Display formatting
// =========================================================================

#[test]
fn receipt_debug_contains_backend_id() {
    let r = base_builder().build();
    let debug = format!("{r:?}");
    assert!(debug.contains("test-backend"));
}

#[test]
fn receipt_debug_contains_outcome() {
    let r = base_builder().outcome(Outcome::Failed).build();
    let debug = format!("{r:?}");
    assert!(debug.contains("Failed"));
}

#[test]
fn outcome_debug_variants() {
    assert!(format!("{:?}", Outcome::Complete).contains("Complete"));
    assert!(format!("{:?}", Outcome::Partial).contains("Partial"));
    assert!(format!("{:?}", Outcome::Failed).contains("Failed"));
}

#[test]
fn verification_result_display_verified() {
    use abp_receipt::verify::verify_receipt;
    let r = base_builder().with_hash().unwrap();
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("verified"));
}

#[test]
fn verification_result_display_failed() {
    use abp_receipt::verify::verify_receipt;
    let mut r = base_builder().with_hash().unwrap();
    r.backend.id = "tampered".into();
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("failed"));
}

#[test]
fn audit_report_display() {
    use abp_receipt::verify::ReceiptAuditor;
    let auditor = ReceiptAuditor::new();
    let r = base_builder().with_hash().unwrap();
    let report = auditor.audit_batch(&[r]);
    let display = format!("{report}");
    assert!(display.contains("total: 1"));
    assert!(display.contains("valid: 1"));
}

// =========================================================================
// 15. Timestamp handling: created_at, completed_at fields
// =========================================================================

#[test]
fn timestamps_started_equals_finished_zero_duration() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("ts")
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_eq!(r.meta.started_at, ts);
    assert_eq!(r.meta.finished_at, ts);
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn timestamps_duration_computed_from_diff() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 7).unwrap();
    let r = ReceiptBuilder::new("ts")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert_eq!(r.meta.duration_ms, 7000);
}

#[test]
fn timestamps_duration_builder_method() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("ts")
        .started_at(ts)
        .duration(std::time::Duration::from_millis(1500))
        .build();
    assert_eq!(r.meta.duration_ms, 1500);
    assert_eq!(
        r.meta.finished_at,
        ts + chrono::Duration::milliseconds(1500)
    );
}

#[test]
fn timestamps_affect_hash() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
    let r1 = ReceiptBuilder::new("ts")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t1)
        .finished_at(t1)
        .build();
    let r2 = ReceiptBuilder::new("ts")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t2)
        .finished_at(t2)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn timestamps_contract_version_present() {
    let r = base_builder().build();
    assert_eq!(r.meta.contract_version, abp_receipt::CONTRACT_VERSION);
}

#[test]
fn timestamps_serde_roundtrip_preserves_precision() {
    let ts = Utc.with_ymd_and_hms(2025, 3, 15, 8, 45, 30).unwrap();
    let r = ReceiptBuilder::new("ts")
        .started_at(ts)
        .finished_at(ts + chrono::Duration::milliseconds(123))
        .with_hash()
        .unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.meta.started_at, r2.meta.started_at);
    assert_eq!(r.meta.finished_at, r2.meta.finished_at);
    assert_eq!(r.meta.duration_ms, r2.meta.duration_ms);
}
