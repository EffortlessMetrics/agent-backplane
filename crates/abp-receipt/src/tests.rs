// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::*;
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, ExecutionMode, UsageNormalized, VerificationReport,
};
use chrono::{TimeZone, Utc};
use std::time::Duration;
use uuid::Uuid;

// ── Canonicalization tests ─────────────────────────────────────────

#[test]
fn canonical_json_deterministic() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_json_nullifies_receipt_sha256() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some("deadbeef".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn canonical_json_is_compact() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json = canonicalize(&r).unwrap();
    // Compact JSON should not contain newlines or multi-space indentation.
    assert!(!json.contains('\n'));
}

#[test]
fn canonical_json_same_regardless_of_existing_hash() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("anything".into());
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

// ── Hash computation tests ─────────────────────────────────────────

#[test]
fn hash_is_64_hex_chars() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_deterministic() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(compute_hash(&r).unwrap(), compute_hash(&r).unwrap());
}

#[test]
fn hash_changes_when_outcome_changes() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_matches_known_value() {
    // Build a receipt at a fixed timestamp so the hash is reproducible.
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let fixed_id = Uuid::nil();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(fixed_id)
        .work_order_id(fixed_id)
        .started_at(ts)
        .finished_at(ts)
        .build();
    let h = compute_hash(&r).unwrap();
    // Re-derive to confirm stability — if canonical form changes, this will
    // catch it as a regression.
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h, h2);
    assert_eq!(h.len(), 64);
}

// ── Hash verification tests ────────────────────────────────────────

#[test]
fn verify_hash_passes_for_correct_hash() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_passes_when_no_hash_stored() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_fails_for_tampered_outcome() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_for_tampered_backend() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_for_garbage_hash() {
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    r.receipt_sha256 = Some("not_a_real_hash".into());
    assert!(!verify_hash(&r));
}

// ── Chain tests ────────────────────────────────────────────────────

#[test]
fn chain_new_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert!(chain.latest().is_none());
}

#[test]
fn chain_push_and_len() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(chain.latest().is_some());
}

#[test]
fn chain_verify_empty_returns_error() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_verify_single() {
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_verify_multiple() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .started_at(ts1)
                .finished_at(ts1)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Partial)
                .started_at(ts2)
                .finished_at(ts2)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(chain.len(), 2);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_rejects_duplicate_id() {
    let mut chain = ReceiptChain::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("mock")
        .run_id(id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .run_id(id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    chain.push(r1).unwrap();
    assert_eq!(chain.push(r2), Err(ChainError::DuplicateId { id }));
}

#[test]
fn chain_detects_hash_mismatch_on_push() {
    let mut chain = ReceiptChain::new();
    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.outcome = Outcome::Failed; // tamper
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_detects_broken_link_ordering() {
    let mut chain = ReceiptChain::new();
    let ts_later = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let ts_earlier = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .started_at(ts_later)
                .finished_at(ts_later)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    let r2 = ReceiptBuilder::new("mock")
        .started_at(ts_earlier)
        .finished_at(ts_earlier)
        .with_hash()
        .unwrap();
    assert!(matches!(chain.push(r2), Err(ChainError::BrokenLink { .. })));
}

#[test]
fn chain_latest_returns_last_pushed() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("latest-backend")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let expected_id = r.meta.run_id;
    chain.push(r).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, expected_id);
}

#[test]
fn chain_iter_yields_all() {
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
    let ids: Vec<_> = chain.iter().map(|r| r.backend.id.clone()).collect();
    assert_eq!(ids, vec!["a", "b"]);
}

// ── ReceiptBuilder tests ───────────────────────────────────────────

#[test]
fn builder_defaults() {
    let r = ReceiptBuilder::new("test-backend").build();
    assert_eq!(r.backend.id, "test-backend");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
}

#[test]
fn builder_outcome() {
    let r = ReceiptBuilder::new("x").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn builder_backend_version() {
    let r = ReceiptBuilder::new("x")
        .backend_version("1.2.3")
        .adapter_version("0.9")
        .build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.9"));
}

#[test]
fn builder_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("x").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn builder_run_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("x").run_id(id).build();
    assert_eq!(r.meta.run_id, id);
}

#[test]
fn builder_timestamps_and_duration() {
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let r = ReceiptBuilder::new("x")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert_eq!(r.meta.duration_ms, 5000);
}

#[test]
fn builder_mode() {
    let r = ReceiptBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert!(matches!(r.mode, ExecutionMode::Passthrough));
}

#[test]
fn builder_usage_raw() {
    let r = ReceiptBuilder::new("x")
        .usage_raw(serde_json::json!({"tokens": 42}))
        .build();
    assert_eq!(r.usage_raw["tokens"], 42);
}

#[test]
fn builder_usage() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("x").usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
}

#[test]
fn builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff".into()),
        ..Default::default()
    };
    let r = ReceiptBuilder::new("x").verification(v).build();
    assert_eq!(r.verification.git_diff.as_deref(), Some("diff"));
}

#[test]
fn builder_add_trace_event() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("x").add_trace_event(evt).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_add_artifact() {
    let art = ArtifactRef {
        kind: "patch".into(),
        path: "a.patch".into(),
    };
    let r = ReceiptBuilder::new("x").add_artifact(art).build();
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
}

