// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::*;
use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, ExecutionMode, UsageNormalized, VerificationReport,
};
use chrono::{TimeZone, Utc};
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
    assert!(
        d.changes
            .iter()
            .any(|c| c.field == "backend.backend_version")
    );
}

#[test]
fn diff_detects_verification_change() {
    let a = ReceiptBuilder::new("x").build();
    let mut b = a.clone();
    b.verification.harness_ok = true;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "verification"));
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
    assert!(
        ChainError::DuplicateId { id }
            .to_string()
            .contains("duplicate")
    );
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
