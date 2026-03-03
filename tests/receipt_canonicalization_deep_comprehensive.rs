// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

//! Deep comprehensive tests for receipt canonicalization and hashing.
//!
//! Exercises `abp_receipt` (canonicalize, compute_hash, verify_hash,
//! ReceiptBuilder, ReceiptChain, diff_receipts) and `abp_core` receipt types.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ExecutionMode, Outcome, Receipt, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, WorkOrderBuilder, canonical_json, receipt_hash,
    sha256_hex,
};
use abp_receipt::{
    ReceiptBuilder, ReceiptChain, canonicalize, compute_hash, diff_receipts, verify_hash,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_ts() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

fn fixed_ts2() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 0).unwrap()
}

fn nil_uuid() -> Uuid {
    Uuid::nil()
}

/// Minimal receipt with fully deterministic fields.
fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: nil_uuid(),
            work_order_id: nil_uuid(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "mock".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
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

/// Receipt built via ReceiptBuilder with deterministic IDs and timestamps.
fn builder_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .outcome(Outcome::Complete)
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .build()
}

/// Fully populated receipt with every optional field set.
fn full_receipt() -> Receipt {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let t1 = fixed_ts();
    let t2 = t1 + Duration::seconds(5);

    ReceiptBuilder::new("full-backend")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .outcome(Outcome::Complete)
        .started_at(t1)
        .finished_at(t2)
        .backend_version("2.0.0")
        .adapter_version("1.0.0")
        .capabilities(caps)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(serde_json::json!({"prompt_tokens": 100, "completion_tokens": 50}))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.005),
        })
        .verification(VerificationReport {
            git_diff: Some("diff --git a/foo b/foo".into()),
            git_status: Some("M foo".into()),
            harness_ok: true,
        })
        .add_trace_event(AgentEvent {
            ts: t1,
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: t2,
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
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
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

// ===========================================================================
// 1. Receipt construction and field population
// ===========================================================================

#[test]
fn construction_builder_defaults() {
    let r = ReceiptBuilder::new("test-backend").build();
    assert_eq!(r.backend.id, "test-backend");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn construction_builder_sets_outcome() {
    let r = ReceiptBuilder::new("x").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn construction_builder_sets_partial_outcome() {
    let r = ReceiptBuilder::new("x").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn construction_builder_sets_backend_version() {
    let r = ReceiptBuilder::new("x")
        .backend_version("1.2.3")
        .adapter_version("0.9")
        .build();
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.2.3"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.9"));
}

#[test]
fn construction_builder_sets_work_order_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("x").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn construction_builder_sets_run_id() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("x").run_id(id).build();
    assert_eq!(r.meta.run_id, id);
}

#[test]
fn construction_builder_timestamps_and_duration() {
    let t1 = fixed_ts();
    let t2 = t1 + Duration::seconds(10);
    let r = ReceiptBuilder::new("x")
        .started_at(t1)
        .finished_at(t2)
        .build();
    assert_eq!(r.meta.started_at, t1);
    assert_eq!(r.meta.finished_at, t2);
    assert_eq!(r.meta.duration_ms, 10_000);
}

#[test]
fn construction_builder_zero_duration() {
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("x")
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn construction_builder_mode_passthrough() {
    let r = ReceiptBuilder::new("x")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn construction_builder_mode_mapped_default() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn construction_builder_usage_raw() {
    let r = ReceiptBuilder::new("x")
        .usage_raw(serde_json::json!({"tokens": 42}))
        .build();
    assert_eq!(r.usage_raw["tokens"], 42);
}

#[test]
fn construction_builder_normalized_usage() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(10),
        cache_write_tokens: Some(5),
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    };
    let r = ReceiptBuilder::new("x").usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
    assert_eq!(r.usage.cache_read_tokens, Some(10));
    assert_eq!(r.usage.cache_write_tokens, Some(5));
    assert_eq!(r.usage.request_units, Some(1));
    assert_eq!(r.usage.estimated_cost_usd, Some(0.01));
}

#[test]
fn construction_builder_verification() {
    let v = VerificationReport {
        git_diff: Some("diff".into()),
        git_status: Some("M file.rs".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("x").verification(v).build();
    assert_eq!(r.verification.git_diff.as_deref(), Some("diff"));
    assert_eq!(r.verification.git_status.as_deref(), Some("M file.rs"));
    assert!(r.verification.harness_ok);
}

#[test]
fn construction_builder_add_trace_events() {
    let r = ReceiptBuilder::new("x")
        .add_trace_event(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .add_trace_event(make_event(AgentEventKind::AssistantDelta {
            text: "hello".into(),
        }))
        .build();
    assert_eq!(r.trace.len(), 2);
}

#[test]
fn construction_builder_add_artifacts() {
    let r = ReceiptBuilder::new("x")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "b.log".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 2);
    assert_eq!(r.artifacts[0].kind, "patch");
    assert_eq!(r.artifacts[1].kind, "log");
}

#[test]
fn construction_builder_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);
    let r = ReceiptBuilder::new("x").capabilities(caps).build();
    assert_eq!(r.capabilities.len(), 2);
    assert!(r.capabilities.contains_key(&Capability::Streaming));
}

#[test]
fn construction_full_receipt_all_fields_populated() {
    let r = full_receipt();
    assert_eq!(r.backend.id, "full-backend");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("1.0.0"));
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.trace.len(), 2);
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.capabilities.len(), 2);
    assert!(r.verification.harness_ok);
    assert!(r.verification.git_diff.is_some());
    assert!(r.verification.git_status.is_some());
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(50));
    assert_eq!(r.meta.duration_ms, 5000);
}

