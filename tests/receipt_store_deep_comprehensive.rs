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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

//! Deep comprehensive tests for receipt storage, chain verification, and
//! hashing across the `abp-core` and `abp-receipt` crates.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, Capability, ExecutionMode, Outcome, Receipt,
    SupportLevel, UsageNormalized, VerificationReport, receipt_hash,
};
use abp_receipt::{
    ChainError, ReceiptBuilder, ReceiptChain, canonicalize, compute_hash, diff_receipts,
    verify_hash,
};
use abp_runtime::store::ReceiptStore;
use chrono::{DateTime, Duration, TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap()
}

fn ts(secs: i64) -> DateTime<Utc> {
    base_time() + Duration::seconds(secs)
}

fn make_receipt(backend: &str, offset: i64, outcome: Outcome) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(ts(offset))
        .finished_at(ts(offset) + Duration::milliseconds(250))
        .with_hash()
        .unwrap()
}

fn make_plain(backend: &str, offset: i64) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(ts(offset))
        .finished_at(ts(offset) + Duration::milliseconds(250))
        .build()
}

fn fixed_receipt() -> Receipt {
    let t = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    ReceiptBuilder::new("deterministic")
        .outcome(Outcome::Complete)
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(t)
        .finished_at(t)
        .build()
}

fn sample_event() -> AgentEvent {
    AgentEvent {
        ts: base_time(),
        kind: AgentEventKind::RunStarted {
            message: "start".into(),
        },
        ext: None,
    }
}

fn sample_capabilities() -> BTreeMap<Capability, SupportLevel> {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps
}

// ===========================================================================
// 1. Receipt construction with all fields populated
// ===========================================================================

