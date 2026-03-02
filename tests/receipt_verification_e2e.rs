// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the receipt subsystem: builder, hashing, chains, diffs,
//! serialization round-trips, and edge cases.

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, Capability, ExecutionMode, SupportLevel,
    UsageNormalized, VerificationReport, CONTRACT_VERSION,
};
use abp_receipt::{
    canonicalize, compute_hash, diff_receipts, verify_hash, ChainError, Outcome, Receipt,
    ReceiptBuilder, ReceiptChain,
};
use chrono::{DateTime, TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fixed timestamp for deterministic tests.
fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

fn t1() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap()
}

fn t2() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 2).unwrap()
}

fn t3() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 3).unwrap()
}

fn fixed_uuid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// Build a minimal deterministic receipt (no hash).
fn minimal_receipt() -> Receipt {
    ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .run_id(fixed_uuid(1))
        .work_order_id(fixed_uuid(100))
        .started_at(t0())
        .finished_at(t1())
        .build()
}

/// Build a fully-populated deterministic receipt (no hash).
fn full_receipt() -> Receipt {
    let mut caps = BTreeMap::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);

    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(10),
        cache_write_tokens: Some(5),
        request_units: Some(1),
        estimated_cost_usd: Some(0.01),
    };

    let event = AgentEvent {
        ts: t0(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };

    let artifact = ArtifactRef {
        kind: "patch".into(),
        path: "output.patch".into(),
    };

    let verification = VerificationReport {
        git_diff: Some("diff --git a/f b/f".into()),
        git_status: Some("M f".into()),
        harness_ok: true,
    };

    ReceiptBuilder::new("sidecar:node")
        .outcome(Outcome::Complete)
        .run_id(fixed_uuid(2))
        .work_order_id(fixed_uuid(200))
        .started_at(t0())
        .finished_at(t1())
        .backend_version("1.0.0")
        .adapter_version("0.1.0")
        .capabilities(caps)
        .mode(ExecutionMode::Passthrough)
        .usage_raw(serde_json::json!({"prompt_tokens": 100, "completion_tokens": 50}))
        .usage(usage)
        .verification(verification)
        .add_trace_event(event)
        .add_artifact(artifact)
        .build()
}

// ===========================================================================
// 1. Receipt builder patterns
// ===========================================================================

mod builder_patterns {
    use super::*;

