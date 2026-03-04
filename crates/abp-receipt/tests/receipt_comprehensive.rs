#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive tests for the `abp-receipt` crate covering builder, hashing,
//! serialization, chain, validation, verification, diffing, store, and edge cases.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::ArtifactRef;
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_receipt::verify::{ReceiptAuditor, verify_receipt};
use abp_receipt::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, ChainBuilder, ChainError, ExecutionMode, Outcome,
    Receipt, ReceiptBuilder, ReceiptChain, ReceiptValidator, UsageNormalized, VerificationReport,
    canonicalize, compute_hash, diff_receipts, verify_hash,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════
// 1. ReceiptBuilder — all builder methods, defaults, chaining
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builder_new_sets_backend_id() {
    let r = ReceiptBuilder::new("my-backend").build();
    assert_eq!(r.backend.id, "my-backend");
}

#[test]
fn builder_defaults_outcome_complete() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[test]
fn builder_defaults_no_hash() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn builder_defaults_empty_trace() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.trace.is_empty());
}

#[test]
fn builder_defaults_empty_artifacts() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.artifacts.is_empty());
}

#[test]
fn builder_defaults_contract_version() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn builder_defaults_nil_work_order_id() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.meta.work_order_id, Uuid::nil());
}

#[test]
fn builder_defaults_mapped_mode() {
    let r = ReceiptBuilder::new("b").build();
    assert!(matches!(r.mode, ExecutionMode::Mapped));
}

#[test]
fn builder_outcome_failed() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn builder_outcome_partial() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn builder_backend_alias_overrides_id() {
    let r = ReceiptBuilder::new("original").backend("replaced").build();
    assert_eq!(r.backend.id, "replaced");
}

#[test]
fn builder_backend_id_overrides() {
    let r = ReceiptBuilder::new("a").backend_id("b").build();
    assert_eq!(r.backend.id, "b");
}

#[test]
fn builder_backend_version_sets_field() {
    let r = ReceiptBuilder::new("b").backend_version("2.0").build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0"));
}

#[test]
fn builder_adapter_version_sets_field() {
    let r = ReceiptBuilder::new("b").adapter_version("0.5").build();
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5"));
}

#[test]
fn builder_model_merges_into_usage_raw() {
    let r = ReceiptBuilder::new("b")
        .usage_raw(serde_json::json!({"extra": 1}))
        .model("gpt-4o")
        .build();
    assert_eq!(r.usage_raw["model"], "gpt-4o");
    assert_eq!(r.usage_raw["extra"], 1);
}

#[test]
fn builder_dialect_merges_into_usage_raw() {
    let r = ReceiptBuilder::new("b").dialect("openai").build();
    assert_eq!(r.usage_raw["dialect"], "openai");
}

#[test]
fn builder_started_at_finished_at() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 10).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert_eq!(r.meta.started_at, t1);
    assert_eq!(r.meta.finished_at, t2);
    assert_eq!(r.meta.duration_ms, 10_000);
}

#[test]
fn builder_duration_adjusts_finished_at() {
    let ts = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(ts)
        .duration(Duration::from_millis(3500))
        .build();
    assert_eq!(r.meta.duration_ms, 3500);
}

#[test]
fn builder_work_order_id_sets_meta() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("b").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn builder_run_id_sets_meta() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("b").run_id(id).build();
    assert_eq!(r.meta.run_id, id);
}

#[test]
fn builder_mode_passthrough() {
    let r = ReceiptBuilder::new("b")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert!(matches!(r.mode, ExecutionMode::Passthrough));
}

#[test]
fn builder_usage_raw_sets_payload() {
    let r = ReceiptBuilder::new("b")
        .usage_raw(serde_json::json!({"tokens": 42}))
        .build();
    assert_eq!(r.usage_raw["tokens"], 42);
}

#[test]
fn builder_usage_sets_normalized() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("b").usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
}

#[test]
fn builder_usage_tokens_shorthand() {
    let r = ReceiptBuilder::new("b").usage_tokens(500, 1000).build();
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(1000));
}