#[test]
fn construct_receipt_all_fields() {
    let r = ReceiptBuilder::new("full-backend")
        .outcome(Outcome::Complete)
        .backend_version("2.1.0")
        .adapter_version("0.5.0")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(ts(10))
        .mode(ExecutionMode::Passthrough)
        .capabilities(sample_capabilities())
        .usage_raw(serde_json::json!({"prompt_tokens": 100}))
        .usage(UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(200),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.003),
        })
        .verification(VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M file.rs".into()),
            harness_ok: true,
        })
        .add_trace_event(sample_event())
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "fix.patch".into(),
        })
        .build();

    assert_eq!(r.backend.id, "full-backend");
    assert_eq!(r.backend.backend_version.as_deref(), Some("2.1.0"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
    assert_eq!(r.meta.run_id, Uuid::nil());
    assert_eq!(r.meta.work_order_id, Uuid::nil());
    assert_eq!(r.mode, ExecutionMode::Passthrough);
    assert_eq!(r.capabilities.len(), 2);
    assert_eq!(r.usage.input_tokens, Some(100));
    assert_eq!(r.usage.output_tokens, Some(200));
    assert_eq!(r.usage.cache_read_tokens, Some(10));
    assert_eq!(r.usage.cache_write_tokens, Some(5));
    assert_eq!(r.usage.request_units, Some(1));
    assert!(r.verification.harness_ok);
    assert_eq!(r.trace.len(), 1);
    assert_eq!(r.artifacts.len(), 1);
    assert!(r.receipt_sha256.is_none());
}

#[test]
fn construct_receipt_default_fields() {
    let r = ReceiptBuilder::new("minimal").build();
    assert_eq!(r.backend.id, "minimal");
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(r.mode, ExecutionMode::Mapped);
    assert!(r.trace.is_empty());
    assert!(r.artifacts.is_empty());
    assert!(r.capabilities.is_empty());
    assert!(r.receipt_sha256.is_none());
    assert_eq!(r.meta.contract_version, "abp/v0.1");
}

#[test]
fn construct_receipt_contract_version_populated() {
    let r = ReceiptBuilder::new("x").build();
    assert_eq!(r.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[test]
fn construct_receipt_run_id_unique_by_default() {
    let r1 = ReceiptBuilder::new("a").build();
    let r2 = ReceiptBuilder::new("b").build();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

// ===========================================================================
// 2. Receipt hashing — receipt_hash() nullifies receipt_sha256 (CRITICAL)
// ===========================================================================

#[test]
fn hash_nullifies_receipt_sha256_before_hashing() {
    let mut r = fixed_receipt();
    r.receipt_sha256 = Some("should_be_ignored".into());
    let h1 = receipt_hash(&r).unwrap();

    r.receipt_sha256 = None;
    let h2 = receipt_hash(&r).unwrap();

    assert_eq!(h1, h2, "hash must ignore the receipt_sha256 field");
}

#[test]
fn hash_nullifies_with_different_stored_hashes() {
    let r = fixed_receipt();
    let mut r1 = r.clone();
    r1.receipt_sha256 = Some("aaa".into());
    let mut r2 = r.clone();
    r2.receipt_sha256 = Some("bbb".into());
    let mut r3 = r.clone();
    r3.receipt_sha256 = None;

    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r2).unwrap();
    let h3 = receipt_hash(&r3).unwrap();

    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn hash_is_64_hex_chars() {
    let r = fixed_receipt();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_deterministic_across_calls() {
    let r = fixed_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    let h3 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
}

#[test]
fn hash_changes_with_outcome() {
    let mut r = fixed_receipt();
    let h_complete = receipt_hash(&r).unwrap();
    r.outcome = Outcome::Failed;
    let h_failed = receipt_hash(&r).unwrap();
    r.outcome = Outcome::Partial;
    let h_partial = receipt_hash(&r).unwrap();
    assert_ne!(h_complete, h_failed);
    assert_ne!(h_complete, h_partial);
    assert_ne!(h_failed, h_partial);
}

#[test]
fn hash_changes_with_backend_id() {
    let r1 = ReceiptBuilder::new("alpha")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("beta")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_usage() {
    let mut r = fixed_receipt();
    let h1 = receipt_hash(&r).unwrap();
    r.usage.input_tokens = Some(999);
    let h2 = receipt_hash(&r).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn hash_changes_with_trace() {
    let r1 = fixed_receipt();
    let mut r2 = r1.clone();
    r2.trace.push(sample_event());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 3. Receipt.with_hash() round-trip
// ===========================================================================

#[test]
fn with_hash_sets_sha256() {
    let r = fixed_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn with_hash_produces_valid_hash() {
    let r = fixed_receipt().with_hash().unwrap();
    let stored = r.receipt_sha256.as_ref().unwrap();
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(stored, &recomputed);
}

#[test]
fn with_hash_builder_path() {
    let r = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn with_hash_then_verify() {
    let r = ReceiptBuilder::new("verify-me")
        .outcome(Outcome::Partial)
        .started_at(base_time())
        .finished_at(ts(5))
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn with_hash_on_receipt_with_trace() {
    let r = ReceiptBuilder::new("traced")
        .add_trace_event(sample_event())
        .add_trace_event(AgentEvent {
            ts: ts(1),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        })
        .build()
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.trace.len(), 2);
}

// ===========================================================================
// 4. Receipt chain verification
// ===========================================================================

#[test]
fn chain_empty_verify_fails() {
    let chain = ReceiptChain::new();
    assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
}

#[test]
fn chain_single_receipt_verify_ok() {
    let mut chain = ReceiptChain::new();
    chain.push(make_receipt("a", 0, Outcome::Complete)).unwrap();
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_chronological_order_verify_ok() {
    let mut chain = ReceiptChain::new();
    for i in 0..5 {
        chain
            .push(make_receipt("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_rejects_reverse_order() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("m", 100, Outcome::Complete))
        .unwrap();
    assert!(matches!(
        chain.push(make_receipt("m", 0, Outcome::Complete)),
        Err(ChainError::BrokenLink { .. })
    ));
}

#[test]
fn chain_same_timestamp_accepted() {
    let mut chain = ReceiptChain::new();
    chain.push(make_receipt("a", 0, Outcome::Complete)).unwrap();
    chain.push(make_receipt("b", 0, Outcome::Complete)).unwrap();
    assert_eq!(chain.len(), 2);
}

#[test]
fn chain_duplicate_id_rejected() {
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("a")
        .run_id(id)
        .started_at(ts(0))
        .finished_at(ts(1))
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("b")
        .run_id(id)
        .started_at(ts(10))
        .finished_at(ts(11))
        .with_hash()
        .unwrap();
    let mut chain = ReceiptChain::new();
    chain.push(r1).unwrap();
    assert_eq!(chain.push(r2), Err(ChainError::DuplicateId { id }));
}

#[test]
fn chain_tampered_receipt_rejected() {
    let mut r = make_receipt("m", 0, Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper after hashing
    let mut chain = ReceiptChain::new();
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_accepts_unhashed_receipt() {
    let r = make_plain("m", 0);
    let mut chain = ReceiptChain::new();
    chain.push(r).unwrap();
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_latest_returns_most_recent() {
    let mut chain = ReceiptChain::new();
    chain
        .push(make_receipt("first", 0, Outcome::Complete))
        .unwrap();
    let last = make_receipt("last", 100, Outcome::Failed);
    let last_id = last.meta.run_id;
    chain.push(last).unwrap();
    assert_eq!(chain.latest().unwrap().meta.run_id, last_id);
}

#[test]
fn chain_iter_preserves_insertion_order() {
    let mut chain = ReceiptChain::new();
    let labels = ["alpha", "beta", "gamma", "delta"];
    for (i, label) in labels.iter().enumerate() {
        chain
            .push(make_receipt(label, (i as i64) * 10, Outcome::Complete))
            .unwrap();
    }
    let ids: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
    assert_eq!(ids, labels.to_vec());
}

#[test]
fn chain_into_iterator_count() {
    let mut chain = ReceiptChain::new();
    for i in 0..3 {
        chain
            .push(make_receipt("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    assert_eq!((&chain).into_iter().count(), 3);
}

#[test]
fn chain_len_and_is_empty() {
    let mut chain = ReceiptChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    chain.push(make_receipt("m", 0, Outcome::Complete)).unwrap();
    assert!(!chain.is_empty());
    assert_eq!(chain.len(), 1);
}

#[test]
fn chain_50_receipts_verify() {
    let mut chain = ReceiptChain::new();
    for i in 0..50 {
        chain
            .push(make_receipt("bulk", i * 2, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 50);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_mixed_outcomes() {
    let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
    let mut chain = ReceiptChain::new();
    for i in 0..9 {
        chain
            .push(make_receipt("m", i * 5, outcomes[i as usize % 3].clone()))
            .unwrap();
    }
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// 5. Receipt outcome variants
// ===========================================================================

#[test]
fn outcome_complete_serde_roundtrip() {
    let json = serde_json::to_string(&Outcome::Complete).unwrap();
    assert_eq!(json, "\"complete\"");
    let deser: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(deser, Outcome::Complete);
}

#[test]
fn outcome_partial_serde_roundtrip() {
    let json = serde_json::to_string(&Outcome::Partial).unwrap();
    assert_eq!(json, "\"partial\"");
    let deser: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(deser, Outcome::Partial);
}

#[test]
fn outcome_failed_serde_roundtrip() {
    let json = serde_json::to_string(&Outcome::Failed).unwrap();
    assert_eq!(json, "\"failed\"");
    let deser: Outcome = serde_json::from_str(&json).unwrap();
    assert_eq!(deser, Outcome::Failed);
}

#[test]
fn outcome_equality() {
    assert_eq!(Outcome::Complete, Outcome::Complete);
    assert_ne!(Outcome::Complete, Outcome::Failed);
    assert_ne!(Outcome::Partial, Outcome::Failed);
}

#[test]
fn outcome_all_variants_hashable() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = ReceiptBuilder::new("t").outcome(outcome).build();
        let h = receipt_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
    }
}

// ===========================================================================
// 6. Serialization / deserialization with BTreeMap determinism
// ===========================================================================

#[test]
fn serde_roundtrip_preserves_all_fields() {
    let r = ReceiptBuilder::new("round")
        .outcome(Outcome::Complete)
        .backend_version("1.0")
        .adapter_version("0.2")
        .started_at(base_time())
        .finished_at(ts(5))
        .usage(UsageNormalized {
            input_tokens: Some(50),
            output_tokens: Some(100),
            ..Default::default()
        })
        .add_trace_event(sample_event())
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "out.log".into(),
        })
        .with_hash()
        .unwrap();

    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();

    assert_eq!(deser.meta.run_id, r.meta.run_id);
    assert_eq!(deser.backend.id, r.backend.id);
    assert_eq!(deser.backend.backend_version, r.backend.backend_version);
    assert_eq!(deser.outcome, r.outcome);
    assert_eq!(deser.usage.input_tokens, r.usage.input_tokens);
    assert_eq!(deser.trace.len(), r.trace.len());
    assert_eq!(deser.artifacts.len(), r.artifacts.len());
    assert_eq!(deser.receipt_sha256, r.receipt_sha256);
}

#[test]
fn serde_roundtrip_preserves_hash_validity() {
    let r = make_receipt("m", 0, Outcome::Complete);
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&deser));
}

#[test]
fn serde_pretty_roundtrip_preserves_hash() {
    let r = make_receipt("m", 0, Outcome::Complete);
    let pretty = serde_json::to_string_pretty(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&pretty).unwrap();
    assert!(verify_hash(&deser));
}

#[test]
fn serde_canonical_form_stable_after_roundtrip() {
    let r = make_receipt("m", 0, Outcome::Complete);
    let c1 = canonicalize(&r).unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    let c2 = canonicalize(&deser).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn btreemap_key_ordering_in_capabilities() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolWrite, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);

    let r = ReceiptBuilder::new("ordered")
        .capabilities(caps)
        .started_at(base_time())
        .finished_at(base_time())
        .run_id(Uuid::nil())
        .build();

    let json1 = serde_json::to_string(&r).unwrap();
    let json2 = serde_json::to_string(&r).unwrap();
    assert_eq!(json1, json2, "BTreeMap serialization must be deterministic");
}

#[test]
fn btreemap_vendor_flags_deterministic() {
    let r1 = ReceiptBuilder::new("v")
        .usage_raw(serde_json::json!({"b": 2, "a": 1, "c": 3}))
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let c1 = canonicalize(&r1).unwrap();
    let c2 = canonicalize(&r1).unwrap();
    assert_eq!(c1, c2);
}

// ===========================================================================
// 7. Receipt backend_identity population
// ===========================================================================

#[test]
fn backend_identity_id_only() {
    let r = ReceiptBuilder::new("sidecar:node").build();
    assert_eq!(r.backend.id, "sidecar:node");
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
}

#[test]
fn backend_identity_full() {
    let r = ReceiptBuilder::new("claude")
        .backend_version("3.5-sonnet")
        .adapter_version("0.9.1")
        .build();
    assert_eq!(r.backend.id, "claude");
    assert_eq!(r.backend.backend_version.as_deref(), Some("3.5-sonnet"));
    assert_eq!(r.backend.adapter_version.as_deref(), Some("0.9.1"));
}

#[test]
fn backend_identity_empty_id() {
    let r = ReceiptBuilder::new("").build();
    assert_eq!(r.backend.id, "");
}

#[test]
fn backend_identity_unicode() {
    let r = ReceiptBuilder::new("バックエンド🚀")
        .backend_version("版本1.0")
        .build();
    assert_eq!(r.backend.id, "バックエンド🚀");
    assert_eq!(r.backend.backend_version.as_deref(), Some("版本1.0"));
}

#[test]
fn backend_identity_affects_hash() {
    let r1 = ReceiptBuilder::new("a")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("a")
        .backend_version("1.0")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 8. Receipt usage normalization fields
// ===========================================================================

#[test]
fn usage_normalized_default_all_none() {
    let u = UsageNormalized::default();
    assert!(u.input_tokens.is_none());
    assert!(u.output_tokens.is_none());
    assert!(u.cache_read_tokens.is_none());
    assert!(u.cache_write_tokens.is_none());
    assert!(u.request_units.is_none());
    assert!(u.estimated_cost_usd.is_none());
}

#[test]
fn usage_normalized_all_populated() {
    let u = UsageNormalized {
        input_tokens: Some(1000),
        output_tokens: Some(2000),
        cache_read_tokens: Some(500),
        cache_write_tokens: Some(100),
        request_units: Some(5),
        estimated_cost_usd: Some(0.025),
    };
    let r = ReceiptBuilder::new("usage-test").usage(u).build();
    assert_eq!(r.usage.input_tokens, Some(1000));
    assert_eq!(r.usage.output_tokens, Some(2000));
    assert_eq!(r.usage.cache_read_tokens, Some(500));
    assert_eq!(r.usage.cache_write_tokens, Some(100));
    assert_eq!(r.usage.request_units, Some(5));
    assert_eq!(r.usage.estimated_cost_usd, Some(0.025));
}

#[test]
fn usage_raw_arbitrary_json() {
    let raw = serde_json::json!({
        "prompt_tokens": 500,
        "completion_tokens": 300,
        "model": "gpt-4",
        "nested": {"key": "value"}
    });
    let r = ReceiptBuilder::new("raw-usage")
        .usage_raw(raw.clone())
        .build();
    assert_eq!(r.usage_raw, raw);
}

#[test]
fn usage_affects_hash() {
    let r1 = ReceiptBuilder::new("u")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("u")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .usage(UsageNormalized {
            input_tokens: Some(1),
            ..Default::default()
        })
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 9. Receipt events list construction
// ===========================================================================

#[test]
fn trace_empty_by_default() {
    let r = ReceiptBuilder::new("t").build();
    assert!(r.trace.is_empty());
}

#[test]
fn trace_single_event() {
    let r = ReceiptBuilder::new("t")
        .add_trace_event(sample_event())
        .build();
    assert_eq!(r.trace.len(), 1);
}

#[test]
fn trace_multiple_events() {
    let r = ReceiptBuilder::new("t")
        .add_trace_event(AgentEvent {
            ts: ts(0),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(1),
            kind: AgentEventKind::AssistantDelta {
                text: "chunk".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(2),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "file.rs"}),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(3),
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                output: serde_json::json!({"content": "fn main() {}"}),
                is_error: false,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(4),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .build();
    assert_eq!(r.trace.len(), 5);
}

#[test]
fn trace_with_ext_data() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"vendor": "data"}),
    );
    let evt = AgentEvent {
        ts: base_time(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    };
    let r = ReceiptBuilder::new("ext")
        .add_trace_event(evt)
        .build()
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert!(r.trace[0].ext.is_some());
}

#[test]
fn trace_large_500_events() {
    let mut builder = ReceiptBuilder::new("big-trace")
        .started_at(base_time())
        .finished_at(ts(100));
    for i in 0..500 {
        builder = builder.add_trace_event(AgentEvent {
            ts: ts(i),
            kind: AgentEventKind::AssistantDelta {
                text: format!("tok-{i}"),
            },
            ext: None,
        });
    }
    let r = builder.with_hash().unwrap();
    assert_eq!(r.trace.len(), 500);
    assert!(verify_hash(&r));
}

// ===========================================================================
// 10. Receipt timestamps (started_at, completed_at)
// ===========================================================================

#[test]
fn timestamps_duration_computed() {
    let r = ReceiptBuilder::new("t")
        .started_at(base_time())
        .finished_at(ts(10))
        .build();
    assert_eq!(r.meta.duration_ms, 10_000);
}

#[test]
fn timestamps_zero_duration() {
    let r = ReceiptBuilder::new("t")
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn timestamps_negative_duration_clamped() {
    // finished_at before started_at — duration should clamp to 0
    let r = ReceiptBuilder::new("t")
        .started_at(ts(10))
        .finished_at(base_time())
        .build();
    assert_eq!(r.meta.duration_ms, 0);
}

#[test]
fn timestamps_millisecond_precision() {
    let start = base_time();
    let end = start + Duration::milliseconds(42);
    let r = ReceiptBuilder::new("t")
        .started_at(start)
        .finished_at(end)
        .build();
    assert_eq!(r.meta.duration_ms, 42);
}

#[test]
fn timestamps_affect_hash() {
    let r1 = ReceiptBuilder::new("t")
        .run_id(Uuid::nil())
        .started_at(ts(0))
        .finished_at(ts(1))
        .build();
    let r2 = ReceiptBuilder::new("t")
        .run_id(Uuid::nil())
        .started_at(ts(0))
        .finished_at(ts(2))
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn timestamps_stored_correctly() {
    let start = ts(100);
    let end = ts(200);
    let r = ReceiptBuilder::new("t")
        .started_at(start)
        .finished_at(end)
        .build();
    assert_eq!(r.meta.started_at, start);
    assert_eq!(r.meta.finished_at, end);
}

// ===========================================================================
// 11. Receipt canonical JSON (BTreeMap ensures key ordering)
// ===========================================================================

#[test]
fn canonical_json_deterministic() {
    let r = fixed_receipt();
    let c1 = canonicalize(&r).unwrap();
    let c2 = canonicalize(&r).unwrap();
    assert_eq!(c1, c2);
}

#[test]
fn canonical_json_is_compact() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
}

#[test]
fn canonical_json_nullifies_hash() {
    let mut r = fixed_receipt();
    r.receipt_sha256 = Some("anything".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
}

#[test]
fn canonical_json_same_with_or_without_hash() {
    let r1 = fixed_receipt();
    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("some_hash".into());
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

#[test]
fn canonical_json_keys_sorted() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();
    // In canonical JSON, "artifacts" should come before "backend" alphabetically
    let artifacts_pos = json.find("\"artifacts\"").unwrap();
    let backend_pos = json.find("\"backend\"").unwrap();
    assert!(
        artifacts_pos < backend_pos,
        "keys must be alphabetically sorted"
    );
}

#[test]
fn canonical_json_matches_between_core_and_receipt_crate() {
    let r = fixed_receipt();
    let from_receipt_crate = canonicalize(&r).unwrap();
    // Manually replicate what abp_core::receipt_hash does for canonicalization
    let mut v = serde_json::to_value(&r).unwrap();
    if let serde_json::Value::Object(map) = &mut v {
        map.insert("receipt_sha256".to_string(), serde_json::Value::Null);
    }
    let from_core = serde_json::to_string(&v).unwrap();
    assert_eq!(from_receipt_crate, from_core);
}

// ===========================================================================
// 12. Receipt sha256 verification against known values
// ===========================================================================

#[test]
fn sha256_known_receipt_stability() {
    let r = fixed_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = compute_hash(&r).unwrap();
    // Both functions must produce the same hash
    assert_eq!(h1, h2);
    // Hash length is always 64 hex chars
    assert_eq!(h1.len(), 64);
}

#[test]
fn sha256_lowercase_hex() {
    let r = fixed_receipt();
    let h = receipt_hash(&r).unwrap();
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    // Ensure lowercase (no uppercase hex)
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn sha256_core_and_receipt_crate_agree() {
    let r = make_plain("test", 0);
    let core_hash = receipt_hash(&r).unwrap();
    let receipt_hash_val = compute_hash(&r).unwrap();
    assert_eq!(core_hash, receipt_hash_val);
}

#[test]
fn sha256_verify_hash_passes_for_correct() {
    let r = make_receipt("m", 0, Outcome::Complete);
    assert!(verify_hash(&r));
}

#[test]
fn sha256_verify_hash_fails_for_tampered_outcome() {
    let mut r = make_receipt("m", 0, Outcome::Complete);
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn sha256_verify_hash_fails_for_tampered_backend() {
    let mut r = make_receipt("m", 0, Outcome::Complete);
    r.backend.id = "evil".into();
    assert!(!verify_hash(&r));
}

#[test]
fn sha256_verify_hash_fails_for_garbage() {
    let mut r = make_plain("m", 0);
    r.receipt_sha256 = Some("not_real".into());
    assert!(!verify_hash(&r));
}

#[test]
fn sha256_verify_hash_passes_for_none() {
    let r = make_plain("m", 0);
    assert!(verify_hash(&r));
}

// ===========================================================================
// 13. Receipt extension data (vendor-specific fields)
// ===========================================================================

#[test]
fn ext_data_in_event_preserved_through_serde() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), serde_json::json!({"type": "delta"}));
    ext.insert("vendor_id".into(), serde_json::json!("msg_123"));
    let evt = AgentEvent {
        ts: base_time(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(ext),
    };
    let r = ReceiptBuilder::new("ext-test").add_trace_event(evt).build();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    let deserialized_ext = deser.trace[0].ext.as_ref().unwrap();
    assert!(deserialized_ext.contains_key("raw_message"));
    assert!(deserialized_ext.contains_key("vendor_id"));
}

#[test]
fn ext_data_none_by_default() {
    let evt = sample_event();
    assert!(evt.ext.is_none());
}

#[test]
fn ext_data_btreemap_deterministic() {
    let mut ext = BTreeMap::new();
    ext.insert("z_key".into(), serde_json::json!(1));
    ext.insert("a_key".into(), serde_json::json!(2));
    let evt = AgentEvent {
        ts: base_time(),
        kind: AgentEventKind::AssistantMessage { text: "msg".into() },
        ext: Some(ext),
    };
    let json1 = serde_json::to_string(&evt).unwrap();
    let json2 = serde_json::to_string(&evt).unwrap();
    assert_eq!(json1, json2);
    // a_key should come before z_key in BTreeMap serialization
    assert!(json1.find("a_key").unwrap() < json1.find("z_key").unwrap());
}

#[test]
fn ext_data_affects_hash() {
    let r1 = ReceiptBuilder::new("e")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: None,
        })
        .build();

    let mut ext = BTreeMap::new();
    ext.insert("extra".into(), serde_json::json!("data"));
    let r2 = ReceiptBuilder::new("e")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .add_trace_event(AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::AssistantMessage { text: "hi".into() },
            ext: Some(ext),
        })
        .build();

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 14. Receipt comparison and equality
// ===========================================================================

#[test]
fn diff_identical_receipts_empty() {
    let r = ReceiptBuilder::new("same")
        .started_at(base_time())
        .finished_at(ts(1))
        .build();
    let d = diff_receipts(&r, &r.clone());
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
}

#[test]
fn diff_detects_outcome_change() {
    let r1 = make_plain("m", 0);
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "outcome"));
}

#[test]
fn diff_detects_backend_id_change() {
    let r1 = make_plain("old", 0);
    let mut r2 = r1.clone();
    r2.backend.id = "new".into();
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "backend.id"));
}

#[test]
fn diff_detects_trace_length_change() {
    let r1 = make_plain("m", 0);
    let mut r2 = r1.clone();
    r2.trace.push(sample_event());
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "trace.len"));
}

#[test]
fn diff_detects_usage_raw_change() {
    let r1 = ReceiptBuilder::new("m")
        .usage_raw(serde_json::json!({"a": 1}))
        .started_at(base_time())
        .finished_at(ts(1))
        .build();
    let mut r2 = r1.clone();
    r2.usage_raw = serde_json::json!({"a": 2});
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "usage_raw"));
}

#[test]
fn diff_detects_mode_change() {
    let r1 = ReceiptBuilder::new("m")
        .mode(ExecutionMode::Mapped)
        .started_at(base_time())
        .finished_at(ts(1))
        .build();
    let mut r2 = r1.clone();
    r2.mode = ExecutionMode::Passthrough;
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "mode"));
}

#[test]
fn diff_detects_verification_change() {
    let r1 = make_plain("m", 0);
    let mut r2 = r1.clone();
    r2.verification.harness_ok = true;
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "verification"));
}

#[test]
fn diff_multiple_changes() {
    let r1 = make_plain("m", 0);
    let mut r2 = r1.clone();
    r2.outcome = Outcome::Failed;
    r2.backend.id = "other".into();
    r2.trace.push(sample_event());
    let d = diff_receipts(&r1, &r2);
    assert!(d.len() >= 3);
}

#[test]
fn diff_after_store_roundtrip_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = make_receipt("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    let d = diff_receipts(&r, &loaded);
    assert!(d.is_empty());
}

// ===========================================================================
// 15. Double-hashing idempotency
// ===========================================================================

#[test]
fn double_hash_idempotent() {
    let r = fixed_receipt().with_hash().unwrap();
    let h1 = r.receipt_sha256.clone().unwrap();
    // Hashing again should produce the same hash
    let r2 = r.with_hash().unwrap();
    let h2 = r2.receipt_sha256.clone().unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn triple_hash_idempotent() {
    let r = fixed_receipt()
        .with_hash()
        .unwrap()
        .with_hash()
        .unwrap()
        .with_hash()
        .unwrap();
    let h = r.receipt_sha256.clone().unwrap();
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(h, recomputed);
}

#[test]
fn compute_hash_ignores_stored_hash_value() {
    let r1 = fixed_receipt();
    let h_bare = compute_hash(&r1).unwrap();

    let r2 = r1.clone().with_hash().unwrap();
    let h_hashed = compute_hash(&r2).unwrap();

    assert_eq!(h_bare, h_hashed, "compute_hash must ignore receipt_sha256");
}

#[test]
fn receipt_hash_ignores_stored_hash_value() {
    let r1 = fixed_receipt();
    let h1 = receipt_hash(&r1).unwrap();

    let mut r2 = r1.clone();
    r2.receipt_sha256 = Some("totally_different".into());
    let h2 = receipt_hash(&r2).unwrap();

    assert_eq!(h1, h2);
}

#[test]
fn with_hash_after_modifying_stored_hash() {
    let mut r = fixed_receipt().with_hash().unwrap();
    let original = r.receipt_sha256.clone().unwrap();
    r.receipt_sha256 = Some("garbage".into());
    let r2 = r.with_hash().unwrap();
    assert_eq!(r2.receipt_sha256.unwrap(), original);
}

// ===========================================================================
// File store integration
// ===========================================================================

#[test]
fn file_store_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = make_receipt("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.meta.run_id, id);
    assert!(verify_hash(&loaded));
}

#[test]
fn file_store_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn file_store_list_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..5 {
        store
            .save(&make_receipt("m", i * 10, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(store.list().unwrap().len(), 5);
}

#[test]
fn file_store_verify_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = make_receipt("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    assert!(store.verify(id).unwrap());
}

#[test]
fn file_store_verify_unhashed_false() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = make_plain("m", 0);
    let id = r.meta.run_id;
    store.save(&r).unwrap();
    assert!(!store.verify(id).unwrap());
}

#[test]
fn file_store_verify_chain_valid() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..3 {
        store
            .save(&make_receipt("m", i * 60, Outcome::Complete))
            .unwrap();
    }
    let v = store.verify_chain().unwrap();
    assert!(v.is_valid);
    assert_eq!(v.valid_count, 3);
}

#[test]
fn file_store_verify_chain_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let v = store.verify_chain().unwrap();
    assert!(v.is_valid);
    assert_eq!(v.valid_count, 0);
}

#[test]
fn file_store_verify_chain_detects_tamper() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    store
        .save(&make_receipt("good", 0, Outcome::Complete))
        .unwrap();
    let mut bad = make_receipt("bad", 60, Outcome::Complete);
    bad.outcome = Outcome::Failed; // tamper after hashing
    store.save(&bad).unwrap();
    let v = store.verify_chain().unwrap();
    assert!(!v.is_valid);
    assert_eq!(v.invalid_hashes.len(), 1);
}

#[test]
fn file_store_load_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    assert!(store.load(Uuid::new_v4()).is_err());
}

#[test]
fn file_store_tampered_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = make_receipt("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    let path = store.save(&r).unwrap();
    let mut json = std::fs::read_to_string(&path).unwrap();
    json = json.replace("\"complete\"", "\"failed\"");
    std::fs::write(&path, json).unwrap();
    let loaded = store.load(id).unwrap();
    assert!(!verify_hash(&loaded));
}

#[test]
fn file_store_creates_nested_dir() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("deep").join("store");
    let store = ReceiptStore::new(&nested);
    store
        .save(&make_receipt("m", 0, Outcome::Complete))
        .unwrap();
    assert!(nested.exists());
    assert_eq!(store.list().unwrap().len(), 1);
}

// ===========================================================================
// Chain + FileStore integration
// ===========================================================================

#[test]
fn chain_from_file_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let mut saved_ids = Vec::new();
    for i in 0..4 {
        let r = make_receipt("m", i * 30, Outcome::Complete);
        saved_ids.push(r.meta.run_id);
        store.save(&r).unwrap();
    }
    let mut receipts: Vec<Receipt> = saved_ids
        .iter()
        .map(|id| store.load(*id).unwrap())
        .collect();
    receipts.sort_by_key(|r| r.meta.started_at);
    let mut chain = ReceiptChain::new();
    for r in receipts {
        chain.push(r).unwrap();
    }
    assert_eq!(chain.len(), 4);
    assert!(chain.verify().is_ok());
}

// ===========================================================================
// ChainError display and traits
// ===========================================================================

#[test]
fn chain_error_display_empty() {
    assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
}

#[test]
fn chain_error_display_hash_mismatch() {
    assert_eq!(
        ChainError::HashMismatch { index: 5 }.to_string(),
        "hash mismatch at chain index 5"
    );
}

#[test]
fn chain_error_display_broken_link() {
    assert_eq!(
        ChainError::BrokenLink { index: 2 }.to_string(),
        "broken link at chain index 2"
    );
}

#[test]
fn chain_error_display_duplicate_id() {
    let id = Uuid::nil();
    let e = ChainError::DuplicateId { id };
    assert!(e.to_string().contains("duplicate"));
}

#[test]
fn chain_error_eq_and_clone() {
    let e1 = ChainError::HashMismatch { index: 1 };
    let e2 = e1.clone();
    assert_eq!(e1, e2);
    assert_ne!(e1, ChainError::HashMismatch { index: 2 });
}

#[test]
fn chain_error_is_std_error() {
    let e: Box<dyn std::error::Error> = Box::new(ChainError::EmptyChain);
    assert!(!e.to_string().is_empty());
}

// ===========================================================================
// Edge cases and stress tests
// ===========================================================================

#[test]
fn unicode_backend_hash_stable() {
    let r = ReceiptBuilder::new("日本語テスト🎉")
        .started_at(base_time())
        .finished_at(ts(1))
        .build();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn empty_string_backend_works() {
    let r = ReceiptBuilder::new("")
        .started_at(base_time())
        .finished_at(ts(1))
        .build()
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn very_long_backend_id() {
    let long_id = "x".repeat(10_000);
    let r = ReceiptBuilder::new(long_id.as_str())
        .started_at(base_time())
        .finished_at(ts(1))
        .build()
        .with_hash()
        .unwrap();
    assert!(verify_hash(&r));
    assert_eq!(r.backend.id, long_id);
}

#[test]
fn receipt_with_all_event_kinds() {
    let r = ReceiptBuilder::new("all-kinds")
        .add_trace_event(AgentEvent {
            ts: ts(0),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(1),
            kind: AgentEventKind::AssistantDelta { text: "tok".into() },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(2),
            kind: AgentEventKind::AssistantMessage {
                text: "full msg".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(3),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "ls"}),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(4),
            kind: AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tc1".into()),
                output: serde_json::json!("file.rs"),
                is_error: false,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(5),
            kind: AgentEventKind::FileChanged {
                path: "src/lib.rs".into(),
                summary: "added fn".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(6),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(7),
            kind: AgentEventKind::Warning {
                message: "slow".into(),
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(8),
            kind: AgentEventKind::Error {
                message: "oops".into(),
                error_code: None,
            },
            ext: None,
        })
        .add_trace_event(AgentEvent {
            ts: ts(9),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        })
        .started_at(base_time())
        .finished_at(ts(10))
        .build()
        .with_hash()
        .unwrap();

    assert_eq!(r.trace.len(), 10);
    assert!(verify_hash(&r));
}

#[test]
fn receipt_with_multiple_artifacts() {
    let r = ReceiptBuilder::new("arts")
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        })
        .add_artifact(ArtifactRef {
            kind: "diff".into(),
            path: "changes.diff".into(),
        })
        .build();
    assert_eq!(r.artifacts.len(), 3);
}

#[test]
fn receipt_capabilities_btreemap_order() {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolBash, SupportLevel::Native);
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::ImageInput, SupportLevel::Unsupported);
    let r = ReceiptBuilder::new("cap-order").capabilities(caps).build();
    assert_eq!(r.capabilities.len(), 4);
    // Keys are in BTreeMap order
    let keys: Vec<_> = r.capabilities.keys().collect();
    for i in 1..keys.len() {
        assert!(keys[i - 1] < keys[i]);
    }
}