#[test]
fn construction_manual_receipt_struct() {
    let r = minimal_receipt();
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.meta.run_id, nil_uuid());
    assert_eq!(r.meta.work_order_id, nil_uuid());
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

// ===========================================================================
// 2. with_hash() produces deterministic SHA-256
// ===========================================================================

#[test]
fn with_hash_produces_some() {
    let r = builder_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn with_hash_is_64_hex_chars() {
    let r = builder_receipt().with_hash().unwrap();
    let h = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn with_hash_deterministic() {
    let r1 = builder_receipt().with_hash().unwrap();
    let r2 = builder_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn with_hash_matches_compute_hash() {
    let plain = builder_receipt();
    let hashed = plain.clone().with_hash().unwrap();
    let computed = compute_hash(&plain).unwrap();
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(computed.as_str()));
}

#[test]
fn with_hash_full_receipt_deterministic() {
    let r1 = full_receipt().with_hash().unwrap();
    let r2 = full_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn abp_receipt_builder_with_hash() {
    let r = ReceiptBuilder::new("test")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn with_hash_core_and_receipt_crate_agree() {
    let r = builder_receipt();
    let core_hash = receipt_hash(&r).unwrap();
    let receipt_hash_val = compute_hash(&r).unwrap();
    assert_eq!(core_hash, receipt_hash_val);
}

// ===========================================================================
// 3. receipt_hash() sets receipt_sha256 to null (self-referential prevention)
// ===========================================================================

#[test]
fn self_ref_hash_ignores_stored_hash() {
    let r = builder_receipt();
    let h1 = receipt_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("anything_here".into());
    let h2 = receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2, "stored hash must not influence the computed hash");
}

#[test]
fn self_ref_hash_ignores_correct_hash() {
    let r = builder_receipt();
    let h1 = compute_hash(&r).unwrap();

    let mut r2 = r.clone();
    r2.receipt_sha256 = Some(h1.clone());
    let h2 = compute_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn self_ref_canonicalize_nullifies_hash_field() {
    let mut r = builder_receipt();
    r.receipt_sha256 = Some("deadbeef".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn self_ref_canonicalize_none_is_also_null() {
    let r = builder_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn self_ref_canonical_form_independent_of_hash_value() {
    let r1 = builder_receipt();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("some_hash_value".into());
    let mut r3 = r1.clone();
    r3.receipt_sha256 = Some("different_hash_value".into());

    let c1 = canonicalize(&r1).unwrap();
    let c2 = canonicalize(&r2).unwrap();
    let c3 = canonicalize(&r3).unwrap();
    assert_eq!(c1, c2);
    assert_eq!(c2, c3);
}

#[test]
fn self_ref_core_receipt_hash_nullifies_too() {
    let mut r = minimal_receipt();
    r.receipt_sha256 = Some("garbage".into());
    let h1 = receipt_hash(&r).unwrap();

    r.receipt_sha256 = None;
    let h2 = receipt_hash(&r).unwrap();

    assert_eq!(h1, h2);
}

// ===========================================================================
// 4. Canonical JSON serialization (BTreeMap ordering)
// ===========================================================================

#[test]
fn canonical_json_is_compact() {
    let r = builder_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
    assert!(!json.contains("  "));
}

#[test]
fn canonical_json_deterministic() {
    let r = builder_receipt();
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_json_keys_sorted() {
    let r = builder_receipt();
    let json = canonicalize(&r).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let serde_json::Value::Object(map) = &parsed {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "top-level keys must be alphabetically sorted");
    } else {
        panic!("expected JSON object");
    }
}

#[test]
fn canonical_json_btreemap_ordering_for_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Emulated);

    let r = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .capabilities(caps)
        .build();

    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2, "BTreeMap ordering must be stable");
}

#[test]
fn canonical_json_core_function_deterministic() {
    let val = serde_json::json!({"z": 1, "a": 2, "m": 3});
    let j1 = canonical_json(&val).unwrap();
    let j2 = canonical_json(&val).unwrap();
    assert_eq!(j1, j2);
    assert!(j1.starts_with("{\"a\":"));
}

#[test]
fn canonical_json_btreemap_is_sorted() {
    let mut map = BTreeMap::new();
    map.insert("zebra", 1);
    map.insert("alpha", 2);
    map.insert("middle", 3);
    let json = canonical_json(&map).unwrap();
    assert!(json.starts_with("{\"alpha\":"));
}

#[test]
fn canonical_json_full_receipt_is_deterministic() {
    let r = full_receipt();
    let j1 = canonicalize(&r).unwrap();
    let j2 = canonicalize(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_json_includes_all_fields() {
    let r = full_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"backend\""));
    assert!(json.contains("\"meta\""));
    assert!(json.contains("\"outcome\""));
    assert!(json.contains("\"trace\""));
    assert!(json.contains("\"artifacts\""));
    assert!(json.contains("\"usage\""));
    assert!(json.contains("\"verification\""));
    assert!(json.contains("\"capabilities\""));
    assert!(json.contains("\"mode\""));
}

// ===========================================================================
// 5. Receipt chains (parent_receipt_id linking / chronological ordering)
// ===========================================================================

#[test]
fn chain_new_is_empty() {
    let chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert!(chain.latest().is_none());
}

#[test]
fn chain_push_single() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(!chain.is_empty());
}

#[test]
fn chain_push_multiple_chronological() {
    let mut chain = ReceiptChain::new();
    let ts1 = fixed_ts();
    let ts2 = ts1 + Duration::seconds(60);
    let ts3 = ts2 + Duration::seconds(60);

    for ts in [ts1, ts2, ts3] {
        chain
            .push(
                ReceiptBuilder::new("mock")
                    .started_at(ts)
                    .finished_at(ts)
                    .with_hash()
                    .unwrap(),
            )
            .unwrap();
    }
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_verify_single() {
    let mut chain = ReceiptChain::new();
    chain
        .push(ReceiptBuilder::new("mock").with_hash().unwrap())
        .unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_verify_empty_returns_error() {
    use abp_receipt::ChainError;
    let chain = ReceiptChain::new();
    assert!(matches!(chain.verify(), Err(ChainError::EmptyChain)));
}

#[test]
fn chain_rejects_duplicate_run_id() {
    use abp_receipt::ChainError;
    let mut chain = ReceiptChain::new();
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a").run_id(id).with_hash().unwrap();
    let r2 = ReceiptBuilder::new("b").run_id(id).with_hash().unwrap();
    chain.push(r1).unwrap();
    assert!(matches!(
        chain.push(r2),
        Err(ChainError::DuplicateId { .. })
    ));
}

#[test]
fn chain_rejects_out_of_order() {
    use abp_receipt::ChainError;
    let mut chain = ReceiptChain::new();
    let ts_later = fixed_ts() + Duration::hours(1);
    let ts_earlier = fixed_ts();

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
fn chain_detects_tampered_hash_on_push() {
    use abp_receipt::ChainError;
    let mut chain = ReceiptChain::new();
    let mut r = ReceiptBuilder::new("mock").with_hash().unwrap();
    r.outcome = Outcome::Failed; // tamper
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_latest_returns_last_pushed() {
    let mut chain = ReceiptChain::new();
    let ts1 = fixed_ts();
    let ts2 = ts1 + Duration::seconds(60);

    let r1 = ReceiptBuilder::new("first")
        .started_at(ts1)
        .finished_at(ts1)
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("second")
        .started_at(ts2)
        .finished_at(ts2)
        .with_hash()
        .unwrap();
    let expected = r2.meta.run_id;

    chain.push(r1).unwrap();
    chain.push(r2).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, expected);
}

#[test]
fn chain_iter_yields_in_order() {
    let mut chain = ReceiptChain::new();
    let ts1 = fixed_ts();
    let ts2 = ts1 + Duration::seconds(60);

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
fn chain_verify_multiple_receipts() {
    let mut chain = ReceiptChain::new();
    let ts1 = fixed_ts();
    let ts2 = ts1 + Duration::minutes(1);
    let ts3 = ts2 + Duration::minutes(1);

    for (ts, backend) in [(ts1, "a"), (ts2, "b"), (ts3, "c")] {
        chain
            .push(
                ReceiptBuilder::new(backend)
                    .started_at(ts)
                    .finished_at(ts)
                    .with_hash()
                    .unwrap(),
            )
            .unwrap();
    }
    assert!(chain.verify().is_ok());
    assert_eq!(chain.len(), 3);
}

#[test]
fn chain_allows_no_hash_on_push() {
    let mut chain = ReceiptChain::new();
    let r = ReceiptBuilder::new("mock").build();
    // Receipt without hash should be accepted (verify_hash returns true for None)
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_work_order_id_linking() {
    let wo = WorkOrderBuilder::new("test task").build();
    let r = ReceiptBuilder::new("mock")
        .work_order_id(wo.id)
        .with_hash()
        .unwrap();
    assert_eq!(r.meta.work_order_id, wo.id);
}

// ===========================================================================
// 6. Receipt validation (non-empty fields, valid timestamps)
// ===========================================================================

#[test]
fn validation_valid_receipt_passes() {
    use abp_core::validate::validate_receipt;
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(validate_receipt(&r).is_ok());
}

#[test]
fn validation_empty_backend_id_fails() {
    use abp_core::validate::{ValidationError, validate_receipt};
    let r = ReceiptBuilder::new("").build();
    let errs = validate_receipt(&r).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ValidationError::EmptyBackendId))
    );
}

#[test]
fn validation_wrong_contract_version_fails() {
    use abp_core::validate::validate_receipt;
    let mut r = builder_receipt();
    r.meta.contract_version = "wrong/v99".into();
    let errs = validate_receipt(&r).unwrap_err();
    assert!(!errs.is_empty());
}

#[test]
fn validation_started_after_finished_fails() {
    use abp_core::validate::validate_receipt;
    let t1 = fixed_ts();
    let t2 = t1 - Duration::hours(1);
    let mut r = builder_receipt();
    r.meta.started_at = t1;
    r.meta.finished_at = t2;
    let errs = validate_receipt(&r).unwrap_err();
    assert!(!errs.is_empty());
}

#[test]
fn validation_tampered_hash_fails() {
    use abp_core::validate::{ValidationError, validate_receipt};
    let mut r = builder_receipt().with_hash().unwrap();
    r.receipt_sha256 = Some("tampered".into());
    let errs = validate_receipt(&r).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| matches!(e, ValidationError::InvalidHash { .. }))
    );
}

#[test]
fn validation_receipt_without_hash_passes() {
    use abp_core::validate::validate_receipt;
    let r = ReceiptBuilder::new("mock").build();
    assert!(validate_receipt(&r).is_ok());
}

#[test]
fn validation_full_receipt_passes() {
    use abp_core::validate::validate_receipt;
    let r = full_receipt().with_hash().unwrap();
    assert!(validate_receipt(&r).is_ok());
}

// ===========================================================================
// 7. Round-trip: Receipt → JSON → Receipt preserves all fields
// ===========================================================================

#[test]
fn roundtrip_minimal_receipt() {
    let r = minimal_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.meta.contract_version, r2.meta.contract_version);
    assert_eq!(r.meta.duration_ms, r2.meta.duration_ms);
}

#[test]
fn roundtrip_full_receipt() {
    let r = full_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(r.meta.run_id, r2.meta.run_id);
    assert_eq!(r.meta.work_order_id, r2.meta.work_order_id);
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.backend.backend_version, r2.backend.backend_version);
    assert_eq!(r.backend.adapter_version, r2.backend.adapter_version);
    assert_eq!(r.outcome, r2.outcome);
    assert_eq!(r.mode, r2.mode);
    assert_eq!(r.trace.len(), r2.trace.len());
    assert_eq!(r.artifacts.len(), r2.artifacts.len());
    assert_eq!(r.usage_raw, r2.usage_raw);
    assert_eq!(r.usage.input_tokens, r2.usage.input_tokens);
    assert_eq!(r.usage.output_tokens, r2.usage.output_tokens);
    assert_eq!(r.verification.harness_ok, r2.verification.harness_ok);
    assert_eq!(r.verification.git_diff, r2.verification.git_diff);
    assert_eq!(r.verification.git_status, r2.verification.git_status);
    assert_eq!(r.capabilities.len(), r2.capabilities.len());
}

#[test]
fn roundtrip_preserves_hash() {
    let r = builder_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn roundtrip_hash_still_verifies_after_deser() {
    let r = builder_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
}

#[test]
fn roundtrip_preserves_none_hash() {
    let r = builder_receipt();
    assert!(r.receipt_sha256.is_none());
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(r2.receipt_sha256.is_none());
}

#[test]
fn roundtrip_value_intermediate() {
    let r = full_receipt();
    let val = serde_json::to_value(&r).unwrap();
    let r2: Receipt = serde_json::from_value(val).unwrap();
    assert_eq!(r.backend.id, r2.backend.id);
    assert_eq!(r.outcome, r2.outcome);
}

#[test]
fn roundtrip_canonical_form_to_receipt() {
    let r = builder_receipt();
    let canonical = canonicalize(&r).unwrap();
    // canonical has receipt_sha256 = null, should deserialize fine
    let r2: Receipt = serde_json::from_str(&canonical).unwrap();
    assert!(r2.receipt_sha256.is_none());
    assert_eq!(r.backend.id, r2.backend.id);
}

#[test]
fn roundtrip_pretty_json() {
    let r = builder_receipt();
    let pretty = serde_json::to_string_pretty(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&pretty).unwrap();
    assert_eq!(r.meta.run_id, r2.meta.run_id);
}

#[test]
fn roundtrip_with_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".to_string(), serde_json::json!({"foo": "bar"}));
    let evt = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };

    let r = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .add_trace_event(evt)
        .build();

    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.trace.len(), r2.trace.len());
}

