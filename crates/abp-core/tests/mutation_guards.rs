// SPDX-License-Identifier: MIT OR Apache-2.0

//! Mutation guard tests.
//!
//! These tests are specifically designed to catch common mutations that tools
//! like `cargo-mutants` would introduce: flipped comparisons, replaced return
//! values, deleted statements, and swapped arguments.
//!
//! They complement the existing property-based and snapshot tests by focusing on
//! boundary conditions and exact return values.

use abp_core::*;
use abp_core::validate::{ValidationError, validate_receipt};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
}

fn minimal_receipt() -> Receipt {
    let ts = fixed_ts();
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

// ── 1. Receipt hashing: null-before-hash boundary ───────────────────

mod receipt_hash_null_field {
    use super::*;

    /// The critical invariant: `receipt_sha256` is forced to `null` before
    /// hashing, so any value in that field must NOT change the digest.
    #[test]
    fn hash_ignores_receipt_sha256_value() {
        let h_none = receipt_hash(&minimal_receipt()).unwrap();

        let mut r = minimal_receipt();
        r.receipt_sha256 = Some("deadbeef".repeat(8));
        let h_some = receipt_hash(&r).unwrap();

        assert_eq!(h_none, h_some);
    }

    /// After `with_hash()`, the stored hash must equal a fresh recomputation.
    #[test]
    fn with_hash_stores_correct_value() {
        let r = minimal_receipt().with_hash().unwrap();
        let stored = r.receipt_sha256.as_ref().expect("hash must be Some");
        let fresh = receipt_hash(&r).unwrap();
        assert_eq!(stored, &fresh);
    }

    /// `with_hash` must actually set the field to `Some`, not leave it `None`.
    #[test]
    fn with_hash_sets_some() {
        let r = minimal_receipt().with_hash().unwrap();
        assert!(
            r.receipt_sha256.is_some(),
            "with_hash must populate receipt_sha256"
        );
    }

    /// The hash output must be exactly 64 lowercase hex characters.
    #[test]
    fn hash_format_exact() {
        let h = receipt_hash(&minimal_receipt()).unwrap();
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h, h.to_ascii_lowercase());
    }
}

// ── 2. SupportLevel::satisfies boundary conditions ──────────────────

mod satisfies_boundaries {
    use super::*;

    // Truth table — every (MinSupport, SupportLevel) pair.
    // A mutant that flips any single boolean will break at least one test.

    #[test]
    fn native_native_true() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
    }
    #[test]
    fn emulated_native_false() {
        assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
    }
    #[test]
    fn restricted_native_false() {
        assert!(!SupportLevel::Restricted { reason: "x".into() }.satisfies(&MinSupport::Native));
    }
    #[test]
    fn unsupported_native_false() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Native));
    }

    #[test]
    fn native_emulated_true() {
        assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
    }
    #[test]
    fn emulated_emulated_true() {
        assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
    }
    #[test]
    fn restricted_emulated_true() {
        assert!(SupportLevel::Restricted { reason: "x".into() }.satisfies(&MinSupport::Emulated));
    }
    #[test]
    fn unsupported_emulated_false() {
        assert!(!SupportLevel::Unsupported.satisfies(&MinSupport::Emulated));
    }
}

// ── 3. WorkOrder validation / builder edge cases ────────────────────

mod work_order_edges {
    use super::*;

    /// Default lane from the builder must be `PatchFirst`, not `WorkspaceFirst`.
    #[test]
    fn builder_default_lane_is_patch_first() {
        let wo = WorkOrderBuilder::new("test").build();
        assert!(
            matches!(wo.lane, ExecutionLane::PatchFirst),
            "default lane must be PatchFirst"
        );
    }

    /// Default workspace mode must be `Staged`.
    #[test]
    fn builder_default_workspace_mode_is_staged() {
        let wo = WorkOrderBuilder::new("test").build();
        assert!(
            matches!(wo.workspace.mode, WorkspaceMode::Staged),
            "default workspace mode must be Staged"
        );
    }