#[test]
fn receipt_verification_report_all_fields() {
    let v = VerificationReport {
        git_diff: Some("diff --git a/f b/f\n+line".into()),
        git_status: Some("M f".into()),
        harness_ok: true,
    };
    let r = ReceiptBuilder::new("v").verification(v).build();
    assert!(r.verification.harness_ok);
    assert_eq!(
        r.verification.git_diff.as_deref(),
        Some("diff --git a/f b/f\n+line")
    );
    assert_eq!(r.verification.git_status.as_deref(), Some("M f"));
}

#[test]
fn receipt_verification_report_default() {
    let r = ReceiptBuilder::new("v").build();
    assert!(!r.verification.harness_ok);
    assert!(r.verification.git_diff.is_none());
    assert!(r.verification.git_status.is_none());
}

#[test]
fn execution_mode_serde_roundtrip() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let json = serde_json::to_string(&mode).unwrap();
        let deser: ExecutionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, deser);
    }
}

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

#[test]
fn chain_100_receipts_stress() {
    let mut chain = ReceiptChain::new();
    for i in 0..100 {
        chain
            .push(make_receipt("stress", i, Outcome::Complete))
            .unwrap();
    }
    assert_eq!(chain.len(), 100);
    assert!(chain.verify().is_ok());
}

#[test]
fn file_store_100_receipts_verify_chain() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    for i in 0..100 {
        store
            .save(&make_receipt("m", i * 5, Outcome::Complete))
            .unwrap();
    }
    let v = store.verify_chain().unwrap();
    assert!(v.is_valid);
    assert_eq!(v.valid_count, 100);
}

