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
//! Comprehensive deep tests for the receipt system in `abp-core`.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ExecutionMode, Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, canonical_json, receipt_hash, sha256_hex,
};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixed_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap()
}

fn fixed_time_later() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 42).unwrap()
}

/// Build a minimal receipt with deterministic timestamps and nil UUIDs.
fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .build()
}

/// Build a receipt with all fields populated.
fn full_receipt() -> Receipt {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    ReceiptBuilder::new("full-backend")
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .work_order_id(Uuid::nil())
        .outcome(Outcome::Complete)
        .backend_version("2.0.0")
        .adapter_version("1.0.0")
        .capabilities(caps)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(json!({"prompt_tokens": 100, "completion_tokens": 50}))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.005),
        })
        .add_trace_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::RunStarted {
                message: "starting".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: fixed_time_later(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "output.patch".into(),
        })
        .verification(VerificationReport {
            git_diff: Some("diff --git a/foo b/foo".into()),
            git_status: Some("M foo".into()),
            harness_ok: true,
        })
        .build()
}

/// Build two structurally identical receipts (same run_id too).
fn twin_receipts() -> (Receipt, Receipt) {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    let shared_id = Uuid::nil();
    a.meta.run_id = shared_id;
    b.meta.run_id = shared_id;
    (a, b)
}

fn roundtrip_json(receipt: &Receipt) -> Receipt {
    let json = serde_json::to_string(receipt).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

// =========================================================================
// 1. Receipt construction
// =========================================================================

#[test]
fn construct_minimal_receipt() {
    let r = minimal_receipt();
    assert_eq!(r.backend.id, "mock");
    assert_eq!(r.outcome, Outcome::Complete);
    assert!(r.receipt_sha256.is_none());
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
}

#[test]
fn construct_full_receipt() {
    let r = full_receipt();
    assert_eq!(r.backend.id, "full-backend");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.0.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("1.0.0"));
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.trace.len(), 2);
    assert_eq!(r.artifacts.len(), 1);
    assert_eq!(r.artifacts[0].kind, "patch");
    assert!(r.verification.harness_ok);
    assert!(r.verification.git_diff.is_some());
    assert!(r.verification.git_status.is_some());
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(50));
    assert_eq!(r.usage.cache_read_tokens, Some(10));
    assert_eq!(r.usage.cache_write_tokens, Some(5));
    assert_eq!(r.usage.request_units, Some(1));
    assert_eq!(r.usage.estimated_cost_usd, Some(0.005));
}

#[test]
fn construct_receipt_with_partial_outcome() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Partial).build();
    assert_eq!(r.outcome, Outcome::Partial);
}

#[test]
fn construct_receipt_with_failed_outcome() {
    let r = ReceiptBuilder::new("b").outcome(Outcome::Failed).build();
    assert_eq!(r.outcome, Outcome::Failed);
}