#[test]
fn builder_verification_report() {
    let v = VerificationReport {
        git_diff: Some("diff --git a/f".into()),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("b").verification(v).build();
    assert!(r.verification.git_diff.is_some());
}

#[test]
fn builder_add_event_appends() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_event(evt).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_add_trace_event_alias() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_trace_event(evt).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_events_replaces_existing_trace() {
    let evt1 = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "a".into(),
        },
        ext: None,
    };
    let evt2 = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "b".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b")
        .add_event(evt1)
        .events(vec![evt2])
        .build();
    assert_eq!(r.trace.len(), 1);
    assert!(matches!(
        r.trace[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn builder_add_artifact_appends() {
    let art = ArtifactRef {
        kind: "patch".into(),
        path: "a.patch".into(),
    };
    let r = ReceiptBuilder::new("b").add_artifact(art).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn builder_error_sets_outcome_and_trace() {
    let r = ReceiptBuilder::new("b").error("boom").build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(r.trace.len(), 1);
    match &r.trace[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "boom"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn builder_chaining_multiple_methods() {
    let ts = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("chain-test")
        .outcome(Outcome::Partial)
        .backend_version("1.0")
        .adapter_version("0.2")
        .model("claude-3")
        .dialect("anthropic")
        .started_at(ts)
        .duration(Duration::from_secs(5))
        .work_order_id(id)
        .mode(ExecutionMode::Passthrough)
        .usage_tokens(100, 200)
        .build();
    assert_eq!(r.backend.id, "chain-test");
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.meta.work_order_id, id);
    assert_eq!(r.meta.duration_ms, 5000);
    assert!(matches!(r.mode, ExecutionMode::Passthrough));
}

#[test]
fn builder_with_hash_produces_valid_receipt() {
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Receipt hashing — deterministic, content-sensitive, self-referential
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn hash_is_64_hex_chars() {
    let r = ReceiptBuilder::new("b").build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_deterministic_across_calls() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Complete).build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    let h3 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn hash_changes_with_outcome_change() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Complete).build();
    let mut r2 = r.clone();
    r2.outcome = Outcome::Failed;
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_backend_change() {
    let r = ReceiptBuilder::new("a").build();
    let mut r2 = r.clone();
    r2.backend.id = "b".into();
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_trace_change() {
    let r = ReceiptBuilder::new("b").build();
    let mut r2 = r.clone();
    r2.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "hi".into(),
        },
        ext: None,
    });
    assert_ne!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_usage_tokens() {
    let id = Uuid::nil();
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .usage_tokens(100, 200)
        .build();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .usage_tokens(100, 201)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn self_referential_prevention_hash_excluded() {
    // canonicalize() sets receipt_sha256 to null before hashing
    let r1 = ReceiptBuilder::new("b").build();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("anything".into());
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

#[test]
fn with_hash_vs_manual_hash_produces_same_result() {
    // Build without hash, manually compute, then compare with with_hash()
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let id = Uuid::nil();
    let wo_id = Uuid::nil();
    let r_no_hash = ReceiptBuilder::new("mock")
        .run_id(id)
        .work_order_id(wo_id)
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Complete)
        .build();
    let manual_hash = compute_hash(&r_no_hash).unwrap();

    let r_with_hash = ReceiptBuilder::new("mock")
        .run_id(id)
        .work_order_id(wo_id)
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert_eq!(
        r_with_hash.receipt_sha256.as_deref(),
        Some(manual_hash.as_str())
    );
}

#[test]
fn with_hash_then_verify_roundtrip() {
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Failed)
        .error("fail")
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    // Recompute matches stored
    let recomputed = compute_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_deref().unwrap(), recomputed);
}

#[test]
fn hash_independent_of_existing_hash_value() {
    // compute_hash should return same value regardless of receipt_sha256 content
    let r = ReceiptBuilder::new("b").build();
    let h1 = compute_hash(&r).unwrap();
    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("deadbeef".into());
    let h2 = compute_hash(&r2).unwrap();
    let mut r3 = r.clone();
    r3.receipt_sha256 = Some(h1.clone());
    let h3 = compute_hash(&r3).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn verify_hash_passes_when_none() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_passes_when_correct() {
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_fails_when_tampered() {
    let mut r = ReceiptBuilder::new("b").with_hash().unwrap();
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_with_garbage_hash() {
    let mut r = ReceiptBuilder::new("b").build();
    r.receipt_sha256 = Some("not_a_real_hash".into());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_when_backend_id_tampered() {
    let mut r = ReceiptBuilder::new("good").with_hash().unwrap();
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_when_timestamp_tampered() {
    let mut r = ReceiptBuilder::new("b").with_hash().unwrap();
    r.meta.duration_ms += 1;
    assert!(!verify_hash(&r));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Receipt serialization — JSON roundtrip, bytes, canonical form
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn json_roundtrip_preserves_fields() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(10, 20)
        .with_hash()
        .unwrap();
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r.usage.input_tokens, r2.usage.input_tokens);
}

#[test]
fn bytes_roundtrip_preserves_fields() {
    let r = ReceiptBuilder::new("mock")
        .model("gpt-4")
        .with_hash()
        .unwrap();
    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.usage_raw["model"], r2.usage_raw["model"]);
}

#[test]
fn canonical_json_is_compact_no_newlines() {
    let r = ReceiptBuilder::new("b").build();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
}

#[test]
fn canonical_json_nullifies_hash_field() {
    let mut r = ReceiptBuilder::new("b").build();
    r.receipt_sha256 = Some("deadbeef".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn compact_bytes_smaller_than_pretty_json() {
    let r = ReceiptBuilder::new("b").build();
    let pretty = abp_receipt::serde_formats::to_json(&r).unwrap();
    let compact = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    assert!(compact.len() < pretty.len());
}

#[test]
fn serialized_receipt_contains_contract_version() {
    let r = ReceiptBuilder::new("b").build();
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn json_roundtrip_with_trace_events() {
    let evt = AgentEvent {
        ts: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc1".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/tmp/test"}),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_event(evt).build();
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r2.trace.len(), 1);
    match &r2.trace[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "read_file"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn json_roundtrip_preserves_hash_after_deserialization() {
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2: Receipt = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn bytes_roundtrip_preserves_hash_verification() {
    let r = ReceiptBuilder::new("b")
        .usage_tokens(50, 100)
        .with_hash()
        .unwrap();
    let bytes = abp_receipt::serde_formats::to_bytes(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_bytes(&bytes).unwrap();
    assert!(verify_hash(&r2));
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn json_roundtrip_with_all_outcome_types() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = ReceiptBuilder::new("b").outcome(outcome.clone()).build();
        let json = abp_receipt::serde_formats::to_json(&r).unwrap();
        let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
        assert_eq!(r2.outcome, outcome);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Receipt chain — basic operations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_new_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert!(chain.latest().is_none());
}

#[test]
fn chain_push_increments_len() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("b").with_hash().unwrap())
        .unwrap();
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

#[test]
fn chain_verify_empty_returns_error() {
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify(), Err(ChainError::EmptyChain)));
}

#[test]
fn chain_verify_single_valid() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("b").with_hash().unwrap())
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_multiple_receipts_in_order() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 1, 3, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("c")
                .started_at(ts3)
                .finished_at(ts3)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_rejects_duplicate_run_id() {
    let mut chain = ReceiptChain::new();
    let id = Uuid::new_v4();
    chain
        .push(ReceiptBuilder::new("a").run_id(id).with_hash().unwrap())
        .unwrap();
    let r2 = ReceiptBuilder::new("b").run_id(id).with_hash().unwrap();
    assert!(matches!(
        chain.push(r2),
        Err(ChainError::DuplicateId { .. })
    ));
}

#[test]
fn chain_rejects_out_of_order() {
    let mut chain = ReceiptChain::new();
    let ts_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let ts_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts_later)
                .finished_at(ts_later)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .started_at(ts_earlier)
        .finished_at(ts_earlier)
        .with_hash()
        .unwrap();
    assert!(matches!(chain.push(r2), Err(ChainError::BrokenLink { .. })));
}

#[test]
fn chain_rejects_tampered_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = ReceiptBuilder::new("b").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_latest_returns_last() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("first")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("second")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.latest().unwrap().backend.id, "second");
}

#[test]
fn chain_iter_yields_all_in_order() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("x")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("y")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let ids: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(ids, vec!["x", "y"]);
}

#[test]
fn chain_into_iter_works() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("b").with_hash().unwrap())
        .unwrap();
    let count = (&chain).into_iter().count();
    assert_eq!(count, 1);
}

#[test]
fn chain_get_returns_receipt_by_index() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("first")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("second")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.get(0).unwrap().backend.id, "first");
    assert_eq!(chain.get(1).unwrap().backend.id, "second");
    assert!(chain.get(2).is_none());
}