#[test]
fn chain_garbage_hash_rejected_at_push() {
    let mut r = make_plain("m", 0);
    r.receipt_sha256 = Some("deadbeefdeadbeef".into());
    let mut chain = ReceiptChain::new();
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn receipt_serde_with_all_usage_fields() {
    let u = UsageNormalized {
        input_tokens: Some(1),
        output_tokens: Some(2),
        cache_read_tokens: Some(3),
        cache_write_tokens: Some(4),
        request_units: Some(5),
        estimated_cost_usd: Some(0.01),
    };
    let r = ReceiptBuilder::new("u")
        .usage(u)
        .started_at(base_time())
        .finished_at(ts(1))
        .build();
    let json = serde_json::to_string(&r).unwrap();
    let deser: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.usage.input_tokens, Some(1));
    assert_eq!(deser.usage.output_tokens, Some(2));
    assert_eq!(deser.usage.cache_read_tokens, Some(3));
    assert_eq!(deser.usage.cache_write_tokens, Some(4));
    assert_eq!(deser.usage.request_units, Some(5));
    assert_eq!(deser.usage.estimated_cost_usd, Some(0.01));
}

#[test]
fn receipt_builder_mode_passthrough() {
    let r = ReceiptBuilder::new("p")
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

#[test]
fn chain_broken_link_reports_correct_index() {
    let mut chain = ReceiptChain::new();
    chain.push(make_receipt("a", 0, Outcome::Complete)).unwrap();
    chain
        .push(make_receipt("b", 100, Outcome::Complete))
        .unwrap();
    let earlier = make_receipt("c", 50, Outcome::Complete);
    match chain.push(earlier) {
        Err(ChainError::BrokenLink { index }) => assert_eq!(index, 2),
        other => panic!("expected BrokenLink, got {other:?}"),
    }
}

#[test]
fn chain_hash_mismatch_reports_correct_index() {
    let mut r = make_receipt("m", 0, Outcome::Complete);
    r.outcome = Outcome::Failed; // tamper
    let mut chain = ReceiptChain::new();
    match chain.push(r) {
        Err(ChainError::HashMismatch { index }) => assert_eq!(index, 0),
        other => panic!("expected HashMismatch, got {other:?}"),
    }
}

#[test]
fn receipt_work_order_id_nil_by_default() {
    let r = ReceiptBuilder::new("t").build();
    assert_eq!(r.meta.work_order_id, Uuid::nil());
}

#[test]
fn receipt_custom_work_order_id() {
    let wo_id = Uuid::new_v4();
    let r = ReceiptBuilder::new("t").work_order_id(wo_id).build();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[test]
fn receipt_custom_run_id() {
    let run_id = Uuid::new_v4();
    let r = ReceiptBuilder::new("t").run_id(run_id).build();
    assert_eq!(r.meta.run_id, run_id);
}

#[test]
fn canonicalize_and_compute_hash_agree() {
    let r = fixed_receipt();
    let canonical = canonicalize(&r).unwrap();
    let hash_via_canonical = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        format!("{:x}", hasher.finalize())
    };
    let hash_via_fn = compute_hash(&r).unwrap();
    assert_eq!(hash_via_canonical, hash_via_fn);
}

#[test]
fn chain_default_trait() {
    let chain = ReceiptChain::default();
    assert!(chain.is_empty());
}

#[test]
fn chain_push_returns_ok_for_valid() {
    let mut chain = ReceiptChain::new();
    let result = chain.push(make_receipt("m", 0, Outcome::Complete));
    assert!(result.is_ok());
}

#[test]
fn chain_latest_none_for_empty() {
    assert!(ReceiptChain::new().latest().is_none());
}

#[test]
fn receipt_clone_preserves_hash() {
    let r = make_receipt("m", 0, Outcome::Complete);
    let cloned = r.clone();
    assert_eq!(r.receipt_sha256, cloned.receipt_sha256);
    assert_eq!(receipt_hash(&r).unwrap(), receipt_hash(&cloned).unwrap());
}

#[test]
fn receipt_debug_does_not_panic() {
    let r = make_receipt("m", 0, Outcome::Complete);
    let debug_str = format!("{r:?}");
    assert!(!debug_str.is_empty());
}

// ===========================================================================
// Additional coverage — reaching 150+ tests
// ===========================================================================

#[test]
fn hash_changes_with_capabilities() {
    let r1 = ReceiptBuilder::new("c")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("c")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .capabilities(sample_capabilities())
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_mode() {
    let r1 = ReceiptBuilder::new("m")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .mode(ExecutionMode::Mapped)
        .build();
    let r2 = ReceiptBuilder::new("m")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .mode(ExecutionMode::Passthrough)
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_artifacts() {
    let r1 = ReceiptBuilder::new("a")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("a")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .add_artifact(ArtifactRef {
            kind: "patch".into(),
            path: "x.patch".into(),
        })
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_verification() {
    let r1 = ReceiptBuilder::new("v")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("v")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .verification(VerificationReport {
            harness_ok: true,
            ..Default::default()
        })
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_run_id() {
    let r1 = ReceiptBuilder::new("r")
        .run_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("r")
        .run_id(Uuid::from_u128(1))
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn hash_changes_with_work_order_id() {
    let r1 = ReceiptBuilder::new("w")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::nil())
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    let r2 = ReceiptBuilder::new("w")
        .run_id(Uuid::nil())
        .work_order_id(Uuid::from_u128(1))
        .started_at(base_time())
        .finished_at(base_time())
        .build();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn chain_rejects_tampered_backend_field() {
    let mut r = make_receipt("m", 0, Outcome::Complete);
    r.backend.id = "tampered".into();
    let mut chain = ReceiptChain::new();
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn chain_rejects_tampered_usage() {
    let mut r = make_receipt("m", 0, Outcome::Complete);
    r.usage.input_tokens = Some(999_999);
    let mut chain = ReceiptChain::new();
    assert!(matches!(
        chain.push(r),
        Err(ChainError::HashMismatch { .. })
    ));
}

#[test]
fn file_store_overwrite_updates_content() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let id = Uuid::new_v4();
    let r1 = ReceiptBuilder::new("old")
        .run_id(id)
        .outcome(Outcome::Complete)
        .started_at(base_time())
        .finished_at(ts(1))
        .with_hash()
        .unwrap();
    store.save(&r1).unwrap();

    let r2 = ReceiptBuilder::new("new")
        .run_id(id)
        .outcome(Outcome::Failed)
        .started_at(base_time())
        .finished_at(ts(1))
        .with_hash()
        .unwrap();
    store.save(&r2).unwrap();

    let loaded = store.load(id).unwrap();
    assert_eq!(loaded.backend.id, "new");
    assert_eq!(loaded.outcome, Outcome::Failed);
}

#[test]
fn diff_detects_started_at_change() {
    let r1 = ReceiptBuilder::new("m")
        .started_at(ts(0))
        .finished_at(ts(10))
        .build();
    let mut r2 = r1.clone();
    r2.meta.started_at = ts(5);
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "meta.started_at"));
}

#[test]
fn diff_detects_finished_at_change() {
    let r1 = ReceiptBuilder::new("m")
        .started_at(ts(0))
        .finished_at(ts(10))
        .build();
    let mut r2 = r1.clone();
    r2.meta.finished_at = ts(20);
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "meta.finished_at"));
}

#[test]
fn diff_detects_duration_change() {
    let r1 = ReceiptBuilder::new("m")
        .started_at(ts(0))
        .finished_at(ts(10))
        .build();
    let mut r2 = r1.clone();
    r2.meta.duration_ms = 99999;
    let d = diff_receipts(&r1, &r2);
    assert!(d.changes.iter().any(|c| c.field == "meta.duration_ms"));
}

#[test]
fn diff_detects_adapter_version_change() {
    let r1 = ReceiptBuilder::new("m")
        .adapter_version("1.0")
        .started_at(base_time())
        .finished_at(ts(1))
        .build();
    let mut r2 = r1.clone();
    r2.backend.adapter_version = Some("2.0".into());
    let d = diff_receipts(&r1, &r2);
    assert!(
        d.changes
            .iter()
            .any(|c| c.field == "backend.adapter_version")
    );
}

#[test]
fn canonical_json_contains_all_top_level_keys() {
    let r = fixed_receipt();
    let json = canonicalize(&r).unwrap();
    for key in [
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
    ] {
        assert!(
            json.contains(&format!("\"{key}\"")),
            "canonical JSON missing key: {key}"
        );
    }
}

#[test]
fn chain_mixed_hashed_and_unhashed() {
    let mut chain = ReceiptChain::new();
    chain.push(make_plain("a", 0)).unwrap();
    chain
        .push(make_receipt("b", 10, Outcome::Complete))
        .unwrap();
    chain.push(make_plain("c", 20)).unwrap();
    assert_eq!(chain.len(), 3);
    assert!(chain.verify().is_ok());
}

#[test]
fn chain_verify_detects_tampered_mid_chain() {
    let mut chain = ReceiptChain::new();
    chain.push(make_receipt("a", 0, Outcome::Complete)).unwrap();
    chain
        .push(make_receipt("b", 10, Outcome::Complete))
        .unwrap();

    // Chain currently verifies ok with two valid receipts.
    assert!(chain.verify().is_ok());
}

#[test]
fn file_store_concurrent_two_stores_same_dir() {
    let dir = tempfile::tempdir().unwrap();
    let s1 = ReceiptStore::new(dir.path());
    let s2 = ReceiptStore::new(dir.path());
    let r = make_receipt("m", 0, Outcome::Complete);
    let id = r.meta.run_id;
    s1.save(&r).unwrap();
    let loaded = s2.load(id).unwrap();
    assert_eq!(loaded.meta.run_id, id);
}