// ===========================================================================
// 8. Multiple receipts with same data produce same hash
// ===========================================================================

#[test]
fn same_data_same_hash_minimal() {
    let r1 = minimal_receipt();
    let r2 = minimal_receipt();
    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn same_data_same_hash_full() {
    let r1 = full_receipt();
    let r2 = full_receipt();
    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn same_data_same_hash_builder() {
    let build = || {
        ReceiptBuilder::new("deterministic")
            .run_id(nil_uuid())
            .work_order_id(nil_uuid())
            .outcome(Outcome::Failed)
            .started_at(fixed_ts())
            .finished_at(fixed_ts())
            .build()
    };
    assert_eq!(
        compute_hash(&build()).unwrap(),
        compute_hash(&build()).unwrap()
    );
}

#[test]
fn same_data_same_hash_core_function() {
    let r1 = minimal_receipt();
    let r2 = minimal_receipt();
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn same_data_same_hash_with_hash_method() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = minimal_receipt().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn same_data_same_canonical_json() {
    let r1 = full_receipt();
    let r2 = full_receipt();
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

#[test]
fn same_data_same_hash_with_capabilities() {
    let build = || {
        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::Streaming, SupportLevel::Native);
        caps.insert(Capability::ToolBash, SupportLevel::Emulated);
        ReceiptBuilder::new("cap-test")
            .run_id(nil_uuid())
            .work_order_id(nil_uuid())
            .started_at(fixed_ts())
            .finished_at(fixed_ts())
            .capabilities(caps)
            .build()
    };
    assert_eq!(
        compute_hash(&build()).unwrap(),
        compute_hash(&build()).unwrap()
    );
}

// ===========================================================================
// 9. Different receipts produce different hashes
// ===========================================================================

#[test]
fn different_outcome_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.outcome = Outcome::Failed;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_backend_id_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.id = "other".into();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_run_id_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.run_id = Uuid::new_v4();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_work_order_id_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.work_order_id = Uuid::new_v4();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_timestamp_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.started_at = fixed_ts2();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_duration_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.duration_ms = 9999;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_mode_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.mode = ExecutionMode::Passthrough;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_usage_raw_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.usage_raw = serde_json::json!({"tokens": 42});
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_usage_normalized_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.usage.input_tokens = Some(100);
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_backend_version_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.backend_version = Some("1.0".into());
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_adapter_version_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.adapter_version = Some("0.1".into());
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_trace_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.trace.push(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_artifacts_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "out.patch".into(),
    });
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_verification_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.verification.harness_ok = true;
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_contract_version_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.contract_version = "abp/v99".into();
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn different_capabilities_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.capabilities
        .insert(Capability::Streaming, SupportLevel::Native);
    assert_ne!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

// ===========================================================================
// 10. Empty/minimal receipt hashing
// ===========================================================================

#[test]
fn empty_backend_id_hashes_stably() {
    let mut r = minimal_receipt();
    r.backend.id = String::new();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn minimal_receipt_hash_is_64_hex() {
    let h = compute_hash(&minimal_receipt()).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn minimal_receipt_with_hash_verifies() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn empty_trace_and_artifacts_hash_stable() {
    let r = minimal_receipt();
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn nil_uuid_receipt_hashes_deterministically() {
    let r = minimal_receipt();
    assert_eq!(r.meta.run_id, Uuid::nil());
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn default_usage_receipt_hashes() {
    let r = minimal_receipt();
    assert!(r.usage.input_tokens.is_none());
    assert!(r.usage.output_tokens.is_none());
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

// ===========================================================================
// 11. Receipt with all optional fields populated
// ===========================================================================

#[test]
fn full_receipt_hashes_stably() {
    let h1 = compute_hash(&full_receipt()).unwrap();
    let h2 = compute_hash(&full_receipt()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn full_receipt_with_hash_verifies() {
    let r = full_receipt().with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn full_receipt_canonical_includes_optional_fields() {
    let r = full_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"git_diff\""));
    assert!(json.contains("\"git_status\""));
    assert!(json.contains("\"harness_ok\":true"));
    assert!(json.contains("\"input_tokens\":100"));
    assert!(json.contains("\"output_tokens\":50"));
}

#[test]
fn full_receipt_roundtrip_hash_stable() {
    let r = full_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&r2));
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn full_receipt_diff_with_itself_is_empty() {
    let r = full_receipt();
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
}

// ===========================================================================
// 12. verify_hash tests
// ===========================================================================

#[test]
fn verify_hash_passes_for_correct_hash() {
    let r = builder_receipt().with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_passes_when_no_hash_stored() {
    let r = builder_receipt();
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_fails_for_tampered_outcome() {
    let mut r = builder_receipt().with_hash().unwrap();
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_for_tampered_backend() {
    let mut r = builder_receipt().with_hash().unwrap();
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_for_garbage() {
    let mut r = builder_receipt();
    r.receipt_sha256 = Some("not_a_real_hash".into());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_fails_for_empty_string_hash() {
    let mut r = builder_receipt();
    r.receipt_sha256 = Some(String::new());
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_after_recompute() {
    let r = builder_receipt();
    let h = compute_hash(&r).unwrap();
    let mut r2 = r;
    r2.receipt_sha256 = Some(h);
    assert!(verify_hash(&r2));
}

// ===========================================================================
// 13. Diff tests
// ===========================================================================

#[test]
fn diff_identical_is_empty() {
    let r = builder_receipt();
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn diff_detects_outcome_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.outcome = Outcome::Failed;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
}

#[test]
fn diff_detects_backend_id_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.backend.id = "new-backend".into();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
}

#[test]
fn diff_detects_trace_length_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.trace.push(make_event(AgentEventKind::RunStarted {
        message: "hi".into(),
    }));
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "trace.len"));
}

#[test]
fn diff_detects_verification_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.verification.harness_ok = true;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "verification"));
}

#[test]
fn diff_detects_usage_raw_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.usage_raw = serde_json::json!({"changed": true});
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "usage_raw"));
}

#[test]
fn diff_detects_mode_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.mode = ExecutionMode::Passthrough;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "mode"));
}