#[test]
fn chain_sequence_at_returns_correct_sequence() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.sequence_at(0), Some(0));
    assert_eq!(chain.sequence_at(1), Some(1));
    assert_eq!(chain.sequence_at(2), None);
}

#[test]
fn chain_parent_hash_at_first_is_none() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("b").with_hash().unwrap())
        .unwrap();
    assert!(chain.parent_hash_at(0).is_none());
}

#[test]
fn chain_parent_hash_at_second_matches_first_hash() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let r1 = ReceiptBuilder::new("a")
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();
    let first_hash = r1.receipt_sha256.clone().unwrap();
    chain.push(r1).unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.parent_hash_at(1).unwrap(), first_hash);
}

#[test]
fn chain_as_slice_returns_all() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("b").with_hash().unwrap())
        .unwrap();
    let slice = chain.as_slice();
    assert_eq!(slice.len(), 1);
    assert_eq!(slice[0].backend.id, "b");
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Chain — verify_chain, detect_tampering, find_gaps, summary
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_verify_chain_empty_returns_error() {
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify_chain(), Err(ChainError::EmptyChain)));
}

#[test]
fn chain_verify_chain_valid_chain_passes() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_detect_tampering_clean_chain_returns_empty() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("b").with_hash().unwrap())
        .unwrap();
    let evidence = chain.detect_tampering();
    assert!(evidence.is_empty());
}