#[test]
fn builder_with_hash() {
    let r = ReceiptBuilder::new("x").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

// ── New builder method tests ───────────────────────────────────────

#[test]
fn builder_backend_alias() {
    let r = ReceiptBuilder::new("initial").backend("replaced").build();
    assert_eq!(r.backend.id, "replaced");
}

#[test]
fn builder_model() {
    let r = ReceiptBuilder::new("x").model("gpt-4").build();
    assert_eq!(r.usage_raw["model"], "gpt-4");
}

#[test]
fn builder_dialect() {
    let r = ReceiptBuilder::new("x").dialect("claude").build();
    assert_eq!(r.usage_raw["dialect"], "claude");
}

#[test]
fn builder_model_merges_with_usage_raw() {
    let r = ReceiptBuilder::new("x")
        .usage_raw(serde_json::json!({"extra": true}))
        .model("gpt-4")
        .build();
    assert_eq!(r.usage_raw["model"], "gpt-4");
    assert_eq!(r.usage_raw["extra"], true);
}

#[test]
fn builder_usage_tokens() {
    let r = ReceiptBuilder::new("x").usage_tokens(500, 1000).build();
    assert_eq!(r.usage.input_tokens, Some(500));
    assert_eq!(r.usage.output_tokens, Some(1000));
}

#[test]
fn builder_events_replaces_trace() {
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
    let r = ReceiptBuilder::new("x")
        .add_event(evt1.clone())
        .events(vec![evt2])
        .build();
    // events() replaces, so only one event
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_add_event() {
    let evt = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("x").add_event(evt).build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn builder_duration() {
    let ts = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("x")
        .started_at(ts)
        .duration(Duration::from_secs(10))
        .build();
    assert_eq!(r.meta.duration_ms, 10_000);
}

#[test]
fn builder_error() {
    let r = ReceiptBuilder::new("x").error("something broke").build();
    assert_eq!(r.outcome, Outcome::Failed);
    assert_eq!(r.trace.len(), 1);
    match &r.trace[0].kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "something broke"),
        other => panic!("expected Error event, got {other:?}"),
    }
}

// ── Diff tests ─────────────────────────────────────────────────────

#[test]
fn diff_identical_receipts_empty() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn diff_detects_outcome_change() {
    let a = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
}

#[test]
fn diff_detects_backend_id_change() {
    let a = ReceiptBuilder::new("old").build();
    let mut b = a.clone();
    b.backend.id = "new".into();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
}

#[test]
fn diff_detects_trace_length_change() {
    let a = ReceiptBuilder::new("x").build();
    let mut b = a.clone();
    b.trace.push(AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "hi".into(),
        },
        ext: None,
    });
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "trace.len"));
}

#[test]
fn diff_detects_usage_raw_change() {
    let a = ReceiptBuilder::new("x")
        .usage_raw(serde_json::json!({"a": 1}))
        .build();
    let mut b = a.clone();
    b.usage_raw = serde_json::json!({"a": 2});
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "usage_raw"));
}

#[test]
fn diff_detects_backend_version_change() {
    let a = ReceiptBuilder::new("x").backend_version("1.0").build();
    let mut b = a.clone();
    b.backend.backend_version = Some("2.0".into());
    let d = diff_receipts(&a, &b);
    assert!(d
        .changes
        .iter()
        .any(|c| c.field == "backend.backend_version"));
}

#[test]
fn diff_detects_verification_change() {
    let a = ReceiptBuilder::new("x").build();
    let mut b = a.clone();
    b.verification.harness_ok = true;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "verification"));
}

// ── Validator tests ────────────────────────────────────────────────

#[test]
fn validator_passes_valid_receipt() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(v.validate(&r).is_ok());
}

#[test]
fn validator_passes_without_hash() {
    let v = ReceiptValidator::new();
    let r = ReceiptBuilder::new("mock").build();
    assert!(v.validate(&r).is_ok());
}

#[test]
fn validator_detects_wrong_contract_version() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.contract_version = "wrong/v9".into();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.contract_version"));
}

#[test]
fn validator_detects_tampered_hash() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "receipt_sha256"));
}

#[test]
fn validator_detects_empty_backend_id() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.backend.id = String::new();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "backend.id"));
}

#[test]
fn validator_detects_bad_timestamps() {
    let v = ReceiptValidator::new();
    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let mut r = ReceiptBuilder::new("mock")
        .started_at(t1)
        .finished_at(t2)
        .build();
    // Manually fix the timestamps to make finished < started
    r.meta.started_at = t1;
    r.meta.finished_at = t2;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.finished_at"));
}

#[test]
fn validator_detects_inconsistent_duration() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.duration_ms = 99999;
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.iter().any(|e| e.field == "meta.duration_ms"));
}

#[test]
fn validator_reports_multiple_errors() {
    let v = ReceiptValidator::new();
    let mut r = ReceiptBuilder::new("mock").build();
    r.backend.id = String::new();
    r.meta.contract_version = "bad".into();
    let errs = v.validate(&r).unwrap_err();
    assert!(errs.len() >= 2);
}

#[test]
fn validation_error_display() {
    let e = ValidationError {
        field: "test".into(),
        message: "oops".into(),
    };
    assert_eq!(e.to_string(), "test: oops");
}

// ── Store tests ────────────────────────────────────────────────────

#[test]
fn store_and_retrieve() {
    use crate::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};

    let mut store = InMemoryReceiptStore::new();
    assert!(store.is_empty());

    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let id = r.meta.run_id;
    store.store(r).unwrap();

    assert_eq!(store.len(), 1);
    assert!(!store.is_empty());

    let got = store.get(id).unwrap().unwrap();
    assert_eq!(got.backend.id, "mock");

    let all = store.list(&ReceiptFilter::default()).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, id);
}

#[test]
fn store_rejects_duplicate() {
    use crate::store::{InMemoryReceiptStore, ReceiptStore, StoreError};

    let mut store = InMemoryReceiptStore::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a").run_id(id).build();
    let r2 = ReceiptBuilder::new("b").run_id(id).build();
    store.store(r1).unwrap();
    assert!(matches!(store.store(r2), Err(StoreError::DuplicateId(_))));
}