#[test]
fn builder_default_mode_is_mapped() {
    let r = ReceiptBuilder::new("b").build();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[test]
fn builder_passthrough_mode() {
    let r = ReceiptBuilder::new("b")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn builder_sets_contract_version() {
    let r = minimal_receipt();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn builder_computes_duration_ms() {
    let r = minimal_receipt();
    assert_eq!(r.meta.duration_ms, 42_000);
}

#[test]
fn builder_zero_duration_when_same_timestamps() {
    let t = fixed_time();
    let r = ReceiptBuilder::new("b")
        .started_at(t)
        .finished_at(t)
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn builder_work_order_id_set() {
    let id = Uuid::new_v4();
    let r = ReceiptBuilder::new("b").work_order_id(id).build();
    assert_eq!(r.meta.work_order_id, id);
}

#[test]
fn builder_run_id_is_unique() {
    let a = ReceiptBuilder::new("b").build();
    let b = ReceiptBuilder::new("b").build();
    assert_ne!(a.meta.run_id, b.meta.run_id);
}

#[test]
fn builder_with_hash_produces_hashed_receipt() {
    let r = ReceiptBuilder::new("b").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

// =========================================================================
// 2. receipt_hash() determinism
// =========================================================================

#[test]
fn hash_is_deterministic() {
    let (a, b) = twin_receipts();
    let h1 = receipt_hash(&a).unwrap();
    let h2 = receipt_hash(&b).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn hash_is_64_hex_chars() {
    let r = minimal_receipt();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_same_receipt_twice_identical() {
    let r = minimal_receipt();
    assert_eq!(receipt_hash(&r).unwrap(), receipt_hash(&r).unwrap());
}

#[test]
fn hash_stability_across_recomputation() {
    let r = minimal_receipt();
    let first = receipt_hash(&r).unwrap();
    for _ in 0..50 {
        assert_eq!(first, receipt_hash(&r).unwrap());
    }
}

// =========================================================================
// 3. Self-referential prevention: receipt_sha256 nullified before hashing
// =========================================================================

#[test]
fn hash_ignores_existing_receipt_sha256() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let h1 = receipt_hash(&r).unwrap();

    r.receipt_sha256 = Some("deadbeef".into());
    let h2 = receipt_hash(&r).unwrap();

    assert_eq!(h1, h2, "hash must not depend on receipt_sha256 field");
}

#[test]
fn hash_nullifies_before_hashing_proof() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let hash_without = receipt_hash(&r).unwrap();

    r.receipt_sha256 = Some(hash_without.clone());
    let hash_with = receipt_hash(&r).unwrap();
    assert_eq!(hash_without, hash_with);
}

#[test]
fn hash_with_arbitrary_sha256_field_still_matches() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let baseline = receipt_hash(&r).unwrap();

    for fake in ["", "abc", "0".repeat(64).as_str(), "💀🔑"] {
        r.receipt_sha256 = Some(fake.to_string());
        assert_eq!(baseline, receipt_hash(&r).unwrap());
    }
}

#[test]
fn hash_with_none_sha256_matches_null() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    r.receipt_sha256 = None;
    let h1 = receipt_hash(&r).unwrap();

    // Manually set to None again.
    r.receipt_sha256 = None;
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

// =========================================================================
// 4. with_hash() behaviour
// =========================================================================

#[test]
fn with_hash_returns_non_none() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn with_hash_len_is_64() {
    let r = minimal_receipt().with_hash().unwrap();
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[test]
fn with_hash_is_consistent_with_receipt_hash() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let expected = receipt_hash(&r).unwrap();
    let hashed = r.with_hash().unwrap();
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(expected.as_str()));
}

#[test]
fn with_hash_idempotent() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let first = r.clone().with_hash().unwrap();
    let second = first.clone().with_hash().unwrap();
    assert_eq!(first.receipt_sha256, second.receipt_sha256);
}

#[test]
fn with_hash_preserves_all_fields() {
    let before = full_receipt();
    let backend_id = before.backend.id.clone();
    let outcome = before.outcome.clone();
    let trace_len = before.trace.len();
    let after = before.with_hash().unwrap();
    assert_eq!(after.backend.id, backend_id);
    assert_eq!(after.outcome, outcome);
    assert_eq!(after.trace.len(), trace_len);
}

// =========================================================================
// 5. Identical receipts → same hash; different receipts → different hash
// =========================================================================

#[test]
fn identical_receipts_same_hash() {
    let (a, b) = twin_receipts();
    assert_eq!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_backend_id_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.backend.id = "other".into();
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_outcome_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    a.outcome = Outcome::Complete;
    b.outcome = Outcome::Failed;
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_work_order_id_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    a.meta.work_order_id = Uuid::nil();
    b.meta.work_order_id = Uuid::from_u128(1);
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_run_id_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::from_u128(999);
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_mode_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    a.mode = ExecutionMode::Mapped;
    b.mode = ExecutionMode::Passthrough;
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_duration_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.meta.duration_ms = 9999;
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_trace_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::Warning {
            message: "x".into(),
        },
        ext: None,
    });
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_artifacts_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "run.log".into(),
    });
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_verification_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.verification.harness_ok = true;
    b.verification.git_diff = Some("+ line".into());
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_usage_raw_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.usage_raw = json!({"tokens": 999});
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_usage_normalized_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.usage.input_tokens = Some(42);
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_capabilities_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.capabilities
        .insert(Capability::ToolBash, SupportLevel::Native);
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_contract_version_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    b.meta.contract_version = "abp/v99".into();
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_backend_version_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    a.backend.backend_version = None;
    b.backend.backend_version = Some("1.2.3".into());
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn different_adapter_version_different_hash() {
    let mut a = minimal_receipt();
    let mut b = minimal_receipt();
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    a.backend.adapter_version = None;
    b.backend.adapter_version = Some("0.1.0".into());
    assert_ne!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

// =========================================================================
// 6. Serde roundtrip
// =========================================================================

#[test]
fn serde_roundtrip_minimal() {
    let orig = minimal_receipt();
    let rt = roundtrip_json(&orig);
    assert_eq!(rt.backend.id, orig.backend.id);
    assert_eq!(rt.outcome, orig.outcome);
    assert!(rt.receipt_sha256.is_none());
}

#[test]
fn serde_roundtrip_full() {
    let orig = full_receipt();
    let rt = roundtrip_json(&orig);
    assert_eq!(rt.backend.id, orig.backend.id);
    assert_eq!(rt.outcome, orig.outcome);
    assert_eq!(rt.trace.len(), orig.trace.len());
    assert_eq!(rt.artifacts.len(), orig.artifacts.len());
    assert_eq!(rt.usage.input_tokens, orig.usage.input_tokens);
    assert!(rt.verification.harness_ok);
}

#[test]
fn serde_roundtrip_with_hash() {
    let orig = minimal_receipt().with_hash().unwrap();
    let rt = roundtrip_json(&orig);
    assert_eq!(rt.receipt_sha256, orig.receipt_sha256);
}

#[test]
fn serde_roundtrip_preserves_hash_after_rehash() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let hashed = r.with_hash().unwrap();
    let rt = roundtrip_json(&hashed);
    let rehashed = rt.with_hash().unwrap();
    assert_eq!(hashed.receipt_sha256, rehashed.receipt_sha256);
}