#[test]
fn chain_find_gaps_contiguous_returns_empty() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert!(chain.find_gaps().is_empty());
}

#[test]
fn chain_summary_counts_outcomes() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .outcome(Outcome::Complete)
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .outcome(Outcome::Failed)
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("c")
                .outcome(Outcome::Partial)
                .started_at(ts3)
                .finished_at(ts3)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let summary = chain.chain_summary();
    assert_eq!(summary.total_receipts, 3);
    assert_eq!(summary.complete_count, 1);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.partial_count, 1);
    assert!(summary.all_hashes_valid);
    assert_eq!(summary.gap_count, 0);
}

#[test]
fn chain_summary_aggregates_tokens() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .usage_tokens(100, 200)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .usage_tokens(300, 400)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let summary = chain.chain_summary();
    assert_eq!(summary.total_input_tokens, 400);
    assert_eq!(summary.total_output_tokens, 600);
}

#[test]
fn chain_summary_collects_backends() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("alpha")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("beta")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let summary = chain.chain_summary();
    assert_eq!(summary.backends.len(), 2);
    assert!(summary.backends.contains(&"alpha".to_string()));
    assert!(summary.backends.contains(&"beta".to_string()));
}

#[test]
fn chain_summary_tracks_time_range() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 12, 30, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let summary = chain.chain_summary();
    assert_eq!(summary.first_started_at.unwrap(), ts1);
    assert_eq!(summary.last_finished_at.unwrap(), ts2);
}