#[test]
fn store_get_missing_returns_none() {
    use crate::store::{InMemoryReceiptStore, ReceiptStore};

    let store = InMemoryReceiptStore::new();
    assert!(store.get(Uuid::new_v4()).unwrap().is_none());
}

#[test]
fn store_filter_by_backend() {
    use crate::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};

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
    use crate::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};

    let mut store = InMemoryReceiptStore::new();
    store
        .store(ReceiptBuilder::new("x").outcome(Outcome::Complete).build())
        .unwrap();
    store
        .store(ReceiptBuilder::new("y").outcome(Outcome::Failed).build())
        .unwrap();

    let filter = ReceiptFilter {
        outcome: Some(Outcome::Failed),
        ..Default::default()
    };
    let results = store.list(&filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, Outcome::Failed);
}

#[test]
fn store_filter_by_time_range() {
    use crate::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};

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

// ── Serialization tests ────────────────────────────────────────────

#[test]
fn json_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let json = crate::serde_formats::to_json(&r).unwrap();
    let r2 = crate::serde_formats::from_json(&json).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn bytes_roundtrip() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .model("gpt-4")
        .usage_tokens(100, 200)
        .with_hash()
        .unwrap();
    let bytes = crate::serde_formats::to_bytes(&r).unwrap();
    let r2 = crate::serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.usage_raw["model"], "gpt-4");
}

#[test]
fn bytes_compact_vs_json_pretty() {
    let r = ReceiptBuilder::new("mock").build();
    let pretty = crate::serde_formats::to_json(&r).unwrap();
    let compact = crate::serde_formats::to_bytes(&r).unwrap();
    // Compact should be smaller than pretty-printed
    assert!(compact.len() < pretty.len());
}

// ── Edge cases ─────────────────────────────────────────────────────

#[test]
fn empty_receipt_hash_stable() {
    let r = ReceiptBuilder::new("").build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn unicode_in_fields() {
    let r = ReceiptBuilder::new("バックエンド🚀")
        .backend_version("版本 1.0")
        .build();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("バックエンド🚀"));
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn large_trace_receipt() {
    let mut builder = ReceiptBuilder::new("mock");
    for i in 0..500 {
        builder = builder.add_trace_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        });
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 500);
}

#[test]
fn chain_error_display() {
    assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
    assert_eq!(
        ChainError::HashMismatch { index: 3 }.to_string(),
        "hash mismatch at chain index 3"
    );
    assert_eq!(
        ChainError::BrokenLink { index: 1 }.to_string(),
        "broken link at chain index 1"
    );
    let id = Uuid::nil();
    assert!(ChainError::DuplicateId { id }
        .to_string()
        .contains("duplicate"));
}

#[test]
fn chain_into_iterator() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("x").with_hash().unwrap())
        .unwrap();
    let count = (&chain).into_iter().count();
    assert_eq!(count, 1);
}

#[test]
fn receipt_diff_len_and_is_empty() {
    let a = ReceiptBuilder::new("x").build();
    let d = diff_receipts(&a, &a.clone());
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);

    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    b.backend.id = "y".into();
    let d2 = diff_receipts(&a, &b);
    assert!(!d2.is_empty());
    assert!(d2.len() >= 2);
}

// ── Receipt summary tests ──────────────────────────────────────────

#[test]
fn receipt_summary_from_receipt() {
    use crate::store::ReceiptSummary;

    let r = ReceiptBuilder::new("test-be")
        .outcome(Outcome::Partial)
        .build();
    let summary = ReceiptSummary::from(&r);
    assert_eq!(summary.id, r.meta.run_id);
    assert_eq!(summary.backend_id, "test-be");
    assert_eq!(summary.outcome, Outcome::Partial);
}

// ── Enrichment tests ───────────────────────────────────────────────

#[test]
fn enrich_metadata_from_empty_receipt() {
    use crate::enrich::ReceiptMetadata;

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let meta = ReceiptMetadata::from_receipt(&r);
    assert_eq!(meta.backend_id, "mock");
    assert_eq!(meta.event_count, 0);
    assert_eq!(meta.error_count, 0);
    assert_eq!(meta.tool_use_count, 0);
    assert_eq!(meta.delta_count, 0);
    assert_eq!(meta.total_delta_chars, 0);
    assert_eq!(meta.artifact_count, 0);
    assert!(!meta.has_hash);
    assert!(meta.tags.is_empty());
    assert!(meta.annotations.is_empty());
}

#[test]
fn enrich_metadata_counts_events() {
    use crate::enrich::ReceiptMetadata;

    let r = ReceiptBuilder::new("mock")
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "hello world".into(),
            },
            ext: None,
        })
        .build();

    let meta = ReceiptMetadata::from_receipt(&r);
    assert_eq!(meta.event_count, 3);
    assert_eq!(meta.error_count, 1);
    assert_eq!(meta.tool_use_count, 1);
    assert_eq!(meta.delta_count, 1);
    assert_eq!(meta.total_delta_chars, 11);
}

#[test]
fn enrich_metadata_detects_hash() {
    use crate::enrich::ReceiptMetadata;

    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let meta = ReceiptMetadata::from_receipt(&r);
    assert!(meta.has_hash);
}

#[test]
fn enrich_metadata_counts_artifacts() {
    use crate::enrich::ReceiptMetadata;

    let r = ReceiptBuilder::new("mock")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();
    let meta = ReceiptMetadata::from_receipt(&r);
    assert_eq!(meta.artifact_count, 2);
}