#[test]
fn serde_to_value_and_back() {
    let r = full_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    let back: Receipt = serde_json::from_value(val).unwrap();
    assert_eq!(back.backend.id, r.backend.id);
}

#[test]
fn serde_pretty_print_and_back() {
    let r = minimal_receipt();
    let pretty = serde_json::to_string_pretty(&r).unwrap();
    let back: Receipt = serde_json::from_str(&pretty).unwrap();
    assert_eq!(back.outcome, r.outcome);
}

#[test]
fn serde_roundtrip_outcome_complete() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    let rt: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, Outcome::Complete);
}

#[test]
fn serde_roundtrip_outcome_partial() {
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    let rt: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, Outcome::Partial);
}

#[test]
fn serde_roundtrip_outcome_failed() {
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    let rt: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, Outcome::Failed);
}

#[test]
fn serde_roundtrip_execution_mode_mapped() {
    let json = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ExecutionMode::Mapped);
}

#[test]
fn serde_roundtrip_execution_mode_passthrough() {
    let json = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    let rt: ExecutionMode = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, ExecutionMode::Passthrough);
}

// =========================================================================
// 7. Outcome enum coverage
// =========================================================================

#[test]
fn outcome_complete_serde_name() {
    assert_eq!(
        serde_json::to_string(&Outcome::Complete).unwrap(),
        r#""complete""#
    );
}

#[test]
fn outcome_partial_serde_name() {
    assert_eq!(
        serde_json::to_string(&Outcome::Partial).unwrap(),
        r#""partial""#
    );
}

#[test]
fn outcome_failed_serde_name() {
    assert_eq!(
        serde_json::to_string(&Outcome::Failed).unwrap(),
        r#""failed""#
    );
}