#[test]
fn chain_summary_total_duration() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts1_end = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let ts2_end = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 10).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1_end)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2_end)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let summary = chain.chain_summary();
    assert_eq!(summary.total_duration_ms, 15_000);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. ChainBuilder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn chain_builder_builds_valid_chain() {
    let chain = ChainBuilder::new()
        .append(ReceiptBuilder::new("b").with_hash().unwrap())
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_builder_multiple_receipts() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let chain = ChainBuilder::new()
        .append(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap()
        .append(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap()
        .build();
    assert_eq!(chain.len(), 2);
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_builder_skip_validation_allows_unhashed() {
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(ReceiptBuilder::new("b").build())
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_append_with_sequence_creates_gap() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let chain = ChainBuilder::new()
        .append_with_sequence(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
            0,
        )
        .unwrap()
        .append_with_sequence(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
            5,
        )
        .unwrap()
        .build();
    assert_eq!(chain.len(), 2);
    let gaps = chain.find_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].expected, 1);
    assert_eq!(gaps[0].actual, 5);
}

#[test]
fn chain_builder_default_is_new() {
    let chain = ChainBuilder::default().build();
    assert!(chain.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Receipt outcomes — Complete, Failed, Partial
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn outcome_complete_verifies_cleanly() {
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let result = verify_receipt(&r);
    assert!(result.is_verified());
}

#[test]
fn outcome_failed_with_error_event_is_consistent() {
    let r = ReceiptBuilder::new("b").error("something broke").build();
    let result = verify_receipt(&r);
    assert!(result.outcome_consistent);
}

#[test]
fn outcome_failed_without_error_event_is_inconsistent() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Failed)
        .add_event(evt)
        .build();
    let result = verify_receipt(&r);
    assert!(!result.outcome_consistent);
}

#[test]
fn outcome_complete_with_error_event_is_inconsistent() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Complete)
        .add_event(evt)
        .build();
    let result = verify_receipt(&r);
    assert!(!result.outcome_consistent);
}

#[test]
fn outcome_partial_hashes_differently_from_complete() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let id = Uuid::nil();
    let r1 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Complete)
        .build();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Partial)
        .build();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn outcome_failed_with_empty_trace_is_consistent() {
    // Failed outcome with empty trace should be consistent (no events to check)
    let r = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    let result = verify_receipt(&r);
    assert!(result.outcome_consistent);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Receipt trace — event ordering, event types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn trace_preserves_event_order() {
    let now = Utc::now();
    let events = vec![
        AgentEvent {
            ts: now,
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
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
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        },
    ];
    let r = ReceiptBuilder::new("b").events(events).build();
    assert_eq!(r.trace.len(), 3);
    assert!(matches!(r.trace[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        r.trace[1].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(
        r.trace[2].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn trace_tool_call_and_result() {
    let now = Utc::now();
    let events = vec![
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "/a.rs"}),
            },
            ext: None,
        },
        AgentEvent {
            ts: now,
            kind: AgentEventKind::ToolResult {
                tool_name: "read_file".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        },
    ];
    let r = ReceiptBuilder::new("b").events(events).build();
    assert_eq!(r.trace.len(), 2);
}

#[test]
fn trace_file_changed_event() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added main function".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_event(evt).build();
    match &r.trace[0].kind {
        AgentEventKind::FileChanged { path, summary } => {
            assert_eq!(path, "src/main.rs");
            assert_eq!(summary, "added main function");
        }
        other => panic!("expected FileChanged, got {other:?}"),
    }
}

#[test]
fn trace_command_executed_event() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("test result: ok".into()),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_event(evt).build();
    match &r.trace[0].kind {
        AgentEventKind::CommandExecuted {
            command, exit_code, ..
        } => {
            assert_eq!(command, "cargo test");
            assert_eq!(*exit_code, Some(0));
        }
        other => panic!("expected CommandExecuted, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Receipt timing — started_at, finished_at, duration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn timing_zero_duration_when_same_timestamps() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn timing_duration_calculated_correctly() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 30).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert_eq!(r.meta.duration_ms, 90_000);
}

#[test]
fn timing_duration_builder_method() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(ts)
        .duration(Duration::from_millis(1234))
        .build();
    assert_eq!(r.meta.duration_ms, 1234);
}

#[test]
fn timing_validator_catches_inconsistent_duration() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("b").build();
    r.meta.duration_ms = 999_999;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.duration_ms"));
}

#[test]
fn timing_sub_second_duration() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("b")
        .started_at(ts)
        .duration(Duration::from_millis(42))
        .build();
    assert_eq!(r.meta.duration_ms, 42);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Receipt metadata — backend_id, contract_version, work_order_id
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn metadata_contract_version_is_current() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn metadata_validator_rejects_wrong_contract_version() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("b").build();
    r.meta.contract_version = "wrong/v99".into();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn metadata_validator_rejects_empty_backend_id() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("b").build();
    r.backend.id = String::new();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "backend.id"));
}

#[test]
fn metadata_verification_checks_contract() {
    let mut r = ReceiptBuilder::new("b").build();
    r.meta.contract_version = "bad".into();
    let result = verify_receipt(&r);
    assert!(!result.contract_valid);
}