    /// Default root must be ".".
    #[test]
    fn builder_default_root_is_dot() {
        let wo = WorkOrderBuilder::new("test").build();
        assert_eq!(wo.workspace.root, ".");
    }

    /// Task text must be preserved exactly.
    #[test]
    fn builder_preserves_task_text() {
        let wo = WorkOrderBuilder::new("Fix the login bug").build();
        assert_eq!(wo.task, "Fix the login bug");
    }

    /// Empty task string is allowed (no panic, no substitution).
    #[test]
    fn builder_empty_task() {
        let wo = WorkOrderBuilder::new("").build();
        assert_eq!(wo.task, "");
    }

    /// `.model()` setter stores the value in `config.model`.
    #[test]
    fn builder_model_setter() {
        let wo = WorkOrderBuilder::new("t").model("gpt-4").build();
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4"));
    }

    /// `.max_turns()` setter stores the value in `config.max_turns`.
    #[test]
    fn builder_max_turns_setter() {
        let wo = WorkOrderBuilder::new("t").max_turns(10).build();
        assert_eq!(wo.config.max_turns, Some(10));
    }

    /// `.max_budget_usd()` setter stores the value in `config.max_budget_usd`.
    #[test]
    fn builder_max_budget_setter() {
        let wo = WorkOrderBuilder::new("t").max_budget_usd(5.0).build();
        assert_eq!(wo.config.max_budget_usd, Some(5.0));
    }

    /// Default config values: no model, no budget, no turns.
    #[test]
    fn builder_default_config_is_none() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.config.model.is_none());
        assert!(wo.config.max_budget_usd.is_none());
        assert!(wo.config.max_turns.is_none());
    }

    /// Include/exclude lists default to empty.
    #[test]
    fn builder_default_globs_empty() {
        let wo = WorkOrderBuilder::new("t").build();
        assert!(wo.workspace.include.is_empty());
        assert!(wo.workspace.exclude.is_empty());
    }

    /// `.include()` and `.exclude()` setters are preserved.
    #[test]
    fn builder_include_exclude_preserved() {
        let wo = WorkOrderBuilder::new("t")
            .include(vec!["src/**".into()])
            .exclude(vec!["*.log".into()])
            .build();
        assert_eq!(wo.workspace.include, vec!["src/**"]);
        assert_eq!(wo.workspace.exclude, vec!["*.log"]);
    }
}

// ── 4. ReceiptBuilder default values ────────────────────────────────

mod receipt_builder_defaults {
    use super::*;

    #[test]
    fn default_outcome_is_complete() {
        let r = ReceiptBuilder::new("test").build();
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn default_mode_is_mapped() {
        let r = ReceiptBuilder::new("test").build();
        assert_eq!(r.mode, ExecutionMode::Mapped);
    }

    #[test]
    fn default_work_order_id_is_nil() {
        let r = ReceiptBuilder::new("test").build();
        assert_eq!(r.meta.work_order_id, Uuid::nil());
    }

    #[test]
    fn default_contract_version_matches_constant() {
        let r = ReceiptBuilder::new("test").build();
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn default_receipt_sha256_is_none() {
        let r = ReceiptBuilder::new("test").build();
        assert!(r.receipt_sha256.is_none());
    }

    #[test]
    fn default_capabilities_empty() {
        let r = ReceiptBuilder::new("test").build();
        assert!(r.capabilities.is_empty());
    }

    #[test]
    fn default_trace_empty() {
        let r = ReceiptBuilder::new("test").build();
        assert!(r.trace.is_empty());
    }

    #[test]
    fn default_artifacts_empty() {
        let r = ReceiptBuilder::new("test").build();
        assert!(r.artifacts.is_empty());
    }

    #[test]
    fn backend_id_is_stored() {
        let r = ReceiptBuilder::new("my-backend").build();
        assert_eq!(r.backend.id, "my-backend");
    }

    #[test]
    fn default_backend_versions_none() {
        let r = ReceiptBuilder::new("test").build();
        assert!(r.backend.backend_version.is_none());
        assert!(r.backend.adapter_version.is_none());
    }

    /// Duration must be computed from started_at / finished_at, not hardcoded.
    #[test]
    fn duration_computed_from_timestamps() {
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 5).unwrap();
        let r = ReceiptBuilder::new("test")
            .started_at(start)
            .finished_at(end)
            .build();
        assert_eq!(r.meta.duration_ms, 5000);
    }

    /// When started_at == finished_at, duration must be 0.
    #[test]
    fn zero_duration_when_same_timestamps() {
        let ts = fixed_ts();
        let r = ReceiptBuilder::new("test")
            .started_at(ts)
            .finished_at(ts)
            .build();
        assert_eq!(r.meta.duration_ms, 0);
    }
}

// ── 5. Validation edge cases ────────────────────────────────────────

mod validation_edges {
    use super::*;