#[test]
fn outcome_deser_from_string() {
    let c: Outcome = serde_json::from_str(r#""complete""#).unwrap();
    let p: Outcome = serde_json::from_str(r#""partial""#).unwrap();
    let f: Outcome = serde_json::from_str(r#""failed""#).unwrap();
    assert_eq!(c, Outcome::Complete);
    assert_eq!(p, Outcome::Partial);
    assert_eq!(f, Outcome::Failed);
}

#[test]
fn outcome_eq_variants() {
    assert_eq!(Outcome::Complete, Outcome::Complete);
    assert_eq!(Outcome::Partial, Outcome::Partial);
    assert_eq!(Outcome::Failed, Outcome::Failed);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Complete, Outcome::Partial);
    assert_ne!(Outcome::Partial, Outcome::Failed);
}

// =========================================================================
// 8. BTreeMap metadata / deterministic ordering
// =========================================================================

#[test]
fn btreemap_capabilities_deterministic_json() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let json1 = serde_json::to_string(&caps).unwrap();
    let json2 = serde_json::to_string(&caps).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn receipt_with_capabilities_hash_deterministic() {
    let mut r1 = minimal_receipt();
    r1.meta.run_id = Uuid::nil();
    r1.capabilities
        .insert(Capability::ToolRead, SupportLevel::Native);
    r1.capabilities
        .insert(Capability::Streaming, SupportLevel::Emulated);

    let mut r2 = r1.clone();
    // BTreeMap already deterministic, but verify.
    r2.capabilities
        .insert(Capability::Streaming, SupportLevel::Emulated);
    r2.capabilities
        .insert(Capability::ToolRead, SupportLevel::Native);

    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn usage_raw_btreemap_ordering() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    r.usage_raw = json!({"z": 1, "a": 2, "m": 3});
    let h1 = receipt_hash(&r).unwrap();
    r.usage_raw = json!({"a": 2, "m": 3, "z": 1});
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2, "JSON object key order must not affect hash");
}

// =========================================================================
// 9. Empty fields
// =========================================================================

#[test]
fn receipt_empty_trace() {
    let r = minimal_receipt();
    assert!(r.trace.is_empty());
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_empty_artifacts() {
    let r = minimal_receipt();
    assert!(r.artifacts.is_empty());
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_empty_capabilities() {
    let r = minimal_receipt();
    assert!(r.capabilities.is_empty());
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_empty_usage_raw() {
    let r = ReceiptBuilder::new("b").usage_raw(json!({})).build();
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_default_usage_normalized() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn receipt_default_verification() {
    let v = VerificationReport::default();
    assert!(v.git_diff.is_none());
    assert!(v.git_status.is_none());
    assert!(!v.harness_ok);
}

#[test]
fn receipt_empty_backend_versions() {
    let r = ReceiptBuilder::new("b").build();
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
}

#[test]
fn receipt_null_usage_raw() {
    let r = ReceiptBuilder::new("b").usage_raw(Value::Null).build();
    assert!(receipt_hash(&r).is_ok());
}

// =========================================================================
// 10. Large content
// =========================================================================

#[test]
fn receipt_large_trace() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    for i in 0..500 {
        r.trace.push(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::AssistantDelta {
                text: format!("token_{i}"),
            },
            ext: None,
        });
    }
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn receipt_large_usage_raw() {
    let mut map = serde_json::Map::new();
    for i in 0..1000 {
        map.insert(format!("key_{i}"), json!(i));
    }
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    r.usage_raw = Value::Object(map);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_large_git_diff() {
    let big_diff = "a".repeat(100_000);
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    r.verification.git_diff = Some(big_diff);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_many_artifacts() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    for i in 0..200 {
        r.artifacts.push(ArtifactRef {
            kind: "file".into(),
            path: format!("out/{i}.txt"),
        });
    }
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_many_capabilities() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let caps = vec![
        Capability::Streaming,
        Capability::ToolRead,
        Capability::ToolWrite,
        Capability::ToolEdit,
        Capability::ToolBash,
        Capability::ToolGlob,
        Capability::ToolGrep,
        Capability::ToolWebSearch,
        Capability::ToolWebFetch,
        Capability::ToolAskUser,
        Capability::HooksPreToolUse,
        Capability::HooksPostToolUse,
        Capability::SessionResume,
        Capability::SessionFork,
        Capability::Checkpointing,
        Capability::StructuredOutputJsonSchema,
        Capability::McpClient,
        Capability::McpServer,
        Capability::ToolUse,
        Capability::ExtendedThinking,
        Capability::ImageInput,
        Capability::PdfInput,
        Capability::CodeExecution,
        Capability::Logprobs,
        Capability::SeedDeterminism,
        Capability::StopSequences,
    ];
    for cap in caps {
        r.capabilities.insert(cap, SupportLevel::Native);
    }
    assert!(receipt_hash(&r).is_ok());
}

// =========================================================================
// 11. Edge cases: unicode, special characters, null fields
// =========================================================================

#[test]
fn receipt_unicode_backend_id() {
    let r = ReceiptBuilder::new("バックエンド🚀").build();
    assert_eq!(r.backend.id, "バックエンド🚀");
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_unicode_trace_message() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage {
            text: "こんにちは世界 🌍".into(),
        },
        ext: None,
    });
    assert!(receipt_hash(&r).is_ok());
    let rt = roundtrip_json(&r);
    if let AgentEventKind::AssistantMessage { text } = &rt.trace[0].kind {
        assert_eq!(text, "こんにちは世界 🌍");
    } else {
        panic!("unexpected event kind");
    }
}

#[test]
fn receipt_special_chars_in_path() {
    let mut r = minimal_receipt();
    r.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "path/with spaces/and\"quotes\"/file.txt".into(),
    });
    assert!(receipt_hash(&r).is_ok());
    let rt = roundtrip_json(&r);
    assert_eq!(rt.artifacts[0].path, r.artifacts[0].path);
}

#[test]
fn receipt_empty_string_backend_id() {
    let r = ReceiptBuilder::new("").build();
    assert_eq!(r.backend.id, "");
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_newlines_in_git_diff() {
    let mut r = minimal_receipt();
    r.verification.git_diff = Some("line1\nline2\nline3\n".into());
    assert!(receipt_hash(&r).is_ok());
    let rt = roundtrip_json(&r);
    assert_eq!(rt.verification.git_diff, r.verification.git_diff);
}

#[test]
fn receipt_null_json_in_usage_raw() {
    let mut r = minimal_receipt();
    r.usage_raw = json!(null);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_array_in_usage_raw() {
    let mut r = minimal_receipt();
    r.usage_raw = json!([1, 2, 3]);
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_nested_json_in_usage_raw() {
    let mut r = minimal_receipt();
    r.usage_raw = json!({"a": {"b": {"c": [1, 2, {"d": true}]}}});
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_ext_field_on_event() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor": "test"}));
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    });
    assert!(receipt_hash(&r).is_ok());
    let rt = roundtrip_json(&r);
    assert!(rt.trace[0].ext.is_some());
}

#[test]
fn receipt_emoji_in_all_string_fields() {
    let mut r = minimal_receipt();
    r.backend.id = "🤖".into();
    r.backend.backend_version = Some("✨1.0".into());
    r.backend.adapter_version = Some("🔧0.1".into());
    r.verification.git_diff = Some("📝 diff".into());
    r.verification.git_status = Some("📁 status".into());
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_control_chars_in_string() {
    let mut r = minimal_receipt();
    r.backend.id = "back\tend\n\r\0".into();
    assert!(receipt_hash(&r).is_ok());
}

// =========================================================================
// 12. Hash stability after mutation and reversal
// =========================================================================

#[test]
fn hash_unchanged_after_clone() {
    let r = minimal_receipt();
    let cloned = r.clone();
    // Both have random run_ids, make them the same to compare
    let mut a = r;
    let mut b = cloned;
    a.meta.run_id = Uuid::nil();
    b.meta.run_id = Uuid::nil();
    assert_eq!(receipt_hash(&a).unwrap(), receipt_hash(&b).unwrap());
}

#[test]
fn hash_changes_on_trace_append_then_reverts() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    let original_hash = receipt_hash(&r).unwrap();

    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::Warning {
            message: "oops".into(),
        },
        ext: None,
    });
    let modified_hash = receipt_hash(&r).unwrap();
    assert_ne!(original_hash, modified_hash);

    r.trace.pop();
    let reverted_hash = receipt_hash(&r).unwrap();
    assert_eq!(original_hash, reverted_hash);
}