#[test]
fn metadata_verification_checks_timestamps() {
    let mut r = ReceiptBuilder::new("b").build();
    r.meta.duration_ms = 12345;
    let result = verify_receipt(&r);
    assert!(!result.timestamps_valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Receipt diff, audit, store
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn diff_identical_is_empty() {
    let r = ReceiptBuilder::new("b").build();
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn diff_detects_multiple_changes() {
    let a = ReceiptBuilder::new("a").outcome(Outcome::Complete).build();
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    b.backend.id = "b".into();
    let d = diff_receipts(&a, &b);
    assert!(d.len() >= 2);
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
}

#[test]
fn diff_detects_timestamp_change() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let a = ReceiptBuilder::new("b")
        .started_at(t1)
        .finished_at(t1)
        .build();
    let b = ReceiptBuilder::new("b")
        .started_at(t2)
        .finished_at(t2)
        .build();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "meta.started_at"));
}

#[test]
fn diff_detects_mode_change() {
    let a = ReceiptBuilder::new("b").mode(ExecutionMode::Mapped).build();
    let b = ReceiptBuilder::new("b")
        .mode(ExecutionMode::Passthrough)
        .build();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "mode"));
}

#[test]
fn diff_detects_usage_raw_change() {
    let a = ReceiptBuilder::new("b")
        .usage_raw(serde_json::json!({"a": 1}))
        .build();
    let mut b = a.clone();
    b.usage_raw = serde_json::json!({"a": 2});
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "usage_raw"));
}

#[test]
fn diff_detects_artifact_length_change() {
    let a = ReceiptBuilder::new("b").build();
    let b = ReceiptBuilder::new("b")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .build();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "artifacts.len"));
}

#[test]
fn auditor_clean_batch() {
    let auditor = ReceiptAuditor::new();
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    let report = auditor.audit_batch(&[r]);
    assert!(report.is_clean());
    assert_eq!(report.total, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.invalid, 0);
}

#[test]
fn auditor_detects_invalid_receipt() {
    let auditor = ReceiptAuditor::new();
    let mut r = ReceiptBuilder::new("b").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn auditor_detects_duplicate_run_ids() {
    let auditor = ReceiptAuditor::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a").run_id(id).build();
    let r2 = ReceiptBuilder::new("b").run_id(id).build();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(!report.is_clean());
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.description.contains("duplicate run_id"))
    );
}

#[test]
fn auditor_detects_overlapping_timeline() {
    let auditor = ReceiptAuditor::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts1_end = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 30).unwrap(); // overlaps
    let ts2_end = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 30).unwrap();
    let r1 = ReceiptBuilder::new("same-backend")
        .started_at(ts1)
        .finished_at(ts1_end)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("same-backend")
        .started_at(ts2)
        .finished_at(ts2_end)
        .with_hash()
        .unwrap();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.description.contains("overlapping"))
    );
}

#[test]
fn auditor_empty_batch_is_clean() {
    let auditor = ReceiptAuditor::new();
    let report = auditor.audit_batch(&[]);
    assert!(report.is_clean());
    assert_eq!(report.total, 0);
}

#[test]
fn store_put_and_get() {
    let mut store = InMemoryReceiptStore::new();
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    let id = r.meta.run_id;
    store.store(r).unwrap();
    assert_eq!(store.len(), 1);
    let got = store.get(id).unwrap().unwrap();
    assert_eq!(got.backend.id, "b");
}

#[test]
fn store_rejects_duplicate() {
    let mut store = InMemoryReceiptStore::new();
    let id = Uuid::new_v4();
    store
        .store(ReceiptBuilder::new("a").run_id(id).build())
        .unwrap();
    let result = store.store(ReceiptBuilder::new("b").run_id(id).build());
    assert!(result.is_err());
}

#[test]
fn store_filter_by_backend() {
    let mut store = InMemoryReceiptStore::new();
    store.store(ReceiptBuilder::new("alpha").build()).unwrap();
    store.store(ReceiptBuilder::new("beta").build()).unwrap();
    let filter = ReceiptFilter {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "alpha");
}