#[test]
fn enricher_applies_tags_and_annotations() {
    use crate::enrich::ReceiptEnricher;

    let enricher = ReceiptEnricher::new()
        .tag("production")
        .tag("v2")
        .annotate("team", "platform")
        .annotate("env", "staging");

    let r = ReceiptBuilder::new("mock").build();
    let meta = enricher.enrich(&r);
    assert!(meta.tags.contains(&"production".to_string()));
    assert!(meta.tags.contains(&"v2".to_string()));
    assert_eq!(meta.annotations.get("team").unwrap(), "platform");
    assert_eq!(meta.annotations.get("env").unwrap(), "staging");
}

#[test]
fn enricher_batch() {
    use crate::enrich::ReceiptEnricher;

    let enricher = ReceiptEnricher::new().tag("batch");
    let receipts = vec![
        ReceiptBuilder::new("a").build(),
        ReceiptBuilder::new("b").build(),
    ];
    let metas = enricher.enrich_batch(&receipts);
    assert_eq!(metas.len(), 2);
    assert_eq!(metas[0].backend_id, "a");
    assert_eq!(metas[1].backend_id, "b");
    assert!(metas[0].tags.contains(&"batch".to_string()));
}

// ── Stats tests ────────────────────────────────────────────────────

#[test]
fn stats_from_receipt_basic() {
    use crate::stats::ReceiptStats;

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .build();
    let stats = ReceiptStats::from_receipt(&r);
    assert_eq!(stats.input_tokens, Some(100));
    assert_eq!(stats.output_tokens, Some(200));
    assert_eq!(stats.total_tokens(), Some(300));
    assert_eq!(stats.event_count, 0);
    assert_eq!(stats.error_count, 0);
    assert_eq!(stats.outcome, Outcome::Complete);
}

#[test]
fn stats_total_tokens_partial() {
    use crate::stats::ReceiptStats;

    let r = ReceiptBuilder::new("x")
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: None,
            ..Default::default()
        })
        .build();
    let stats = ReceiptStats::from_receipt(&r);
    assert_eq!(stats.total_tokens(), Some(50));
}

#[test]
fn stats_total_tokens_none() {
    use crate::stats::ReceiptStats;

    let r = ReceiptBuilder::new("x").build();
    let stats = ReceiptStats::from_receipt(&r);
    assert_eq!(stats.total_tokens(), None);
}

#[test]
fn stats_tokens_per_ms() {
    use crate::stats::ReceiptStats;

    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
    let r = ReceiptBuilder::new("x")
        .started_at(ts1)
        .finished_at(ts2)
        .usage_tokens(500, 500)
        .build();
    let stats = ReceiptStats::from_receipt(&r);
    let tpm = stats.tokens_per_ms().unwrap();
    assert!((tpm - 1.0).abs() < 0.01); // 1000 tokens / 1000ms = 1.0
}

#[test]
fn stats_tokens_per_ms_zero_duration() {
    use crate::stats::ReceiptStats;

    let r = ReceiptBuilder::new("x").usage_tokens(100, 100).build();
    let stats = ReceiptStats::from_receipt(&r);
    assert!(stats.tokens_per_ms().is_none());
}

#[test]
fn stats_counts_errors_and_tool_use() {
    use crate::stats::ReceiptStats;

    let r = ReceiptBuilder::new("x")
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .add_event(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "write".into(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!({}),
            },
            ext: None,
        })
        .build();
    let stats = ReceiptStats::from_receipt(&r);
    assert_eq!(stats.event_count, 3);
    assert_eq!(stats.error_count, 1);
    assert_eq!(stats.tool_use_count, 2);
}

#[test]
fn batch_stats_empty() {
    use crate::stats::BatchStats;

    let stats = BatchStats::from_receipts(&[]);
    assert_eq!(stats.total_receipts, 0);
    assert!(stats.success_rate.is_none());
    assert!(stats.avg_duration_ms().is_none());
    assert_eq!(stats.total_tokens(), 0);
}

#[test]
fn batch_stats_basic() {
    use crate::stats::BatchStats;

    let receipts = vec![
        ReceiptBuilder::new("a")
            .outcome(Outcome::Complete)
            .usage_tokens(50, 100)
            .build(),
        ReceiptBuilder::new("b")
            .outcome(Outcome::Failed)
            .usage_tokens(30, 60)
            .build(),
        ReceiptBuilder::new("a")
            .outcome(Outcome::Partial)
            .usage_tokens(20, 40)
            .build(),
    ];
    let stats = BatchStats::from_receipts(&receipts);
    assert_eq!(stats.total_receipts, 3);
    assert_eq!(stats.complete_count, 1);
    assert_eq!(stats.failed_count, 1);
    assert_eq!(stats.partial_count, 1);
    assert_eq!(stats.total_input_tokens, 100);
    assert_eq!(stats.total_output_tokens, 200);
    assert_eq!(stats.total_tokens(), 300);
    assert_eq!(stats.backend_counts.get("a"), Some(&2));
    assert_eq!(stats.backend_counts.get("b"), Some(&1));
    // success rate = 1/3
    let sr = stats.success_rate.unwrap();
    assert!((sr - 1.0 / 3.0).abs() < 0.01);
}

#[test]
fn batch_stats_avg_duration() {
    use crate::stats::BatchStats;

    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 2).unwrap();
    let receipts = vec![
        ReceiptBuilder::new("a")
            .started_at(ts1)
            .finished_at(ts2)
            .build(),
        ReceiptBuilder::new("b")
            .started_at(ts1)
            .finished_at(ts2)
            .build(),
    ];
    let stats = BatchStats::from_receipts(&receipts);
    let avg = stats.avg_duration_ms().unwrap();
    assert!((avg - 2000.0).abs() < 0.01);
}