    #[test]
    fn minimal_builder() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.backend.id, "mock");
        assert_eq!(r.outcome, Outcome::Complete);
        assert!(r.receipt_sha256.is_none());
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn builder_sets_outcome() {
        let r = ReceiptBuilder::new("mock").outcome(Outcome::Failed).build();
        assert_eq!(r.outcome, Outcome::Failed);
    }

    #[test]
    fn builder_sets_partial_outcome() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Partial)
            .build();
        assert_eq!(r.outcome, Outcome::Partial);
    }

    #[test]
    fn builder_sets_backend_version() {
        let r = ReceiptBuilder::new("mock")
            .backend_version("2.0.0")
            .build();
        assert_eq!(r.backend.backend_version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn builder_sets_adapter_version() {
        let r = ReceiptBuilder::new("mock")
            .adapter_version("0.5.0")
            .build();
        assert_eq!(r.backend.adapter_version.as_deref(), Some("0.5.0"));
    }

    #[test]
    fn builder_sets_work_order_id() {
        let id = fixed_uuid(42);
        let r = ReceiptBuilder::new("mock").work_order_id(id).build();
        assert_eq!(r.meta.work_order_id, id);
    }

    #[test]
    fn builder_sets_run_id() {
        let id = fixed_uuid(99);
        let r = ReceiptBuilder::new("mock").run_id(id).build();
        assert_eq!(r.meta.run_id, id);
    }

    #[test]
    fn builder_sets_timestamps() {
        let r = ReceiptBuilder::new("mock")
            .started_at(t0())
            .finished_at(t1())
            .build();
        assert_eq!(r.meta.started_at, t0());
        assert_eq!(r.meta.finished_at, t1());
    }

    #[test]
    fn builder_computes_duration_ms() {
        let r = ReceiptBuilder::new("mock")
            .started_at(t0())
            .finished_at(t1())
            .build();
        assert_eq!(r.meta.duration_ms, 1000);
    }

    #[test]
    fn builder_duration_zero_when_same_timestamps() {
        let r = ReceiptBuilder::new("mock")
            .started_at(t0())
            .finished_at(t0())
            .build();
        assert_eq!(r.meta.duration_ms, 0);
    }

    #[test]
    fn builder_duration_clamps_negative() {
        let r = ReceiptBuilder::new("mock")
            .started_at(t1())
            .finished_at(t0())
            .build();
        assert_eq!(r.meta.duration_ms, 0);
    }

    #[test]
    fn builder_sets_capabilities() {
        let mut caps = BTreeMap::new();
        caps.insert(Capability::ToolRead, SupportLevel::Native);
        let r = ReceiptBuilder::new("mock").capabilities(caps).build();
        assert!(r.capabilities.contains_key(&Capability::ToolRead));
    }

    #[test]
    fn builder_sets_execution_mode_passthrough() {
        let r = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Passthrough)
            .build();
        assert_eq!(r.mode, ExecutionMode::Passthrough);
    }

    #[test]
    fn builder_default_mode_is_mapped() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn builder_sets_usage_raw() {
        let raw = serde_json::json!({"tokens": 42});
        let r = ReceiptBuilder::new("mock").usage_raw(raw.clone()).build();
        assert_eq!(r.usage_raw, raw);
    }

    #[test]
    fn builder_sets_usage_normalized() {
        let usage = UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(200),
            ..Default::default()
        };
        let r = ReceiptBuilder::new("mock").usage(usage).build();
        assert_eq!(r.usage.input_tokens, Some(500));
        assert_eq!(r.usage.output_tokens, Some(200));
    }

    #[test]
    fn builder_adds_trace_event() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::AssistantMessage {
                text: "hi".into(),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock").add_trace_event(event).build();
        assert_eq!(r.trace.len(), 1);
    }

    #[test]
    fn builder_adds_multiple_trace_events() {
        let e1 = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::RunStarted {
                message: "start".into(),
            },
            ext: None,
        };
        let e2 = AgentEvent {
            ts: t1(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .add_trace_event(e1)
            .add_trace_event(e2)
            .build();
        assert_eq!(r.trace.len(), 2);
    }

    #[test]
    fn builder_adds_artifact() {
        let a = ArtifactRef {
            kind: "log".into(),
            path: "run.log".into(),
        };
        let r = ReceiptBuilder::new("mock").add_artifact(a).build();
        assert_eq!(r.artifacts.len(), 1);
        assert_eq!(r.artifacts[0].kind, "log");
    }

    #[test]
    fn builder_adds_multiple_artifacts() {
        let a1 = ArtifactRef {
            kind: "patch".into(),
            path: "a.patch".into(),
        };
        let a2 = ArtifactRef {
            kind: "log".into(),
            path: "b.log".into(),
        };
        let r = ReceiptBuilder::new("mock")
            .add_artifact(a1)
            .add_artifact(a2)
            .build();
        assert_eq!(r.artifacts.len(), 2);
    }

    #[test]
    fn builder_sets_verification() {
        let v = VerificationReport {
            git_diff: Some("diff".into()),
            git_status: Some("M file".into()),
            harness_ok: true,
        };
        let r = ReceiptBuilder::new("mock").verification(v).build();
        assert!(r.verification.harness_ok);
        assert_eq!(r.verification.git_diff.as_deref(), Some("diff"));
    }

    #[test]
    fn builder_with_hash_attaches_sha256() {
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .with_hash()
            .unwrap();
        assert!(r.receipt_sha256.is_some());
        assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
    }

    #[test]
    fn builder_with_hash_verifies() {
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn full_builder_pattern() {
        let r = full_receipt();
        assert_eq!(r.backend.id, "sidecar:node");
        assert_eq!(r.backend.backend_version.as_deref(), Some("1.0.0"));
        assert_eq!(r.backend.adapter_version.as_deref(), Some("0.1.0"));
        assert_eq!(r.outcome, Outcome::Complete);
        assert_eq!(r.mode, ExecutionMode::Passthrough);
        assert_eq!(r.trace.len(), 1);
        assert_eq!(r.artifacts.len(), 1);
        assert!(r.verification.harness_ok);
        assert_eq!(r.usage.input_tokens, Some(100));
    }

    #[test]
    fn builder_overrides_backend_id() {
        let r = ReceiptBuilder::new("first")
            .backend_id("second")
            .build();
        assert_eq!(r.backend.id, "second");
    }

    #[test]
    fn builder_contract_version_matches_constant() {
        let r = ReceiptBuilder::new("mock").build();
        assert_eq!(r.meta.contract_version, "abp/v0.1");
    }
}

// ===========================================================================
// 2. Canonical hashing: determinism
// ===========================================================================

mod canonical_hashing {
    use super::*;

    #[test]
    fn same_receipt_same_hash() {
        let r = minimal_receipt();
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_is_64_hex_chars() {
        let r = minimal_receipt();
        let h = compute_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn canonicalize_deterministic() {
        let r = minimal_receipt();
        let j1 = canonicalize(&r).unwrap();
        let j2 = canonicalize(&r).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn canonicalize_forces_receipt_sha256_null() {
        let mut r = minimal_receipt();
        r.receipt_sha256 = Some("should_be_removed".into());
        let json = canonicalize(&r).unwrap();
        assert!(json.contains("\"receipt_sha256\":null"));
        assert!(!json.contains("should_be_removed"));
    }

    #[test]
    fn hash_ignores_existing_receipt_sha256() {
        let r = minimal_receipt();
        let h1 = compute_hash(&r).unwrap();

        let mut r2 = minimal_receipt();
        r2.receipt_sha256 = Some("anything".into());
        let h2 = compute_hash(&r2).unwrap();

        assert_eq!(h1, h2);
    }

    #[test]
    fn with_hash_produces_verifiable_receipt() {
        let r = minimal_receipt().with_hash().unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn verify_hash_accepts_none() {
        let r = minimal_receipt();
        assert!(r.receipt_sha256.is_none());
        assert!(verify_hash(&r));
    }

    #[test]
    fn verify_hash_rejects_tampered() {
        let mut r = minimal_receipt().with_hash().unwrap();
        r.receipt_sha256 = Some("0000000000000000000000000000000000000000000000000000000000000000".into());
        assert!(!verify_hash(&r));
    }

    #[test]
    fn verify_hash_rejects_garbage() {
        let mut r = minimal_receipt();
        r.receipt_sha256 = Some("not_a_real_hash".into());
        assert!(!verify_hash(&r));
    }

    #[test]
    fn full_receipt_hashing_deterministic() {
        let r = full_receipt();
        let h1 = compute_hash(&r).unwrap();
        let h2 = compute_hash(&r).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn full_receipt_with_hash_verifies() {
        let r = full_receipt().with_hash().unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn double_with_hash_still_verifies() {
        let r = minimal_receipt().with_hash().unwrap().with_hash().unwrap();
        assert!(verify_hash(&r));
    }
}

// ===========================================================================
// 3. Hash stability: changing any field changes the hash
// ===========================================================================

mod hash_stability {
    use super::*;

    fn base_hash() -> String {
        compute_hash(&minimal_receipt()).unwrap()
    }

    #[test]
    fn changing_backend_id_changes_hash() {
        let mut r = minimal_receipt();
        r.backend.id = "other".into();
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_outcome_changes_hash() {
        let mut r = minimal_receipt();
        r.outcome = Outcome::Failed;
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_outcome_to_partial_changes_hash() {
        let mut r = minimal_receipt();
        r.outcome = Outcome::Partial;
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_work_order_id_changes_hash() {
        let mut r = minimal_receipt();
        r.meta.work_order_id = fixed_uuid(999);
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_run_id_changes_hash() {
        let mut r = minimal_receipt();
        r.meta.run_id = fixed_uuid(999);
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_started_at_changes_hash() {
        let mut r = minimal_receipt();
        r.meta.started_at = t2();
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_finished_at_changes_hash() {
        let mut r = minimal_receipt();
        r.meta.finished_at = t2();
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_duration_ms_changes_hash() {
        let mut r = minimal_receipt();
        r.meta.duration_ms = 99999;
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_contract_version_changes_hash() {
        let mut r = minimal_receipt();
        r.meta.contract_version = "abp/v999".into();
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_backend_version_changes_hash() {
        let mut r = minimal_receipt();
        r.backend.backend_version = Some("9.9.9".into());
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_adapter_version_changes_hash() {
        let mut r = minimal_receipt();
        r.backend.adapter_version = Some("9.9.9".into());
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_mode_changes_hash() {
        let mut r = minimal_receipt();
        r.mode = ExecutionMode::Passthrough;
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn adding_capability_changes_hash() {
        let mut r = minimal_receipt();
        r.capabilities
            .insert(Capability::ToolRead, SupportLevel::Native);
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_usage_raw_changes_hash() {
        let mut r = minimal_receipt();
        r.usage_raw = serde_json::json!({"tokens": 1});
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_usage_input_tokens_changes_hash() {
        let mut r = minimal_receipt();
        r.usage.input_tokens = Some(42);
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_usage_output_tokens_changes_hash() {
        let mut r = minimal_receipt();
        r.usage.output_tokens = Some(42);
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn adding_trace_event_changes_hash() {
        let mut r = minimal_receipt();
        r.trace.push(AgentEvent {
            ts: t0(),
            kind: AgentEventKind::AssistantMessage {
                text: "hi".into(),
            },
            ext: None,
        });
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn adding_artifact_changes_hash() {
        let mut r = minimal_receipt();
        r.artifacts.push(ArtifactRef {
            kind: "log".into(),
            path: "x.log".into(),
        });
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_verification_harness_ok_changes_hash() {
        let mut r = minimal_receipt();
        r.verification.harness_ok = true;
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_verification_git_diff_changes_hash() {
        let mut r = minimal_receipt();
        r.verification.git_diff = Some("diff".into());
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn changing_verification_git_status_changes_hash() {
        let mut r = minimal_receipt();
        r.verification.git_status = Some("M f".into());
        assert_ne!(compute_hash(&r).unwrap(), base_hash());
    }

    #[test]
    fn receipt_sha256_field_does_not_change_hash() {
        let r1 = minimal_receipt();
        let mut r2 = minimal_receipt();
        r2.receipt_sha256 = Some("anything".into());
        assert_eq!(compute_hash(&r1).unwrap(), compute_hash(&r2).unwrap());
    }
}

// ===========================================================================
// 4. Chain verification
// ===========================================================================

mod chain_verification {
    use super::*;

    fn make_hashed(run: u128, wo: u128, start: DateTime<Utc>, end: DateTime<Utc>) -> Receipt {
        ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(run))
            .work_order_id(fixed_uuid(wo))
            .started_at(start)
            .finished_at(end)
            .with_hash()
            .unwrap()
    }

    #[test]
    fn empty_chain_verify_fails() {
        let chain = ReceiptChain::new();
        assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
    }

    #[test]
    fn empty_chain_is_empty() {
        let chain = ReceiptChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn single_receipt_chain() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t0(), t1())).unwrap();
        assert_eq!(chain.len(), 1);
        assert!(!chain.is_empty());
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn chain_latest_returns_last() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t0(), t1())).unwrap();
        chain.push(make_hashed(2, 101, t1(), t2())).unwrap();
        assert_eq!(chain.latest().unwrap().meta.run_id, fixed_uuid(2));
    }

    #[test]
    fn chain_three_receipts_in_order() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t0(), t1())).unwrap();
        chain.push(make_hashed(2, 101, t1(), t2())).unwrap();
        chain.push(make_hashed(3, 102, t2(), t3())).unwrap();
        assert_eq!(chain.len(), 3);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn chain_rejects_duplicate_run_id() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t0(), t1())).unwrap();
        let err = chain.push(make_hashed(1, 101, t1(), t2())).unwrap_err();
        assert_eq!(err, ChainError::DuplicateId { id: fixed_uuid(1) });
    }

    #[test]
    fn chain_rejects_out_of_order() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t1(), t2())).unwrap();
        let err = chain.push(make_hashed(2, 101, t0(), t1())).unwrap_err();
        assert!(matches!(err, ChainError::BrokenLink { index: 1 }));
    }

    #[test]
    fn chain_rejects_tampered_hash() {
        let mut chain = ReceiptChain::new();
        let mut r = make_hashed(1, 100, t0(), t1());
        r.receipt_sha256 = Some("bad".into());
        let err = chain.push(r).unwrap_err();
        assert!(matches!(err, ChainError::HashMismatch { index: 0 }));
    }

    #[test]
    fn chain_accepts_receipt_without_hash() {
        let mut chain = ReceiptChain::new();
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build();
        assert!(r.receipt_sha256.is_none());
        chain.push(r).unwrap();
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn chain_iter_yields_all() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t0(), t1())).unwrap();
        chain.push(make_hashed(2, 101, t1(), t2())).unwrap();
        let ids: Vec<Uuid> = chain.iter().map(|r| r.meta.run_id).collect();
        assert_eq!(ids, vec![fixed_uuid(1), fixed_uuid(2)]);
    }

    #[test]
    fn chain_into_iter() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t0(), t1())).unwrap();
        let mut count = 0;
        for _ in &chain {
            count += 1;
        }
        assert_eq!(count, 1);
    }

    #[test]
    fn chain_same_start_time_accepted() {
        let mut chain = ReceiptChain::new();
        chain.push(make_hashed(1, 100, t0(), t1())).unwrap();
        // Same start time is not "before" the previous, so it should be accepted.
        chain.push(make_hashed(2, 101, t0(), t1())).unwrap();
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn chain_error_display_hash_mismatch() {
        let err = ChainError::HashMismatch { index: 3 };
        assert_eq!(err.to_string(), "hash mismatch at chain index 3");
    }

    #[test]
    fn chain_error_display_broken_link() {
        let err = ChainError::BrokenLink { index: 5 };
        assert_eq!(err.to_string(), "broken link at chain index 5");
    }

    #[test]
    fn chain_error_display_empty() {
        assert_eq!(ChainError::EmptyChain.to_string(), "chain is empty");
    }

    #[test]
    fn chain_error_display_duplicate_id() {
        let id = fixed_uuid(42);
        let err = ChainError::DuplicateId { id };
        assert!(err.to_string().contains("duplicate receipt id"));
    }

    #[test]
    fn chain_latest_none_when_empty() {
        let chain = ReceiptChain::new();
        assert!(chain.latest().is_none());
    }
}

// ===========================================================================
// 5. Receipt diffing
// ===========================================================================

mod receipt_diffing {
    use super::*;

    #[test]
    fn identical_receipts_no_diff() {
        let a = minimal_receipt();
        let b = minimal_receipt();
        let diff = diff_receipts(&a, &b);
        assert!(diff.is_empty());
        assert_eq!(diff.len(), 0);
    }

    #[test]
    fn different_backend_id_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.backend.id = "other".into();
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "backend.id"));
    }

    #[test]
    fn different_outcome_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.outcome = Outcome::Failed;
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "outcome"));
    }

    #[test]
    fn different_run_id_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.meta.run_id = fixed_uuid(999);
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "meta.run_id"));
    }

    #[test]
    fn different_work_order_id_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.meta.work_order_id = fixed_uuid(999);
        let diff = diff_receipts(&a, &b);
        assert!(
            diff.changes
                .iter()
                .any(|d| d.field == "meta.work_order_id")
        );
    }

    #[test]
    fn different_contract_version_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.meta.contract_version = "abp/v99".into();
        let diff = diff_receipts(&a, &b);
        assert!(
            diff.changes
                .iter()
                .any(|d| d.field == "meta.contract_version")
        );
    }

    #[test]
    fn different_started_at_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.meta.started_at = t2();
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "meta.started_at"));
    }

    #[test]
    fn different_finished_at_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.meta.finished_at = t2();
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "meta.finished_at"));
    }

    #[test]
    fn different_duration_ms_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.meta.duration_ms = 9999;
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "meta.duration_ms"));
    }

    #[test]
    fn different_backend_version_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.backend.backend_version = Some("new".into());
        let diff = diff_receipts(&a, &b);
        assert!(
            diff.changes
                .iter()
                .any(|d| d.field == "backend.backend_version")
        );
    }

    #[test]
    fn different_adapter_version_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.backend.adapter_version = Some("new".into());
        let diff = diff_receipts(&a, &b);
        assert!(
            diff.changes
                .iter()
                .any(|d| d.field == "backend.adapter_version")
        );
    }

    #[test]
    fn different_mode_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.mode = ExecutionMode::Passthrough;
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "mode"));
    }

    #[test]
    fn different_usage_raw_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.usage_raw = serde_json::json!({"x": 1});
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "usage_raw"));
    }

    #[test]
    fn different_usage_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.usage.input_tokens = Some(42);
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "usage"));
    }

    #[test]
    fn different_trace_len_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.trace.push(AgentEvent {
            ts: t0(),
            kind: AgentEventKind::AssistantMessage {
                text: "x".into(),
            },
            ext: None,
        });
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "trace.len"));
    }

    #[test]
    fn different_artifacts_len_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.artifacts.push(ArtifactRef {
            kind: "log".into(),
            path: "x".into(),
        });
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "artifacts.len"));
    }

    #[test]
    fn different_verification_detected() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.verification.harness_ok = true;
        let diff = diff_receipts(&a, &b);
        assert!(diff.changes.iter().any(|d| d.field == "verification"));
    }

    #[test]
    fn diff_field_diff_has_old_and_new() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.backend.id = "changed".into();
        let diff = diff_receipts(&a, &b);
        let fd = diff
            .changes
            .iter()
            .find(|d| d.field == "backend.id")
            .unwrap();
        assert_eq!(fd.old, "mock");
        assert_eq!(fd.new, "changed");
    }

    #[test]
    fn diff_multiple_changes() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.backend.id = "new_backend".into();
        b.outcome = Outcome::Failed;
        let diff = diff_receipts(&a, &b);
        assert!(diff.len() >= 2);
    }

    #[test]
    fn diff_full_vs_minimal() {
        let a = minimal_receipt();
        let b = full_receipt();
        let diff = diff_receipts(&a, &b);
        assert!(!diff.is_empty());
    }
}

