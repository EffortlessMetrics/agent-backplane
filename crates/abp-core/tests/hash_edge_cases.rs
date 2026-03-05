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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive edge-case tests for receipt hashing (`receipt_hash` / `with_hash`).

use abp_core::*;
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────

/// A fully deterministic minimal receipt (fixed timestamps, nil UUIDs).
fn fixed_receipt() -> Receipt {
    let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: BTreeMap::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport::default(),
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        kind,
        ext: None,
    }
}

// ── 1. Idempotent hashing — hashing twice produces same result ──────

#[test]
fn hash_is_idempotent() {
    let r = fixed_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(
        h1, h2,
        "hashing the same receipt twice must yield the same hash"
    );
}

#[test]
fn with_hash_is_idempotent() {
    let r = fixed_receipt();
    let r1 = r.clone().with_hash().unwrap();
    let r2 = r1.clone().with_hash().unwrap();
    assert_eq!(
        r1.receipt_sha256, r2.receipt_sha256,
        "with_hash applied twice must produce the same stored hash"
    );
}

// ── 2. Hash changes with any field change ───────────────────────────

#[test]
fn hash_changes_when_backend_id_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.backend.id = "other-backend".into();
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_outcome_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.outcome = Outcome::Failed;
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_duration_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.meta.duration_ms = 9999;
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_contract_version_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.meta.contract_version = "abp/v99".into();
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_mode_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.mode = ExecutionMode::Passthrough;
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_trace_added() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.trace.push(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_artifact_added() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "out.diff".into(),
    });
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_usage_raw_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.usage_raw = serde_json::json!({"tokens": 42});
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_usage_normalized_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.usage.input_tokens = Some(100);
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_verification_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.verification.harness_ok = true;
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_capabilities_change() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.capabilities
        .insert(Capability::Streaming, SupportLevel::Native);
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_run_id_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.meta.run_id = Uuid::from_u128(1);
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

#[test]
fn hash_changes_when_work_order_id_changes() {
    let base_hash = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.meta.work_order_id = Uuid::from_u128(1);
    assert_ne!(receipt_hash(&r).unwrap(), base_hash);
}

// ── 3. Hash is deterministic ────────────────────────────────────────

#[test]
fn hash_deterministic_across_separate_constructions() {
    let h1 = receipt_hash(&fixed_receipt()).unwrap();
    let h2 = receipt_hash(&fixed_receipt()).unwrap();
    assert_eq!(
        h1, h2,
        "identically constructed receipts must hash the same"
    );
}

#[test]
fn hash_deterministic_after_serde_round_trip() {
    let r = fixed_receipt();
    let h_before = receipt_hash(&r).unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    let h_after = receipt_hash(&r2).unwrap();
    assert_eq!(h_before, h_after, "hash must survive a JSON round-trip");
}

// ── 4. Hash format — valid hex, SHA-256 length ──────────────────────

#[test]
fn hash_is_64_hex_chars() {
    let h = receipt_hash(&fixed_receipt()).unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex digest must be 64 characters");
    assert!(
        h.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must contain only hex characters, got: {h}"
    );
}

#[test]
fn hash_is_lowercase_hex() {
    let h = receipt_hash(&fixed_receipt()).unwrap();
    assert_eq!(h, h.to_ascii_lowercase(), "hash must be lowercase hex");
}

// ── 5. Pre-existing hash is cleared before recompute ────────────────

#[test]
fn preexisting_hash_does_not_affect_result() {
    let r = fixed_receipt();
    let expected = receipt_hash(&r).unwrap();

    let mut r_with_bogus = fixed_receipt();
    r_with_bogus.receipt_sha256 = Some("aaaa".repeat(16));
    assert_eq!(
        receipt_hash(&r_with_bogus).unwrap(),
        expected,
        "pre-existing receipt_sha256 must be cleared before hashing"
    );
}