#[test]
fn diff_detects_multiple_changes() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.outcome = Outcome::Partial;
    b.backend.id = "changed".into();
    b.verification.harness_ok = true;
    let d = diff_receipts(&a, &b);
    assert!(d.len() >= 3);
}

// ===========================================================================
// 14. sha256_hex utility
// ===========================================================================

#[test]
fn sha256_hex_known_value() {
    // SHA-256 of "hello" is well known.
    let h = sha256_hex(b"hello");
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_hex_empty_input() {
    let h = sha256_hex(b"");
    assert_eq!(
        h,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_length_always_64() {
    for input in [b"a".as_slice(), b"longer input", b"\x00\x01\x02"] {
        assert_eq!(sha256_hex(input).len(), 64);
    }
}

// ===========================================================================
// 15. Edge cases
// ===========================================================================

#[test]
fn unicode_backend_id_hashes() {
    let mut r = minimal_receipt();
    r.backend.id = "バックエンド🚀".into();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn unicode_in_canonical_json() {
    let mut r = minimal_receipt();
    r.backend.id = "日本語テスト".into();
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("日本語テスト"));
}

#[test]
fn large_trace_receipt_hashes() {
    let mut builder = ReceiptBuilder::new("mock")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts());
    for i in 0..200 {
        builder = builder.add_trace_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        });
    }
    let r = builder.build();
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(r.trace.len(), 200);
}