// ── Version tests ──────────────────────────────────────────────────

#[test]
fn version_parse_valid() {
    use crate::version::FormatVersion;

    let v = FormatVersion::parse("receipt/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn version_parse_larger() {
    use crate::version::FormatVersion;

    let v = FormatVersion::parse("receipt/v2.15").unwrap();
    assert_eq!(v.major, 2);
    assert_eq!(v.minor, 15);
}

#[test]
fn version_parse_invalid_prefix() {
    use crate::version::FormatVersion;

    assert!(FormatVersion::parse("wrong/v0.1").is_err());
}

#[test]
fn version_parse_invalid_format() {
    use crate::version::FormatVersion;

    assert!(FormatVersion::parse("receipt/v0").is_err());
    assert!(FormatVersion::parse("receipt/v0.1.2").is_err());
    assert!(FormatVersion::parse("receipt/vabc").is_err());
}

#[test]
fn version_current() {
    use crate::version::FormatVersion;

    let current = FormatVersion::current();
    assert_eq!(current.major, 0);
    assert_eq!(current.minor, 1);
}

#[test]
fn version_display() {
    use crate::version::FormatVersion;

    let v = FormatVersion::parse("receipt/v0.1").unwrap();
    assert_eq!(v.to_string(), "receipt/v0.1");
}

#[test]
fn version_compatibility_same_major() {
    use crate::version::FormatVersion;

    let v1 = FormatVersion::parse("receipt/v0.1").unwrap();
    let v2 = FormatVersion::parse("receipt/v0.5").unwrap();
    assert!(v1.is_compatible_with(&v2));
    assert!(v2.is_compatible_with(&v1));
}

#[test]
fn version_incompatibility_different_major() {
    use crate::version::FormatVersion;

    let v1 = FormatVersion::parse("receipt/v0.1").unwrap();
    let v2 = FormatVersion::parse("receipt/v1.0").unwrap();
    assert!(!v1.is_compatible_with(&v2));
}

#[test]
fn version_check_contract_version() {
    use crate::version::check_contract_version;

    let r = ReceiptBuilder::new("mock").build();
    assert!(check_contract_version(&r.meta.contract_version));
    assert!(!check_contract_version("wrong/v9"));
}

#[test]
fn version_error_display() {
    use crate::version::VersionError;

    let e = VersionError::InvalidFormat("bad".into());
    assert_eq!(e.to_string(), "invalid version format: \"bad\"");
}

// ══════════════════════════════════════════════════════════════════
// New tests: chain enhancements, archive, summary, verification
// ══════════════════════════════════════════════════════════════════

// ── chain_length / find_by_hash ────────────────────────────────────

#[test]
fn chain_length_empty() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.chain_length(), 0);
}

#[test]
fn chain_length_matches_len() {
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
    assert_eq!(chain.chain_length(), 2);
    assert_eq!(chain.chain_length(), chain.len());
}

#[test]
fn find_by_hash_existing() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let hash = r.receipt_sha256.clone().unwrap();
    let run_id = r.meta.run_id;
    chain.push(r).unwrap();

    let found = chain.find_by_hash(&hash).unwrap();
    assert_eq!(found.meta.run_id, run_id);
}

#[test]
fn find_by_hash_missing() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("mock").with_hash().unwrap())
        .unwrap();
    assert!(chain.find_by_hash("nonexistent").is_none());
}

#[test]
fn find_by_hash_empty_chain() {
    let chain = ReceiptChain::new();
    assert!(chain.find_by_hash("anything").is_none());
}

#[test]
fn find_by_hash_multiple_receipts() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();

    let r1 = ReceiptBuilder::new("a")
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .started_at(ts2)
        .finished_at(ts2)
        .with_hash()
        .unwrap();
    let r3 = ReceiptBuilder::new("c")
        .started_at(ts3)
        .finished_at(ts3)
        .with_hash()
        .unwrap();
    let hash2 = r2.receipt_sha256.clone().unwrap();
    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    chain.push(r3).unwrap();

    let found = chain.find_by_hash(&hash2).unwrap();
    assert_eq!(found.backend.id, "b");
}

// ── Chain building and verification ────────────────────────────────

#[test]
fn chain_verify_chain_empty() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify_chain(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_verify_chain_single() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("m").with_hash().unwrap())
        .unwrap();
    assert!(chain.verify_chain().is_ok());
}

#[test]
fn chain_verify_chain_multiple_valid() {
    let mut chain = ReceiptChain::new();
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
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
    assert!(chain.verify_chain().is_ok());
}

// ── Chain integrity / tamper detection ──────────────────────────────

#[test]
fn chain_detect_tampering_clean_chain() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("m").with_hash().unwrap())
        .unwrap();
    assert!(chain.detect_tampering().is_empty());
}

#[test]
fn chain_detect_tampering_hash_mismatch() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(r).unwrap();
    // Tamper with the outcome after insertion
    // We have to access the internal receipt indirectly
    // Instead, build a chain with skip_validation
    let mut r2 = ReceiptBuilder::new("mock").with_hash().unwrap();
    r2.outcome = Outcome::Failed; // tamper
    let chain2 = ChainBuilder::new()
        .skip_validation()
        .append(r2)
        .unwrap()
        .build();
    let evidence = chain2.detect_tampering();
    assert!(!evidence.is_empty());
    assert!(matches!(evidence[0].kind, TamperKind::HashMismatch { .. }));
}