#[test]
fn preexisting_hash_replaced_by_with_hash() {
    let mut r = fixed_receipt();
    r.receipt_sha256 = Some("stale".into());
    let r = r.with_hash().unwrap();
    assert_ne!(r.receipt_sha256.as_deref(), Some("stale"));
    // Verify the new hash is valid
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ── 6. Empty / minimal receipt still produces valid hash ────────────

#[test]
fn minimal_receipt_hashes_successfully() {
    let r = fixed_receipt();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn minimal_receipt_with_hash_succeeds() {
    let r = fixed_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

// ── 7. Unicode in fields ────────────────────────────────────────────

#[test]
fn unicode_backend_id_hashes() {
    let mut r = fixed_receipt();
    r.backend.id = "バックエンド-🦀".into();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn unicode_in_verification_hashes() {
    let mut r = fixed_receipt();
    r.verification.git_diff = Some("diff --git 日本語ファイル.txt".into());
    r.verification.git_status = Some("M  données/résumé.md".into());
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn emoji_in_trace_events_hashes() {
    let mut r = fixed_receipt();
    r.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: "Hello 🌍🎉✨ — café résumé naïve".into(),
    }));
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn unicode_fields_change_hash() {
    let base = receipt_hash(&fixed_receipt()).unwrap();
    let mut r = fixed_receipt();
    r.backend.id = "тест".into();
    assert_ne!(receipt_hash(&r).unwrap(), base);
}

// ── 8. Large receipts — many trace events ───────────────────────────

#[test]
fn large_trace_hashes_correctly() {
    let mut r = fixed_receipt();
    for i in 0..1000 {
        r.trace.push(make_event(AgentEventKind::AssistantDelta {
            text: format!("chunk {i}"),
        }));
    }
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn large_trace_hash_is_deterministic() {
    let build = || {
        let mut r = fixed_receipt();
        for i in 0..500 {
            r.trace.push(make_event(AgentEventKind::ToolCall {
                tool_name: format!("tool_{i}"),
                tool_use_id: Some(format!("id_{i}")),
                parent_tool_use_id: None,
                input: serde_json::json!({"n": i}),
            }));
        }
        r
    };
    let h1 = receipt_hash(&build()).unwrap();
    let h2 = receipt_hash(&build()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn large_trace_order_matters() {
    let mut r1 = fixed_receipt();
    r1.trace.push(make_event(AgentEventKind::AssistantDelta {
        text: "alpha".into(),
    }));
    r1.trace.push(make_event(AgentEventKind::AssistantDelta {
        text: "beta".into(),
    }));

    let mut r2 = fixed_receipt();
    r2.trace.push(make_event(AgentEventKind::AssistantDelta {
        text: "beta".into(),
    }));
    r2.trace.push(make_event(AgentEventKind::AssistantDelta {
        text: "alpha".into(),
    }));

    assert_ne!(
        receipt_hash(&r1).unwrap(),
        receipt_hash(&r2).unwrap(),
        "trace event order must affect the hash"
    );
}

// ── 9. Hash verification — with_hash then receipt_hash agree ────────

#[test]
fn with_hash_then_receipt_hash_agree() {
    let r = fixed_receipt().with_hash().unwrap();
    let stored = r.receipt_sha256.clone().unwrap();
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(stored, recomputed, "stored hash must match recomputed hash");
}

#[test]
fn receipt_builder_with_hash_consistent() {
    let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let r = ReceiptBuilder::new("mock")
        .work_order_id(Uuid::nil())
        .started_at(ts)
        .finished_at(ts)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();

    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(r.receipt_sha256.as_deref(), Some(recomputed.as_str()));
}

#[test]
fn with_hash_chain_is_stable() {
    let r = fixed_receipt();
    let r1 = r.clone().with_hash().unwrap();
    let r2 = r1.clone().with_hash().unwrap();
    let r3 = r2.clone().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    assert_eq!(r2.receipt_sha256, r3.receipt_sha256);
}

// ── 10. None vs Some(hash) — receipt_sha256 does not affect hash ────

#[test]
fn none_hash_and_some_hash_produce_same_digest() {
    let mut r_none = fixed_receipt();
    r_none.receipt_sha256 = None;

    let mut r_some = fixed_receipt();
    r_some.receipt_sha256 = Some("abc123".into());

    assert_eq!(
        receipt_hash(&r_none).unwrap(),
        receipt_hash(&r_some).unwrap(),
        "None and Some(hash) must produce the same digest"
    );
}

#[test]
fn correct_hash_in_field_still_cleared() {
    let r = fixed_receipt().with_hash().unwrap();
    let correct_hash = r.receipt_sha256.clone().unwrap();

    // Even when the stored hash is already correct, receipt_hash must
    // null it out and still produce the same value.
    let mut r2 = fixed_receipt();
    r2.receipt_sha256 = Some(correct_hash.clone());
    assert_eq!(receipt_hash(&r2).unwrap(), correct_hash);
}

#[test]
fn empty_string_hash_treated_same_as_none() {
    let mut r_empty = fixed_receipt();
    r_empty.receipt_sha256 = Some(String::new());

    let mut r_none = fixed_receipt();
    r_none.receipt_sha256 = None;

    assert_eq!(
        receipt_hash(&r_empty).unwrap(),
        receipt_hash(&r_none).unwrap(),
        "empty-string hash and None must both be nulled before hashing"
    );
}