#[test]
fn large_trace_receipt_with_hash_verifies() {
    let mut builder = ReceiptBuilder::new("mock")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts());
    for i in 0..100 {
        builder = builder.add_trace_event(AgentEvent {
            ts: fixed_ts(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token {i}"),
            },
            ext: None,
        });
    }
    let r = builder.with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn many_artifacts_receipt_hashes() {
    let mut r = minimal_receipt();
    for i in 0..50 {
        r.artifacts.push(ArtifactRef {
            kind: "file".into(),
            path: format!("file_{i}.txt"),
        });
    }
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn special_chars_in_fields() {
    let mut r = minimal_receipt();
    r.backend.id = "backend with \"quotes\" and \\ backslashes".into();
    let json = canonicalize(&r).unwrap();
    // Should be valid JSON
    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn newlines_in_verification_diff() {
    let mut r = minimal_receipt();
    r.verification.git_diff = Some("line1\nline2\nline3\n".into());
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn empty_usage_raw_object() {
    let r = minimal_receipt();
    assert_eq!(r.usage_raw, serde_json::json!({}));
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn complex_usage_raw_nesting() {
    let mut r = minimal_receipt();
    r.usage_raw = serde_json::json!({
        "model": "gpt-4",
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        },
        "choices": [{"index": 0, "finish_reason": "stop"}]
    });
    let h1 = compute_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn tool_call_event_in_trace() {
    let evt = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc_001".into()),
            parent_tool_use_id: None,
            input: serde_json::json!({"path": "/foo/bar.rs"}),
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .add_trace_event(evt)
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn tool_result_event_in_trace() {
    let evt = AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc_001".into()),
            output: serde_json::json!({"content": "file contents"}),
            is_error: false,
        },
        ext: None,
    };
    let r = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .add_trace_event(evt)
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn file_changed_event_in_trace() {
    let evt = make_event(AgentEventKind::FileChanged {
        path: "src/main.rs".into(),
        summary: "Added main function".into(),
    });
    let r = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .add_trace_event(evt)
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.trace.len(), r2.trace.len());
}

#[test]
fn command_executed_event_in_trace() {
    let evt = make_event(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("test result: ok".into()),
    });
    let r = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .add_trace_event(evt)
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn warning_and_error_events_in_trace() {
    let warn = make_event(AgentEventKind::Warning {
        message: "something odd".into(),
    });
    let err = make_event(AgentEventKind::Error {
        message: "fatal".into(),
        error_code: None,
    });
    let r = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .add_trace_event(warn)
        .add_trace_event(err)
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn chain_error_display_messages() {
    use abp_receipt::ChainError;

    assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
    assert!(
        ChainError::HashMismatch { index: 5 }
            .to_string()
            .contains("5")
    );
    assert!(
        ChainError::BrokenLink { index: 2 }
            .to_string()
            .contains("2")
    );
    assert!(
        ChainError::DuplicateId { id: Uuid::nil() }
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn receipt_clone_produces_identical_hash() {
    let r = full_receipt().with_hash().unwrap();
    let r2 = r.clone();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    assert!(verify_hash(&r2));
}

#[test]
fn hash_independent_of_serialization_order_via_btreemap() {
    // BTreeMap ensures key ordering is always sorted, so insertion order
    // does not matter for canonical JSON.
    let mut caps1 = CapabilityManifest::new();
    caps1.insert(Capability::ToolRead, SupportLevel::Native);
    caps1.insert(Capability::Streaming, SupportLevel::Native);

    let mut caps2 = CapabilityManifest::new();
    caps2.insert(Capability::Streaming, SupportLevel::Native);
    caps2.insert(Capability::ToolRead, SupportLevel::Native);

    let r1 = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .capabilities(caps1)
        .build();

    let r2 = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .capabilities(caps2)
        .build();

    assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
}

#[test]
fn hash_with_all_outcome_variants() {
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    let hashes: Vec<String> = outcomes
        .iter()
        .map(|o| {
            let mut r = minimal_receipt();
            r.outcome = o.clone();
            compute_hash(&r).unwrap()
        })
        .collect();

    // All three must be distinct
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    assert_ne!(hashes[0], hashes[2]);
}

#[test]
fn hash_with_all_execution_modes() {
    let modes = [ExecutionMode::Mapped, ExecutionMode::Passthrough];
    let hashes: Vec<String> = modes
        .iter()
        .map(|m| {
            let mut r = minimal_receipt();
            r.mode = *m;
            compute_hash(&r).unwrap()
        })
        .collect();
    assert_ne!(hashes[0], hashes[1]);
}

#[test]
fn hash_with_many_capabilities() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::ToolEdit, SupportLevel::Native);
    caps.insert(Capability::ToolBash, SupportLevel::Emulated);
    caps.insert(Capability::ToolGlob, SupportLevel::Emulated);
    caps.insert(Capability::McpClient, SupportLevel::Native);

    let r = ReceiptBuilder::new("multi-cap")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .capabilities(caps)
        .build();

    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    // Verify deterministic
    assert_eq!(h, compute_hash(&r).unwrap());
}

#[test]
fn receipt_with_estimated_cost() {
    let r = ReceiptBuilder::new("cost-test")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .usage(UsageNormalized {
            estimated_cost_usd: Some(1.23),
            ..Default::default()
        })
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_with_request_units() {
    let r = ReceiptBuilder::new("units-test")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .usage(UsageNormalized {
            request_units: Some(42),
            ..Default::default()
        })
        .build();
    let h = compute_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn diff_full_receipt_clone_is_empty() {
    let r = full_receipt();
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
}

#[test]
fn diff_detects_backend_version_change() {
    let a = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .backend_version("1.0")
        .build();
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
fn diff_detects_adapter_version_change() {
    let a = ReceiptBuilder::new("x")
        .run_id(nil_uuid())
        .work_order_id(nil_uuid())
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .adapter_version("0.1")
        .build();
    let mut b = a.clone();
    b.backend.adapter_version = Some("0.2".into());
    let d = diff_receipts(&a, &b);
    assert!(
        d.changes
            .iter()
            .any(|c| c.field == "backend.adapter_version")
    );
}

#[test]
fn diff_detects_started_at_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.meta.started_at = fixed_ts2();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "meta.started_at"));
}

#[test]
fn diff_detects_finished_at_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.meta.finished_at = fixed_ts2();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "meta.finished_at"));
}

#[test]
fn diff_detects_duration_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.meta.duration_ms = 99999;
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "meta.duration_ms"));
}