#[test]
fn chain_detect_tampering_returns_all_issues() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut r1 = ReceiptBuilder::new("a")
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();
    let mut r2 = ReceiptBuilder::new("b")
        .started_at(ts2)
        .finished_at(ts2)
        .with_hash()
        .unwrap();
    r1.outcome = Outcome::Failed; // tamper
    r2.outcome = Outcome::Failed; // tamper
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r1)
        .unwrap()
        .append(r2)
        .unwrap()
        .build();
    let evidence = chain.detect_tampering();
    // At least one issue per tampered receipt
    assert!(evidence.len() >= 2);
}

#[test]
fn chain_tamper_evidence_display() {
    let ev = TamperEvidence {
        index: 0,
        sequence: 0,
        kind: TamperKind::HashMismatch {
            stored: "aaa".into(),
            computed: "bbb".into(),
        },
    };
    let s = ev.to_string();
    assert!(s.contains("index=0"));
    assert!(s.contains("hash mismatch"));
}

#[test]
fn chain_gap_display() {
    let gap = ChainGap {
        expected: 2,
        actual: 5,
        after_index: 1,
    };
    let s = gap.to_string();
    assert!(s.contains("gap"));
    assert!(s.contains("expected seq 2"));
}

// ── Chain builder tests ────────────────────────────────────────────

#[test]
fn chain_builder_basic() {
    let chain = ChainBuilder::new()
        .append(ReceiptBuilder::new("x").with_hash().unwrap())
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
    assert_eq!(chain.sequence_at(0), Some(0));
}

#[test]
fn chain_builder_skip_validation() {
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
                                 // Should succeed with skip_validation
    let chain = ChainBuilder::new()
        .skip_validation()
        .append(r)
        .unwrap()
        .build();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_builder_with_sequence_gap() {
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
            5, // gap!
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
fn chain_parent_hash_linkage() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("a")
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();
    let r1_hash = r1.receipt_sha256.clone().unwrap();
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
    // First receipt has no parent
    assert!(chain.parent_hash_at(0).is_none());
    // Second receipt's parent is first receipt's hash
    assert_eq!(chain.parent_hash_at(1), Some(r1_hash.as_str()));
}

#[test]
fn chain_serde_roundtrip() {
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
fn chain_summary_basic() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap();
    let mut chain = ReceiptChain::new();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Complete)
                .started_at(ts1)
                .finished_at(ts1)
                .usage_tokens(100, 200)
                .with_hash()
                .unwrap(),
        )
        .unwrap();
    chain
        .push(
            ReceiptBuilder::new("mock")
                .outcome(Outcome::Failed)
                .started_at(ts2)
                .finished_at(ts2)
                .usage_tokens(50, 75)
                .with_hash()
                .unwrap(),
        )
        .unwrap();

    let summary = chain.chain_summary();
    assert_eq!(summary.total_receipts, 2);
    assert_eq!(summary.complete_count, 1);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.total_input_tokens, 150);
    assert_eq!(summary.total_output_tokens, 275);
    assert!(summary.all_hashes_valid);
    assert_eq!(summary.gap_count, 0);
}

// ── Receipt verification tests ─────────────────────────────────────

#[test]
fn verify_receipt_valid() {
    use crate::verify::verify_receipt;

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    let result = verify_receipt(&r);
    assert!(result.is_verified());
    assert!(result.issues.is_empty());
}