#[test]
fn store_filter_by_outcome() {
    let mut store = InMemoryReceiptStore::new();
    store
        .store(ReceiptBuilder::new("a").outcome(Outcome::Complete).build())
        .unwrap();
    store
        .store(ReceiptBuilder::new("b").outcome(Outcome::Failed).build())
        .unwrap();
    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn store_filter_by_time_range() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryReceiptStore::new();
    store
        .store(
            ReceiptBuilder::new("early")
                .started_at(t1)
                .finished_at(t1)
                .build(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("mid")
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();
    store
        .store(
            ReceiptBuilder::new("late")
                .started_at(t3)
                .finished_at(t3)
                .build(),
        )
        .unwrap();
    let filter = ReceiptFilter {
        after: Some(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()),
        before: Some(Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend_id, "mid");
}

#[test]
fn store_get_missing_returns_none() {
    let store = InMemoryReceiptStore::new();
    assert!(store.get(Uuid::new_v4()).unwrap().is_none());
}

#[test]
fn store_empty_initially() {
    let store = InMemoryReceiptStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Edge cases — empty fields, large receipts, unicode, special chars
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_trace_hashes_stably() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.trace.is_empty());
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn edge_large_trace_1000_events() {
    let mut builder = ReceiptBuilder::new("b");
    for i in 0..1000 {
        builder = builder.add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        });
    }
    let r = builder.with_hash().unwrap();
    assert_eq!(r.trace.len(), 1000);
    assert!(verify_hash(&r));
}

#[test]
fn edge_unicode_backend_id() {
    let r = ReceiptBuilder::new("バックエンド🚀").build();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("バックエンド🚀"));
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_unicode_in_trace_event() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "Héllo wörld 你好 🎉".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("b").add_event(evt).with_hash().unwrap();
    assert!(verify_hash(&r));
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r2.trace.len(), 1);
}

#[test]
fn edge_special_chars_in_error_message() {
    let r = ReceiptBuilder::new("b")
        .error("Error: \"quotes\" and \\backslashes\\ and\nnewlines")
        .build();
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r2.outcome, Outcome::Failed);
    assert_eq!(r2.trace.len(), 1);
}

#[test]
fn edge_empty_backend_id_hashes() {
    let r = ReceiptBuilder::new("").build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_very_long_backend_id() {
    let long_id = "x".repeat(10_000);
    let r = ReceiptBuilder::new(&long_id).with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.backend.id.len(), 10_000);
}

#[test]
fn edge_receipt_clone_hashes_same() {
    let r = ReceiptBuilder::new("b")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = r.clone();
    assert_eq!(compute_hash(&r).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn edge_validator_reports_multiple_errors() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("b").build();
    r.backend.id = String::new();
    r.meta.contract_version = "bad".into();
    r.meta.duration_ms = 99999;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.len() >= 3);
}

#[test]
fn edge_chain_error_display() {
    assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
    assert_eq!(
        ChainError::HashMismatch { index: 5 }.to_string(),
        "hash mismatch at chain index 5"
    );
    assert_eq!(
        ChainError::BrokenLink { index: 2 }.to_string(),
        "broken link at chain index 2"
    );
    let id = Uuid::nil();
    assert!(
        ChainError::DuplicateId { id }
            .to_string()
            .contains("duplicate")
    );
    assert!(
        ChainError::ParentMismatch { index: 1 }
            .to_string()
            .contains("parent hash mismatch")
    );
    assert!(
        ChainError::SequenceGap {
            expected: 1,
            actual: 5
        }
        .to_string()
        .contains("sequence gap")
    );
}

#[test]
fn edge_verification_result_display() {
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("verified"));
}

#[test]
fn edge_verification_result_display_failed() {
    let mut r = ReceiptBuilder::new("b").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
    let result = verify_receipt(&r);
    let display = format!("{result}");
    assert!(display.contains("failed"));
}

#[test]
fn edge_audit_report_display() {
    let auditor = ReceiptAuditor::new();
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    let report = auditor.audit_batch(&[r]);
    let display = format!("{report}");
    assert!(display.contains("total: 1"));
}

#[test]
fn edge_capabilities_in_receipt() {
    use abp_core::{Capability, SupportLevel};
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("b").capabilities(caps).build();
    assert_eq!(r.capabilities.len(), 1);
}