// ===========================================================================
// 6. Serialization round-trip
// ===========================================================================

mod serialization_roundtrip {
    use super::*;

    #[test]
    fn minimal_receipt_roundtrip() {
        let r = minimal_receipt();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r.meta.run_id, r2.meta.run_id);
        assert_eq!(r.outcome, r2.outcome);
        assert_eq!(r.backend.id, r2.backend.id);
    }

    #[test]
    fn full_receipt_roundtrip() {
        let r = full_receipt();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r.backend.id, r2.backend.id);
        assert_eq!(r.outcome, r2.outcome);
        assert_eq!(r.mode, r2.mode);
        assert_eq!(r.trace.len(), r2.trace.len());
        assert_eq!(r.artifacts.len(), r2.artifacts.len());
    }

    #[test]
    fn hashed_receipt_roundtrip() {
        let r = minimal_receipt().with_hash().unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r.receipt_sha256, r2.receipt_sha256);
        assert!(verify_hash(&r2));
    }

    #[test]
    fn roundtrip_preserves_hash_verification() {
        let r = full_receipt().with_hash().unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert!(verify_hash(&r2));
    }

    #[test]
    fn canonical_json_roundtrip() {
        let r = minimal_receipt();
        let canonical = canonicalize(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&canonical).unwrap();
        let canonical2 = canonicalize(&r2).unwrap();
        assert_eq!(canonical, canonical2);
    }

    #[test]
    fn pretty_and_compact_produce_same_hash() {
        let r = minimal_receipt();
        let compact = serde_json::to_string(&r).unwrap();
        let pretty = serde_json::to_string_pretty(&r).unwrap();
        let r_compact: Receipt = serde_json::from_str(&compact).unwrap();
        let r_pretty: Receipt = serde_json::from_str(&pretty).unwrap();
        assert_eq!(
            compute_hash(&r_compact).unwrap(),
            compute_hash(&r_pretty).unwrap()
        );
    }

    #[test]
    fn roundtrip_preserves_all_usage_fields() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.01),
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .usage(usage)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.usage.input_tokens, Some(100));
        assert_eq!(r2.usage.output_tokens, Some(50));
        assert_eq!(r2.usage.cache_read_tokens, Some(10));
        assert_eq!(r2.usage.cache_write_tokens, Some(5));
        assert_eq!(r2.usage.request_units, Some(1));
        assert_eq!(r2.usage.estimated_cost_usd, Some(0.01));
    }

    #[test]
    fn roundtrip_preserves_capabilities() {
        let mut caps = BTreeMap::new();
        caps.insert(Capability::ToolRead, SupportLevel::Native);
        caps.insert(Capability::Streaming, SupportLevel::Emulated);
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .capabilities(caps)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert!(r2.capabilities.contains_key(&Capability::ToolRead));
        assert!(r2.capabilities.contains_key(&Capability::Streaming));
    }

    #[test]
    fn roundtrip_preserves_verification_report() {
        let v = VerificationReport {
            git_diff: Some("diff --git".into()),
            git_status: Some("M src/lib.rs".into()),
            harness_ok: true,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .verification(v)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert!(r2.verification.harness_ok);
        assert_eq!(r2.verification.git_diff.as_deref(), Some("diff --git"));
    }

    #[test]
    fn roundtrip_preserves_artifacts() {
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_artifact(ArtifactRef {
                kind: "patch".into(),
                path: "out.patch".into(),
            })
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.artifacts.len(), 1);
        assert_eq!(r2.artifacts[0].kind, "patch");
        assert_eq!(r2.artifacts[0].path, "out.patch");
    }

    #[test]
    fn roundtrip_preserves_trace_events() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("tc1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"path": "file.rs"}),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.trace.len(), 1);
    }

    #[test]
    fn outcome_serializes_as_snake_case() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build();
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"complete\""));
    }

    #[test]
    fn execution_mode_serializes_as_snake_case() {
        let r = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Passthrough)
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build();
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"passthrough\""));
    }
}