#[test]
fn hash_sensitive_to_trace_order() {
    let mut r1 = minimal_receipt();
    r1.meta.run_id = Uuid::nil();
    r1.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage { text: "A".into() },
        ext: None,
    });
    r1.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantMessage { text: "B".into() },
        ext: None,
    });

    let mut r2 = r1.clone();
    r2.trace.reverse();

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// =========================================================================
// 13. Schema validation of Receipt JSON
// =========================================================================

#[test]
fn receipt_json_has_expected_top_level_keys() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    let obj = val.as_object().unwrap();
    let expected = [
        "meta",
        "backend",
        "capabilities",
        "mode",
        "usage_raw",
        "usage",
        "trace",
        "artifacts",
        "verification",
        "outcome",
        "receipt_sha256",
    ];
    for key in &expected {
        assert!(obj.contains_key(*key), "missing top-level key: {key}");
    }
}

#[test]
fn receipt_json_meta_keys() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    let meta = val["meta"].as_object().unwrap();
    for key in &[
        "run_id",
        "work_order_id",
        "contract_version",
        "started_at",
        "finished_at",
        "duration_ms",
    ] {
        assert!(meta.contains_key(*key), "missing meta key: {key}");
    }
}

#[test]
fn receipt_json_backend_keys() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    let backend = val["backend"].as_object().unwrap();
    assert!(backend.contains_key("id"));
    assert!(backend.contains_key("backend_version"));
    assert!(backend.contains_key("adapter_version"));
}

#[test]
fn receipt_json_outcome_is_string() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["outcome"].is_string());
}

#[test]
fn receipt_json_receipt_sha256_null_when_unhashed() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["receipt_sha256"].is_null());
}

#[test]
fn receipt_json_receipt_sha256_string_when_hashed() {
    let r = minimal_receipt().with_hash().unwrap();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["receipt_sha256"].is_string());
}

