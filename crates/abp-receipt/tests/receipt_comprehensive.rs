#![allow(clippy::all)]
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

//! Comprehensive tests for the `abp-receipt` crate covering builder, hashing,
//! serialization, chain, validation, verification, diffing, store, and edge cases.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::ArtifactRef;
use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_receipt::verify::{ReceiptAuditor, verify_receipt};
use abp_receipt::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, ExecutionMode, Outcome, ReceiptBuilder,
    ReceiptChain, ReceiptValidator, UsageNormalized, VerificationReport, canonicalize,
    compute_hash, diff_receipts, verify_hash,
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

// ═══════════════════════════════════════════════════════════════════════
// 3. Receipt serialization — JSON roundtrip, schema, BTreeMap ordering
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

// ═══════════════════════════════════════════════════════════════════════
// 4. Receipt chain — sequence, ordering, verification
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
    assert!(matches!(
        chain.verify(),
        Err(abp_receipt::ChainError::EmptyChain)
    ));
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
        Err(abp_receipt::ChainError::DuplicateId { .. })
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
    assert!(matches!(
        chain.push(r2),
        Err(abp_receipt::ChainError::BrokenLink { .. })
    ));
}

#[test]
fn chain_rejects_tampered_hash() {
    let mut chain = ReceiptChain::new();
    let mut r = ReceiptBuilder::new("b").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
    assert!(matches!(
        chain.push(r),
        Err(abp_receipt::ChainError::HashMismatch { .. })
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

// ═══════════════════════════════════════════════════════════════════════
// 5. Receipt outcomes — Complete, Failed, Partial
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
    // Failed outcome with non-empty trace that has no error events
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

// ═══════════════════════════════════════════════════════════════════════
// 6. Receipt trace — event ordering, event types
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
    match &r.trace[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "read_file"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
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
// 7. Receipt timing — started_at, completed_at, duration
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
    r.meta.duration_ms = 999_999; // bogus
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.duration_ms"));
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Receipt metadata — backend_id, contract_version, work_order_id
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
    r.meta.duration_ms = 12345; // inconsistent
    let result = verify_receipt(&r);
    assert!(!result.timestamps_valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Receipt diff, audit, store
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

// ═══════════════════════════════════════════════════════════════════════
// 10. Edge cases — empty trace, large trace, unicode, special chars
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
    match &r2.trace[0].kind {
        AgentEventKind::AssistantMessage { text } => {
            assert!(text.contains("你好"));
        }
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn edge_empty_backend_id_hashes() {
    let r = ReceiptBuilder::new("").build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_very_long_backend_id() {
    let long_id = "a".repeat(10_000);
    let r = ReceiptBuilder::new(&long_id).build();
    assert_eq!(r.backend.id.len(), 10_000);
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_special_chars_in_error_message() {
    let r = ReceiptBuilder::new("b")
        .error("line1\nline2\ttab \"quotes\" \\backslash")
        .build();
    let json = abp_receipt::serde_formats::to_json(&r).unwrap();
    let r2 = abp_receipt::serde_formats::from_json(&json).unwrap();
    assert_eq!(r2.outcome, Outcome::Failed);
    assert_eq!(r2.trace.len(), 1);
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
    assert_eq!(
        abp_receipt::ChainError::EmptyChain.to_string(),
        "chain is empty"
    );
    assert_eq!(
        abp_receipt::ChainError::HashMismatch { index: 5 }.to_string(),
        "hash mismatch at chain index 5"
    );
    assert_eq!(
        abp_receipt::ChainError::BrokenLink { index: 2 }.to_string(),
        "broken link at chain index 2"
    );
    let id = Uuid::nil();
    assert!(
        abp_receipt::ChainError::DuplicateId { id }
            .to_string()
            .contains("duplicate")
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
fn edge_audit_report_display() {
    let auditor = ReceiptAuditor::new();
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    let report = auditor.audit_batch(&[r]);
    let display = format!("{report}");
    assert!(display.contains("total: 1"));
}

#[test]
fn edge_store_get_missing_returns_none() {
    let store = InMemoryReceiptStore::new();
    assert!(store.get(Uuid::new_v4()).unwrap().is_none());
}

#[test]
fn edge_capabilities_in_receipt() {
    use abp_core::{Capability, SupportLevel};
    let mut caps = BTreeMap::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    let r = ReceiptBuilder::new("b").capabilities(caps).build();
    assert_eq!(r.capabilities.len(), 1);
}