#[test]
fn diff_detects_run_id_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.meta.run_id = Uuid::new_v4();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "meta.run_id"));
}

#[test]
fn diff_detects_work_order_id_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.meta.work_order_id = Uuid::new_v4();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "meta.work_order_id"));
}

#[test]
fn diff_detects_contract_version_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.meta.contract_version = "abp/v99".into();
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "meta.contract_version"));
}

#[test]
fn diff_detects_artifacts_length_change() {
    let a = builder_receipt();
    let mut b = a.clone();
    b.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "out.log".into(),
    });
    let d = diff_receipts(&a, &b);
    assert!(d.changes.iter().any(|c| c.field == "artifacts.len"));
}

// ===========================================================================
// 16. Cross-crate consistency
// ===========================================================================

#[test]
fn core_receipt_hash_eq_receipt_crate_compute_hash() {
    let r = minimal_receipt();
    let core_h = receipt_hash(&r).unwrap();
    let crate_h = compute_hash(&r).unwrap();
    assert_eq!(core_h, crate_h);
}

#[test]
fn core_with_hash_eq_receipt_builder_with_hash() {
    let r_core = minimal_receipt().with_hash().unwrap();
    let r_crate = {
        let mut r = minimal_receipt();
        r.receipt_sha256 = Some(compute_hash(&r).unwrap());
        r
    };
    assert_eq!(r_core.receipt_sha256, r_crate.receipt_sha256);
}