#[test]
fn receipt_json_trace_is_array() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["trace"].is_array());
}

#[test]
fn receipt_json_artifacts_is_array() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["artifacts"].is_array());
}

#[test]
fn receipt_json_capabilities_is_object() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["capabilities"].is_object());
}

#[test]
fn receipt_json_mode_is_string() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["mode"].is_string());
}

#[test]
fn receipt_json_duration_ms_is_number() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    assert!(val["meta"]["duration_ms"].is_number());
}

#[test]
fn receipt_json_verification_keys() {
    let r = minimal_receipt();
    let val: Value = serde_json::to_value(&r).unwrap();
    let v = val["verification"].as_object().unwrap();
    assert!(v.contains_key("git_diff"));
    assert!(v.contains_key("git_status"));
    assert!(v.contains_key("harness_ok"));
}

// =========================================================================
// 14. canonical_json and sha256_hex helpers
// =========================================================================

#[test]
fn canonical_json_deterministic() {
    let r = minimal_receipt();
    let j1 = canonical_json(&r).unwrap();
    let j2 = canonical_json(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn sha256_hex_known_value() {
    // SHA-256 of empty string
    let h = sha256_hex(b"");
    assert_eq!(
        h,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_hello() {
    let h = sha256_hex(b"hello");
    assert_eq!(h.len(), 64);
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_hex_different_inputs_different_hashes() {
    let h1 = sha256_hex(b"a");
    let h2 = sha256_hex(b"b");
    assert_ne!(h1, h2);
}

// =========================================================================
// 15. AgentEvent variants in trace
// =========================================================================

#[test]
fn trace_with_tool_call_event() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc_1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "/tmp/x"}),
        },
        ext: None,
    });
    let rt = roundtrip_json(&r);
    assert_eq!(rt.trace.len(), 1);
}

#[test]
fn trace_with_tool_result_event() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::ToolResult {
            tool_name: "read_file".into(),
            tool_use_id: Some("tc_1".into()),
            output: json!("file contents"),
            is_error: false,
        },
        ext: None,
    });
    let rt = roundtrip_json(&r);
    if let AgentEventKind::ToolResult { is_error, .. } = &rt.trace[0].kind {
        assert!(!is_error);
    } else {
        panic!("wrong event kind");
    }
}

#[test]
fn trace_with_file_changed_event() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::FileChanged {
            path: "src/main.rs".into(),
            summary: "added function".into(),
        },
        ext: None,
    });
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn trace_with_command_executed_event() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::CommandExecuted {
            command: "cargo test".into(),
            exit_code: Some(0),
            output_preview: Some("all passed".into()),
        },
        ext: None,
    });
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn trace_with_error_event() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::Error {
            message: "something broke".into(),
            error_code: None,
        },
        ext: None,
    });
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn trace_with_assistant_delta() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantDelta {
            text: "token".into(),
        },
        ext: None,
    });
    let rt = roundtrip_json(&r);
    if let AgentEventKind::AssistantDelta { text } = &rt.trace[0].kind {
        assert_eq!(text, "token");
    } else {
        panic!("wrong event kind");
    }
}

#[test]
fn trace_with_warning_event() {
    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::Warning {
            message: "caution".into(),
        },
        ext: None,
    });
    let rt = roundtrip_json(&r);
    if let AgentEventKind::Warning { message } = &rt.trace[0].kind {
        assert_eq!(message, "caution");
    } else {
        panic!("wrong event kind");
    }
}

// =========================================================================
// 16. VerificationReport edge cases
// =========================================================================

#[test]
fn verification_all_none() {
    let v = VerificationReport {
        git_diff: None,
        git_status: None,
        harness_ok: false,
    };
    let json = serde_json::to_string(&v).unwrap();
    let rt: VerificationReport = serde_json::from_str(&json).unwrap();
    assert!(rt.git_diff.is_none());
    assert!(!rt.harness_ok);
}

#[test]
fn verification_all_populated() {
    let v = VerificationReport {
        git_diff: Some("diff".into()),
        git_status: Some("status".into()),
        harness_ok: true,
    };
    let json = serde_json::to_string(&v).unwrap();
    let rt: VerificationReport = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.git_diff.as_deref(), Some("diff"));
    assert!(rt.harness_ok);
}

// =========================================================================
// 17. UsageNormalized edge cases
// =========================================================================