// ===========================================================================
// 7. Edge cases
// ===========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_backend_id() {
        let r = ReceiptBuilder::new("").build();
        assert_eq!(r.backend.id, "");
        let h = compute_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn unicode_backend_id() {
        let r = ReceiptBuilder::new("バックエンド")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build();
        let h = compute_hash(&r).unwrap();
        assert_eq!(h.len(), 64);
        let r2 = r.with_hash().unwrap();
        assert!(verify_hash(&r2));
    }

    #[test]
    fn emoji_in_backend_id() {
        let r = ReceiptBuilder::new("🚀backend🔥")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn unicode_in_trace_message() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::AssistantMessage {
                text: "こんにちは世界 🌍".into(),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn unicode_in_verification_diff() {
        let v = VerificationReport {
            git_diff: Some("改行\n追加".into()),
            git_status: None,
            harness_ok: false,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .verification(v)
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn empty_trace_and_artifacts() {
        let r = minimal_receipt();
        assert!(r.trace.is_empty());
        assert!(r.artifacts.is_empty());
        assert!(r.with_hash().is_ok());
    }

    #[test]
    fn many_trace_events() {
        let mut builder = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1());
        for i in 0..100 {
            builder = builder.add_trace_event(AgentEvent {
                ts: t0(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token_{i}"),
                },
                ext: None,
            });
        }
        let r = builder.build().with_hash().unwrap();
        assert_eq!(r.trace.len(), 100);
        assert!(verify_hash(&r));
    }

    #[test]
    fn many_artifacts() {
        let mut builder = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1());
        for i in 0..50 {
            builder = builder.add_artifact(ArtifactRef {
                kind: "file".into(),
                path: format!("artifact_{i}.txt"),
            });
        }
        let r = builder.build().with_hash().unwrap();
        assert_eq!(r.artifacts.len(), 50);
        assert!(verify_hash(&r));
    }

    #[test]
    fn large_usage_raw_payload() {
        let mut obj = serde_json::Map::new();
        for i in 0..100 {
            obj.insert(format!("field_{i}"), serde_json::json!(i));
        }
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .usage_raw(serde_json::Value::Object(obj))
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn nil_uuid_work_order() {
        let r = ReceiptBuilder::new("mock")
            .work_order_id(Uuid::nil())
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
        assert_eq!(r.meta.work_order_id, Uuid::nil());
    }

    #[test]
    fn max_uuid_values() {
        let r = ReceiptBuilder::new("mock")
            .run_id(Uuid::from_u128(u128::MAX))
            .work_order_id(Uuid::from_u128(u128::MAX))
            .started_at(t0())
            .finished_at(t1())
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn very_long_backend_id() {
        let long_id = "a".repeat(10_000);
        let r = ReceiptBuilder::new(&long_id)
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
        assert_eq!(r.backend.id.len(), 10_000);
    }

    #[test]
    fn special_chars_in_strings() {
        let r = ReceiptBuilder::new("back\"end\\id\nnewline\ttab")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn null_bytes_in_backend_id() {
        let r = ReceiptBuilder::new("null\0byte")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn all_usage_fields_none() {
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .usage(UsageNormalized::default())
            .build();
        assert!(r.usage.input_tokens.is_none());
        assert!(r.usage.output_tokens.is_none());
        assert_eq!(compute_hash(&r).unwrap().len(), 64);
    }

    #[test]
    fn all_outcome_variants_hash_differently() {
        let outcomes = [Outcome::Complete, Outcome::Partial, Outcome::Failed];
        let hashes: Vec<String> = outcomes
            .iter()
            .map(|o| {
                let r = ReceiptBuilder::new("mock")
                    .run_id(fixed_uuid(1))
                    .work_order_id(fixed_uuid(100))
                    .started_at(t0())
                    .finished_at(t1())
                    .outcome(o.clone())
                    .build();
                compute_hash(&r).unwrap()
            })
            .collect();
        assert_ne!(hashes[0], hashes[1]);
        assert_ne!(hashes[0], hashes[2]);
        assert_ne!(hashes[1], hashes[2]);
    }

    #[test]
    fn trace_event_with_ext_data() {
        let mut ext = BTreeMap::new();
        ext.insert(
            "raw_message".to_string(),
            serde_json::json!({"vendor": "data"}),
        );
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::AssistantMessage {
                text: "hi".into(),
            },
            ext: Some(ext),
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build()
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));
    }

    #[test]
    fn clone_preserves_hash_verification() {
        let r = minimal_receipt().with_hash().unwrap();
        let r2 = r.clone();
        assert!(verify_hash(&r2));
        assert_eq!(r.receipt_sha256, r2.receipt_sha256);
    }

    #[test]
    fn chain_with_many_receipts() {
        let mut chain = ReceiptChain::new();
        for i in 0u128..20 {
            let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, i as u32).unwrap();
            let end = Utc
                .with_ymd_and_hms(2025, 1, 1, 0, 0, i as u32 + 1)
                .unwrap();
            let r = ReceiptBuilder::new("mock")
                .run_id(fixed_uuid(i + 1))
                .work_order_id(fixed_uuid(i + 100))
                .started_at(start)
                .finished_at(end)
                .with_hash()
                .unwrap();
            chain.push(r).unwrap();
        }
        assert_eq!(chain.len(), 20);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn diff_cloned_receipt_is_empty() {
        let r = full_receipt();
        let r2 = r.clone();
        let diff = diff_receipts(&r, &r2);
        assert!(diff.is_empty());
    }

    #[test]
    fn diff_symmetry_count() {
        let a = minimal_receipt();
        let mut b = minimal_receipt();
        b.backend.id = "other".into();
        let d1 = diff_receipts(&a, &b);
        let d2 = diff_receipts(&b, &a);
        assert_eq!(d1.len(), d2.len());
    }

    #[test]
    fn tool_call_event_roundtrip() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("id-1".into()),
                parent_tool_use_id: Some("parent-1".into()),
                input: serde_json::json!({"command": "ls -la"}),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.trace.len(), 1);
        assert!(verify_hash(&r2.with_hash().unwrap()));
    }

    #[test]
    fn tool_result_event_roundtrip() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("id-2".into()),
                output: serde_json::json!({"content": "file contents"}),
                is_error: false,
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.trace.len(), 1);
    }

    #[test]
    fn file_changed_event_roundtrip() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "added function".into(),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.trace.len(), 1);
    }

    #[test]
    fn command_executed_event_roundtrip() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::CommandExecuted {
                command: "cargo test".into(),
                exit_code: Some(0),
                output_preview: Some("ok".into()),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.trace.len(), 1);
    }

    #[test]
    fn warning_event_roundtrip() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::Warning {
                message: "disk space low".into(),
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.trace.len(), 1);
    }

    #[test]
    fn error_event_roundtrip() {
        let event = AgentEvent {
            ts: t0(),
            kind: AgentEventKind::Error {
                message: "crash".into(),
                error_code: None,
            },
            ext: None,
        };
        let r = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1())
            .add_trace_event(event)
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(r2.trace.len(), 1);
    }

    #[test]
    fn mixed_event_types_in_trace() {
        let events = vec![
            AgentEvent {
                ts: t0(),
                kind: AgentEventKind::RunStarted {
                    message: "go".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: t0(),
                kind: AgentEventKind::AssistantDelta {
                    text: "tok".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: t0(),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read".into(),
                    tool_use_id: None,
                    parent_tool_use_id: None,
                    input: serde_json::json!({}),
                },
                ext: None,
            },
            AgentEvent {
                ts: t0(),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ];
        let mut builder = ReceiptBuilder::new("mock")
            .run_id(fixed_uuid(1))
            .started_at(t0())
            .finished_at(t1());
        for e in events {
            builder = builder.add_trace_event(e);
        }
        let r = builder.build().with_hash().unwrap();
        assert_eq!(r.trace.len(), 4);
        assert!(verify_hash(&r));
    }
}