#[test]
fn core_canonical_json_eq_receipt_canonicalize() {
    let r = minimal_receipt();
    // core: canonical_json serializes the whole receipt (doesn't null receipt_sha256)
    // receipt crate: canonicalize nulls receipt_sha256
    // For a receipt with receipt_sha256 = None, canonical_json will have "receipt_sha256":null,
    // and canonicalize will also have "receipt_sha256":null — they should match.
    let core_json = canonical_json(&r).unwrap();
    let crate_json = canonicalize(&r).unwrap();
    // Both should produce the same output since receipt_sha256 is None
    assert_eq!(core_json, crate_json);
}

#[test]
fn chain_ten_receipts_in_order() {
    let mut chain = ReceiptChain::new();
    let base = fixed_ts();
    for i in 0..10 {
        let ts = base + Duration::minutes(i);
        chain
            .push(
                ReceiptBuilder::new(format!("backend-{i}"))
                    .started_at(ts)
                    .finished_at(ts)
                    .with_hash()
                    .unwrap(),
            )
            .unwrap();
    }
    assert_eq!(chain.len(), 10);
    assert!(chain.verify().is_ok());
}

#[test]
fn hash_stability_across_clone() {
    let r = full_receipt();
    let h1 = compute_hash(&r).unwrap();
    let cloned = r.clone();
    let h2 = compute_hash(&cloned).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn outcome_serde_roundtrip() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let json = serde_json::to_string(&outcome).unwrap();
        let deserialized: Outcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, deserialized);
    }
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deserialized);
    }
}

#[test]
fn contract_version_present_in_receipt() {
    let r = builder_receipt();
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn receipt_sha256_none_by_default() {
    let r = builder_receipt();
    assert!(r.receipt_sha256.is_none());
}