    /// A valid receipt with correct hash passes validation.
    #[test]
    fn valid_receipt_passes() {
        let r = minimal_receipt().with_hash().unwrap();
        assert!(validate_receipt(&r).is_ok());
    }

    /// Empty `backend.id` is rejected.
    #[test]
    fn empty_backend_id_rejected() {
        let mut r = minimal_receipt();
        r.backend.id = String::new();
        let errs = validate_receipt(&r).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::EmptyBackendId)),
            "must report EmptyBackendId"
        );
    }

    /// Wrong contract version is rejected.
    #[test]
    fn wrong_contract_version_rejected() {
        let mut r = minimal_receipt();
        r.meta.contract_version = "abp/v99.0".into();
        let errs = validate_receipt(&r).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::InvalidOutcome { .. })),
            "must report contract version mismatch"
        );
    }

    /// `started_at > finished_at` is rejected.
    #[test]
    fn inverted_timestamps_rejected() {
        let mut r = minimal_receipt();
        r.meta.started_at = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap();
        r.meta.finished_at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let errs = validate_receipt(&r).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::InvalidOutcome { .. })),
            "must report timestamp inversion"
        );
    }

    /// Tampered hash is detected.
    #[test]
    fn tampered_hash_detected() {
        let mut r = minimal_receipt().with_hash().unwrap();
        r.receipt_sha256 = Some("bad".repeat(22)); // wrong hash
        let errs = validate_receipt(&r).unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::InvalidHash { .. })),
            "must report hash mismatch"
        );
    }

    /// Receipt with `receipt_sha256 = None` passes (hash is optional).
    #[test]
    fn no_hash_passes_validation() {
        let r = minimal_receipt();
        assert!(r.receipt_sha256.is_none());
        assert!(validate_receipt(&r).is_ok());
    }

    /// Multiple errors are accumulated, not short-circuited.
    #[test]
    fn multiple_errors_accumulated() {
        let mut r = minimal_receipt();
        r.backend.id = String::new(); // error 1
        r.meta.contract_version = "wrong".into(); // error 2
        let errs = validate_receipt(&r).unwrap_err();
        assert!(errs.len() >= 2, "must accumulate all errors, got {}", errs.len());
    }
}

// ── 6. CONTRACT_VERSION constant ────────────────────────────────────

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

// ── 7. ExecutionMode default ────────────────────────────────────────

#[test]
fn execution_mode_default_is_mapped() {
    assert_eq!(ExecutionMode::default(), ExecutionMode::Mapped);
}

// ── 8. canonical_json determinism ───────────────────────────────────

#[test]
fn canonical_json_produces_string() {
    let r = minimal_receipt();
    let json = canonical_json(&r).unwrap();
    assert!(!json.is_empty());
    // Must be valid JSON
    let _: serde_json::Value = serde_json::from_str(&json).unwrap();
}

// ── 9. sha256_hex ───────────────────────────────────────────────────

#[test]
fn sha256_hex_empty_input() {
    // SHA-256 of empty string is a well-known constant.
    let h = sha256_hex(b"");
    assert_eq!(h, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

#[test]
fn sha256_hex_known_input() {
    // SHA-256("hello") is a well-known constant.
    let h = sha256_hex(b"hello");
    assert_eq!(h, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
}