#[test]
fn edge_multiple_artifacts() {
    let r = ReceiptBuilder::new("b")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 2);
    assert_eq!(r.artifacts[0].kind, "patch");
    assert_eq!(r.artifacts[1].kind, "log");
}

#[test]
fn edge_receipt_summary_from_receipt() {
    use abp_receipt::store::ReceiptSummary;
    let r = ReceiptBuilder::new("test-be")
        .outcome(Outcome::Partial)
        .build();
    let summary = ReceiptSummary::from(&r);
    assert_eq!(summary.id, r.meta.run_id);
    assert_eq!(summary.backend_id, "test-be");
    assert_eq!(summary.outcome, Outcome::Partial);
}

#[test]
fn edge_validation_error_display() {
    use abp_receipt::ValidationError;
    let e = ValidationError {
        field: "test".into(),
        message: "oops".into(),
    };
    assert_eq!(e.to_string(), "test: oops");
}

#[test]
fn edge_audit_issue_display_with_index_and_run_id() {
    use abp_receipt::verify::AuditIssue;
    let issue = AuditIssue {
        receipt_index: Some(3),
        run_id: Some("abc-123".into()),
        description: "bad hash".into(),
    };
    let display = format!("{issue}");
    assert!(display.contains("#3"));
    assert!(display.contains("abc-123"));
    assert!(display.contains("bad hash"));
}

#[test]
fn edge_audit_issue_display_without_context() {
    use abp_receipt::verify::AuditIssue;
    let issue = AuditIssue {
        receipt_index: None,
        run_id: None,
        description: "global issue".into(),
    };
    assert_eq!(format!("{issue}"), "global issue");
}

#[test]
fn edge_tamper_evidence_display() {
    use abp_receipt::{TamperEvidence, TamperKind};
    let ev = TamperEvidence {
        index: 0,
        sequence: 0,
        kind: TamperKind::HashMismatch {
            stored: "aaa".into(),
            computed: "bbb".into(),
        },
    };
    let display = format!("{ev}");
    assert!(display.contains("index=0"));
    assert!(display.contains("aaa"));
}

#[test]
fn edge_chain_gap_display() {
    use abp_receipt::ChainGap;
    let gap = ChainGap {
        expected: 2,
        actual: 5,
        after_index: 1,
    };
    let display = format!("{gap}");
    assert!(display.contains("gap"));
    assert!(display.contains("expected seq 2"));
    assert!(display.contains("found 5"));
}

#[test]
fn edge_chain_serialization_roundtrip() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let json = serde_json::to_string(&chain).unwrap();
    let chain2: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(chain2.len(), 2);
    assert!(chain2.verify().is_ok());
}

#[test]
fn edge_hash_known_value_stability() {
    // Build receipt at fixed timestamp with fixed IDs to ensure hash is stable
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let fixed_id = Uuid::nil();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(fixed_id)
        .work_order_id(fixed_id)
        .started_at(ts)
        .finished_at(ts)
        .build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn edge_receipt_with_ext_field_in_event() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: {
            let mut map = BTreeMap::new();
            map.insert("custom_key".to_string(), serde_json::json!("custom_value"));
            Some(map)
        },
    };
    let r = ReceiptBuilder::new("b").add_event(evt).with_hash().unwrap();
    assert!(verify_hash(&r));
    assert!(r.trace[0].ext.is_some());
}

#[test]
fn edge_validator_passes_valid_receipt_without_hash() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("mock").build();
    assert!(v.validate(&r).is_ok());
}

#[test]
fn edge_validator_detects_tampered_hash() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.outcome = Outcome::Failed;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "receipt_sha256"));
}

#[test]
fn edge_validator_detects_bad_timestamps() {
    let v = ReceiptValidator::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let mut r = ReceiptBuilder::new("mock")
        .started_at(t1)
        .finished_at(t2)
        .build();
    r.meta.started_at = t1;
    r.meta.finished_at = t2;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.finished_at"));
}

#[test]
fn edge_chain_allows_same_timestamp_for_consecutive() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("a")
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    // Same timestamp should be allowed (not strictly before)
    chain
        .push(
            ReceiptBuilder::new("b")
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.len(), 2);
}