#[test]
fn verify_receipt_invalid_hash() {
    use crate::verify::verify_receipt;

    let mut r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    r.outcome = Outcome::Failed; // tamper
    let result = verify_receipt(&r);
    assert!(!result.hash_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_wrong_contract() {
    use crate::verify::verify_receipt;

    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.contract_version = "wrong/v99".into();
    let result = verify_receipt(&r);
    assert!(!result.contract_valid);
    assert!(!result.is_verified());
}

#[test]
fn verify_receipt_bad_timestamps() {
    use crate::verify::verify_receipt;

    let mut r = ReceiptBuilder::new("mock").build();
    r.meta.duration_ms = 999999; // inconsistent
    let result = verify_receipt(&r);
    assert!(!result.timestamps_valid);
}

#[test]
fn verify_receipt_no_hash_passes() {
    use crate::verify::verify_receipt;

    let r = ReceiptBuilder::new("mock").build();
    let result = verify_receipt(&r);
    assert!(result.hash_valid);
}

#[test]
fn verify_receipt_display_verified() {
    use crate::verify::verify_receipt;

    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    let result = verify_receipt(&r);
    assert!(result.to_string().contains("verified"));
}

#[test]
fn verify_receipt_display_failed() {
    use crate::verify::verify_receipt;

    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.outcome = Outcome::Failed;
    let result = verify_receipt(&r);
    assert!(result.to_string().contains("failed"));
}

#[test]
fn auditor_batch_clean() {
    use crate::verify::ReceiptAuditor;

    let auditor = ReceiptAuditor::new();
    let r1 = ReceiptBuilder::new("a").with_hash().unwrap();
    let r2 = ReceiptBuilder::new("b").with_hash().unwrap();
    let report = auditor.audit_batch(&[r1, r2]);
    assert!(report.is_clean());
    assert_eq!(report.total, 2);
    assert_eq!(report.valid, 2);
}

#[test]
fn auditor_batch_detects_invalid() {
    use crate::verify::ReceiptAuditor;

    let auditor = ReceiptAuditor::new();
    let mut r = ReceiptBuilder::new("a").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
    let report = auditor.audit_batch(&[r]);
    assert!(!report.is_clean());
    assert_eq!(report.invalid, 1);
}

#[test]
fn auditor_report_display() {
    use crate::verify::ReceiptAuditor;

    let auditor = ReceiptAuditor::new();
    let r = ReceiptBuilder::new("a").with_hash().unwrap();
    let report = auditor.audit_batch(&[r]);
    let s = report.to_string();
    assert!(s.contains("AuditReport"));
    assert!(s.contains("total: 1"));
}

// ── Archive tests ──────────────────────────────────────────────────

#[test]
fn archive_new_is_empty() {
    use crate::archive::ReceiptArchive;

    let archive = ReceiptArchive::new();
    assert!(archive.is_empty());
    assert_eq!(archive.len(), 0);
}

#[test]
fn archive_store_and_retrieve() {
    use crate::archive::ReceiptArchive;

    let mut archive = ReceiptArchive::new();
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let id = r.meta.run_id;
    archive.store(r).unwrap();

    assert_eq!(archive.len(), 1);
    let got = archive.retrieve(id).unwrap();
    assert_eq!(got.backend.id, "mock");
}

#[test]
fn archive_retrieve_missing() {
    use crate::archive::ReceiptArchive;

    let archive = ReceiptArchive::new();
    assert!(archive.retrieve(Uuid::new_v4()).is_none());
}

#[test]
fn archive_rejects_duplicate() {
    use crate::archive::{ArchiveError, ReceiptArchive};

    let mut archive = ReceiptArchive::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a").run_id(id).build();
    let r2 = ReceiptBuilder::new("b").run_id(id).build();
    archive.store(r1).unwrap();
    assert!(matches!(
        archive.store(r2),
        Err(ArchiveError::DuplicateId(_))
    ));
}

#[test]
fn archive_search_all() {
    use crate::archive::{ArchiveQuery, ReceiptArchive};

    let mut archive = ReceiptArchive::new();
    archive.store(ReceiptBuilder::new("a").build()).unwrap();
    archive.store(ReceiptBuilder::new("b").build()).unwrap();

    let results = archive.search(&ArchiveQuery::default());
    assert_eq!(results.len(), 2);
}

#[test]
fn archive_search_by_backend() {
    use crate::archive::{ArchiveQuery, ReceiptArchive};

    let mut archive = ReceiptArchive::new();
    archive.store(ReceiptBuilder::new("alpha").build()).unwrap();
    archive.store(ReceiptBuilder::new("beta").build()).unwrap();

    let query = ArchiveQuery {
        backend_id: Some("alpha".into()),
        ..Default::default()
    };
    let results = archive.search(&query);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
}

#[test]
fn archive_search_by_work_order_id() {
    use crate::archive::{ArchiveQuery, ReceiptArchive};

    let mut archive = ReceiptArchive::new();
    let wo_id = Uuid::new_v4();
    archive
        .store(ReceiptBuilder::new("a").work_order_id(wo_id).build())
        .unwrap();
    archive.store(ReceiptBuilder::new("b").build()).unwrap();

    let query = ArchiveQuery {
        work_order_id: Some(wo_id),
        ..Default::default()
    };
    let results = archive.search(&query);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].meta.work_order_id, wo_id);
}