#[test]
fn usage_normalized_all_some() {
    let u = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(200),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(25),
        request_units: Some(3),
        estimated_cost_usd: Some(1.23),
    };
    let json = serde_json::to_string(&u).unwrap();
    let rt: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.input_tokens, Some(100));
    assert_eq!(rt.estimated_cost_usd, Some(1.23));
}

#[test]
fn usage_normalized_zero_values() {
    let u = UsageNormalized {
        input_tokens: Some(0),
        output_tokens: Some(0),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        request_units: Some(0),
        estimated_cost_usd: Some(0.0),
    };
    let json = serde_json::to_string(&u).unwrap();
    let rt: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.input_tokens, Some(0));
}

#[test]
fn usage_normalized_large_values() {
    let u = UsageNormalized {
        input_tokens: Some(u64::MAX),
        output_tokens: Some(u64::MAX),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: Some(f64::MAX),
    };
    let json = serde_json::to_string(&u).unwrap();
    let rt: UsageNormalized = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.input_tokens, Some(u64::MAX));
}

// =========================================================================
// 18. BackendIdentity edge cases
// =========================================================================

#[test]
fn backend_identity_minimal() {
    let b = BackendIdentity {
        id: "test".into(),
        backend_version: None,
        adapter_version: None,
    };
    let json = serde_json::to_string(&b).unwrap();
    let rt: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.id, "test");
}

#[test]
fn backend_identity_full() {
    let b = BackendIdentity {
        id: "sidecar:node".into(),
        backend_version: Some("18.0.0".into()),
        adapter_version: Some("0.5.0".into()),
    };
    let json = serde_json::to_string(&b).unwrap();
    let rt: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.backend_version.as_deref(), Some("18.0.0"));
    assert_eq!(rt.adapter_version.as_deref(), Some("0.5.0"));
}

// =========================================================================
// 19. RunMetadata edge cases
// =========================================================================

#[test]
fn run_metadata_serde_roundtrip() {
    let m = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: fixed_time(),
        finished_at: fixed_time_later(),
        duration_ms: 42_000,
    };
    let json = serde_json::to_string(&m).unwrap();
    let rt: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.run_id, Uuid::nil());
    assert_eq!(rt.duration_ms, 42_000);
    assert_eq!(rt.contract_version, CONTRACT_VERSION);
}

#[test]
fn run_metadata_timestamps_preserved() {
    let m = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: fixed_time(),
        finished_at: fixed_time_later(),
        duration_ms: 0,
    };
    let json = serde_json::to_string(&m).unwrap();
    let rt: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.started_at, fixed_time());
    assert_eq!(rt.finished_at, fixed_time_later());
}

// =========================================================================
// 20. ArtifactRef
// =========================================================================

#[test]
fn artifact_ref_serde_roundtrip() {
    let a = ArtifactRef {
        kind: "patch".into(),
        path: "output/fix.patch".into(),
    };
    let json = serde_json::to_string(&a).unwrap();
    let rt: ArtifactRef = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.kind, "patch");
    assert_eq!(rt.path, "output/fix.patch");
}

// =========================================================================
// 21. ExecutionMode default & serde
// =========================================================================

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn execution_mode_serde_mapped() {
    let s = serde_json::to_string(&ExecutionMode::Mapped).unwrap();
    assert_eq!(s, r#""mapped""#);
}

#[test]
fn execution_mode_serde_passthrough() {
    let s = serde_json::to_string(&ExecutionMode::Passthrough).unwrap();
    assert_eq!(s, r#""passthrough""#);
}

// =========================================================================
// 22. SupportLevel and Capability serde
// =========================================================================

#[test]
fn support_level_native_serde() {
    let json = serde_json::to_string(&SupportLevel::Native).unwrap();
    let rt: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(rt, SupportLevel::Native));
}

#[test]
fn support_level_emulated_serde() {
    let json = serde_json::to_string(&SupportLevel::Emulated).unwrap();
    let rt: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(rt, SupportLevel::Emulated));
}

#[test]
fn support_level_unsupported_serde() {
    let json = serde_json::to_string(&SupportLevel::Unsupported).unwrap();
    let rt: SupportLevel = serde_json::from_str(&json).unwrap();
    assert!(matches!(rt, SupportLevel::Unsupported));
}

#[test]
fn support_level_restricted_serde() {
    let sl = SupportLevel::Restricted {
        reason: "policy".into(),
    };
    let json = serde_json::to_string(&sl).unwrap();
    let rt: SupportLevel = serde_json::from_str(&json).unwrap();
    if let SupportLevel::Restricted { reason } = rt {
        assert_eq!(reason, "policy");
    } else {
        panic!("expected Restricted");
    }
}