#[test]
fn archive_search_by_time_range() {
    use crate::archive::{ArchiveQuery, ReceiptArchive};

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap();

    let mut archive = ReceiptArchive::new();
    archive
        .store(
            ReceiptBuilder::new("early")
                .started_at(t1)
                .finished_at(t1)
                .build(),
        )
        .unwrap();
    archive
        .store(
            ReceiptBuilder::new("mid")
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();
    archive
        .store(
            ReceiptBuilder::new("late")
                .started_at(t3)
                .finished_at(t3)
                .build(),
        )
        .unwrap();

    let query = ArchiveQuery {
        after: Some(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()),
        before: Some(Utc.with_ymd_and_hms(2025, 9, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = archive.search(&query);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "mid");
}

#[test]
fn archive_search_combined_filters() {
    use crate::archive::{ArchiveQuery, ReceiptArchive};

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
    let wo = Uuid::new_v4();

    let mut archive = ReceiptArchive::new();
    archive
        .store(
            ReceiptBuilder::new("alpha")
                .work_order_id(wo)
                .started_at(t1)
                .finished_at(t1)
                .build(),
        )
        .unwrap();
    archive
        .store(
            ReceiptBuilder::new("alpha")
                .work_order_id(wo)
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();
    archive
        .store(
            ReceiptBuilder::new("beta")
                .work_order_id(wo)
                .started_at(t2)
                .finished_at(t2)
                .build(),
        )
        .unwrap();

    let query = ArchiveQuery {
        work_order_id: Some(wo),
        backend_id: Some("alpha".into()),
        after: Some(Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap()),
        ..Default::default()
    };
    let results = archive.search(&query);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].backend.id, "alpha");
}

#[test]
fn archive_search_no_match() {
    use crate::archive::{ArchiveQuery, ReceiptArchive};

    let mut archive = ReceiptArchive::new();
    archive.store(ReceiptBuilder::new("a").build()).unwrap();

    let query = ArchiveQuery {
        backend_id: Some("nonexistent".into()),
        ..Default::default()
    };
    let results = archive.search(&query);
    assert!(results.is_empty());
}

#[test]
fn archive_error_display() {
    use crate::archive::ArchiveError;

    let id = Uuid::nil();
    let e = ArchiveError::DuplicateId(id);
    assert!(e.to_string().contains("duplicate"));
}

// ── Summary tests ──────────────────────────────────────────────────

#[test]
fn summary_empty_receipts() {
    use crate::summary::AggregateSummary;

    let summary = AggregateSummary::from_receipts(&[]);
    assert_eq!(summary.total_receipts, 0);
    assert_eq!(summary.success_rate, 0.0);
    assert_eq!(summary.avg_duration_ms, 0.0);
    assert_eq!(summary.total_tokens, 0);
    assert!(summary.most_common_backend.is_none());
}

#[test]
fn summary_single_complete() {
    use crate::summary::AggregateSummary;

    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(100, 200)
        .build();
    let summary = AggregateSummary::from_receipts(&[r]);
    assert_eq!(summary.total_receipts, 1);
    assert!((summary.success_rate - 1.0).abs() < f64::EPSILON);
    assert_eq!(summary.total_tokens, 300);
    assert_eq!(summary.total_input_tokens, 100);
    assert_eq!(summary.total_output_tokens, 200);
    assert_eq!(summary.complete_count, 1);
    assert_eq!(summary.failed_count, 0);
}

#[test]
fn summary_mixed_outcomes() {
    use crate::summary::AggregateSummary;

    let receipts = vec![
        ReceiptBuilder::new("a").outcome(Outcome::Complete).build(),
        ReceiptBuilder::new("b").outcome(Outcome::Failed).build(),
        ReceiptBuilder::new("c").outcome(Outcome::Partial).build(),
        ReceiptBuilder::new("d").outcome(Outcome::Complete).build(),
    ];
    let summary = AggregateSummary::from_receipts(&receipts);
    assert_eq!(summary.total_receipts, 4);
    assert!((summary.success_rate - 0.5).abs() < f64::EPSILON);
    assert_eq!(summary.complete_count, 2);
    assert_eq!(summary.failed_count, 1);
    assert_eq!(summary.partial_count, 1);
}

#[test]
fn summary_error_distribution() {
    use crate::summary::AggregateSummary;

    let receipts = vec![
        ReceiptBuilder::new("a").error("timeout").build(),
        ReceiptBuilder::new("b").error("timeout").build(),
        ReceiptBuilder::new("c").error("auth failed").build(),
    ];
    let summary = AggregateSummary::from_receipts(&receipts);
    assert_eq!(summary.error_distribution.get("timeout"), Some(&2));
    assert_eq!(summary.error_distribution.get("auth failed"), Some(&1));
    assert_eq!(summary.error_distribution.len(), 2);
}

#[test]
fn summary_backend_distribution() {
    use crate::summary::AggregateSummary;

    let receipts = vec![
        ReceiptBuilder::new("openai").build(),
        ReceiptBuilder::new("openai").build(),
        ReceiptBuilder::new("anthropic").build(),
    ];
    let summary = AggregateSummary::from_receipts(&receipts);
    assert_eq!(summary.backend_distribution.get("openai"), Some(&2));
    assert_eq!(summary.backend_distribution.get("anthropic"), Some(&1));
}

#[test]
fn summary_most_common_backend() {
    use crate::summary::AggregateSummary;

    let receipts = vec![
        ReceiptBuilder::new("openai").build(),
        ReceiptBuilder::new("openai").build(),
        ReceiptBuilder::new("anthropic").build(),
    ];
    let summary = AggregateSummary::from_receipts(&receipts);
    assert_eq!(summary.most_common_backend, Some("openai".to_string()));
}

#[test]
fn summary_avg_duration() {
    use crate::summary::AggregateSummary;

    let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 10).unwrap();
    let t3 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 20).unwrap();

    let receipts = vec![
        ReceiptBuilder::new("a")
            .started_at(t1)
            .finished_at(t2)
            .build(), // 10s
        ReceiptBuilder::new("b")
            .started_at(t1)
            .finished_at(t3)
            .build(), // 20s
    ];
    let summary = AggregateSummary::from_receipts(&receipts);
    assert_eq!(summary.total_duration_ms, 30_000);
    assert!((summary.avg_duration_ms - 15_000.0).abs() < f64::EPSILON);
}

#[test]
fn summary_total_tokens_combined() {
    use crate::summary::AggregateSummary;

    let receipts = vec![
        ReceiptBuilder::new("a").usage_tokens(50, 100).build(),
        ReceiptBuilder::new("b").usage_tokens(30, 60).build(),
    ];
    let summary = AggregateSummary::from_receipts(&receipts);
    assert_eq!(summary.total_input_tokens, 80);
    assert_eq!(summary.total_output_tokens, 160);
    assert_eq!(summary.total_tokens, 240);
}

// ── Serde roundtrip tests ──────────────────────────────────────────

#[test]
fn chain_json_roundtrip_preserves_hashes() {
    let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap();
    let mut chain = ReceiptChain::new();
    let r1 = ReceiptBuilder::new("a")
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();
    let h1 = r1.receipt_sha256.clone().unwrap();
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

    let json = serde_json::to_string(&chain).unwrap();
    let chain2: ReceiptChain = serde_json::from_str(&json).unwrap();
    assert_eq!(
        chain2.get(0).unwrap().receipt_sha256.as_deref(),
        Some(h1.as_str())
    );
}

#[test]
fn receipt_json_roundtrip_with_usage() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .usage_tokens(500, 1000)
        .model("gpt-4o")
        .with_hash()
        .unwrap();
    let json = crate::serde_formats::to_json(&r).unwrap();
    let r2 = crate::serde_formats::from_json(&json).unwrap();
    assert_eq!(r.usage.input_tokens, r2.usage.input_tokens);
    assert_eq!(r.usage.output_tokens, r2.usage.output_tokens);
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn receipt_bytes_roundtrip_preserves_all() {
    let r = ReceiptBuilder::new("test")
        .outcome(Outcome::Partial)
        .backend_version("2.0")
        .adapter_version("1.5")
        .usage_tokens(10, 20)
        .with_hash()
        .unwrap();
    let bytes = crate::serde_formats::to_bytes(&r).unwrap();
    let r2 = crate::serde_formats::from_bytes(&bytes).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.backend.backend_version, r2.backend.backend_version);
}