// =========================================================================
// 23. ContractError from receipt_hash
// =========================================================================

// receipt_hash should never fail with valid receipts, but we can test the Ok path
#[test]
fn receipt_hash_returns_ok_for_valid_receipt() {
    let r = minimal_receipt();
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_hash_returns_ok_for_full_receipt() {
    let r = full_receipt();
    assert!(receipt_hash(&r).is_ok());
}

// =========================================================================
// 24. Cross-field interaction tests
// =========================================================================

#[test]
fn hash_with_all_optional_usage_fields_set() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    r.usage = UsageNormalized {
        input_tokens: Some(10),
        output_tokens: Some(20),
        cache_read_tokens: Some(5),
        cache_write_tokens: Some(3),
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    };
    let h1 = receipt_hash(&r).unwrap();

    r.usage.estimated_cost_usd = Some(0.02);
    let h2 = receipt_hash(&r).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn hash_with_tool_call_and_result_pair() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::ToolCall {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            parent_tool_use_id: None,
            input: json!({"command": "ls"}),
        },
        ext: None,
    });
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::ToolResult {
            tool_name: "bash".into(),
            tool_use_id: Some("t1".into()),
            output: json!("file1\nfile2"),
            is_error: false,
        },
        ext: None,
    });
    assert!(receipt_hash(&r).is_ok());
}

#[test]
fn receipt_with_mixed_event_types_in_trace() {
    let mut r = minimal_receipt();
    r.meta.run_id = Uuid::nil();
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    });
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::AssistantDelta { text: "tok".into() },
        ext: None,
    });
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::FileChanged {
            path: "x.rs".into(),
            summary: "edit".into(),
        },
        ext: None,
    });
    r.trace.push(AgentEvent {
        ts: fixed_time(),
        kind: AgentEventKind::RunCompleted {
            message: "fin".into(),
        },
        ext: None,
    });
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

// =========================================================================
// 25. Deserialization from raw JSON strings
// =========================================================================

#[test]
fn deser_receipt_from_json_string() {
    let r = minimal_receipt();
    let json_str = serde_json::to_string(&r).unwrap();
    let parsed: Receipt = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.backend.id, "mock");
}

#[test]
fn deser_outcome_from_lowercase() {
    let cases = [
        (r#""complete""#, Outcome::Complete),
        (r#""partial""#, Outcome::Partial),
        (r#""failed""#, Outcome::Failed),
    ];
    for (input, expected) in cases {
        let parsed: Outcome = serde_json::from_str(input).unwrap();
        assert_eq!(parsed, expected);
    }
}

#[test]
fn deser_invalid_outcome_fails() {
    let result = serde_json::from_str::<Outcome>(r#""unknown""#);
    assert!(result.is_err());
}

#[test]
fn deser_invalid_execution_mode_fails() {
    let result = serde_json::from_str::<ExecutionMode>(r#""turbo""#);
    assert!(result.is_err());
}

// =========================================================================
// 26. Builder chaining
// =========================================================================

#[test]
fn builder_full_chain() {
    let wo_id = Uuid::new_v4();
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::Streaming, SupportLevel::Native);

    let r = ReceiptBuilder::new("chain-test")
        .backend_version("1.0")
        .adapter_version("0.1")
        .outcome(Outcome::Partial)
        .work_order_id(wo_id)
        .started_at(fixed_time())
        .finished_at(fixed_time_later())
        .capabilities(caps)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(json!({"x": 1}))
        .usage(UsageNormalized {
            input_tokens: Some(5),
            output_tokens: Some(10),
            ..UsageNormalized::default()
        })
        .verification(VerificationReport {
            git_diff: Some("d".into()),
            git_status: None,
            harness_ok: true,
        })
        .add_trace_event(AgentEvent {
            ts: fixed_time(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .build();

    assert_eq!(r.backend.id, "chain-test");
    assert_eq!(r.backend.backend_version.as_deref(), Some("1.0"));
    assert_eq!(r.outcome, Outcome::Partial);
    assert_eq!(r.meta.work_order_id, wo_id);
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
    assert!(r.verification.harness_ok);
    assert_eq!(r.usage.input_tokens, Some(5));
}

#[test]
fn builder_backend_id_override() {
    let r = ReceiptBuilder::new("first").backend_id("second").build();
    assert_eq!(r.backend.id, "second");
}

#[test]
fn builder_with_hash_shortcut() {
    let r = ReceiptBuilder::new("mock").with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}
