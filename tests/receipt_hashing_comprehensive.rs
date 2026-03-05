#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]

//! Comprehensive receipt hashing and canonicalization test suite.
//!
//! Validates the critical ABP invariant: `receipt_hash()` sets `receipt_sha256`
//! to `null` before hashing, preventing self-referential hashes.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ContractError, ExecutionMode, Outcome, Receipt, ReceiptBuilder,
    RunMetadata, SupportLevel, UsageNormalized, VerificationReport, canonical_json, receipt_hash,
    sha256_hex,
};
use abp_receipt::{canonicalize, compute_hash, verify_hash};
use abp_runtime::store::{ReceiptStorage, ReceiptStore};
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fixed timestamp for deterministic tests.
fn fixed_ts() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

/// A second fixed timestamp one minute later.
fn fixed_ts_later() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 1, 0).unwrap()
}

/// Build a minimal, fully-deterministic receipt.
fn minimal_receipt() -> Receipt {
    Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
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

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

/// Build a receipt with every field populated.
fn fully_populated_receipt() -> Receipt {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::Streaming, SupportLevel::Emulated);
    caps.insert(Capability::ToolBash, SupportLevel::Unsupported);

    Receipt {
        meta: RunMetadata {
            run_id: Uuid::from_u128(1),
            work_order_id: Uuid::from_u128(2),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts_later(),
            duration_ms: 60_000,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.2.3".into()),
            adapter_version: Some("0.9.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Passthrough,
        usage_raw: serde_json::json!({"prompt_tokens": 100, "completion_tokens": 50}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.003),
        },
        trace: vec![
            make_event(AgentEventKind::RunStarted {
                message: "Starting task".into(),
            }),
            make_event(AgentEventKind::AssistantMessage {
                text: "Hello, world!".into(),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu-1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "echo hi"}),
            }),
            make_event(AgentEventKind::ToolResult {
                tool_name: "bash".into(),
                tool_use_id: Some("tu-1".into()),
                output: serde_json::json!({"stdout": "hi\n"}),
                is_error: false,
            }),
            make_event(AgentEventKind::FileChanged {
                path: "src/main.rs".into(),
                summary: "Added hello world".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "Done".into(),
            }),
        ],
        artifacts: vec![
            ArtifactRef {
                kind: "patch".into(),
                path: "output.patch".into(),
            },
            ArtifactRef {
                kind: "log".into(),
                path: "run.log".into(),
            },
        ],
        verification: VerificationReport {
            git_diff: Some("diff --git a/src/main.rs".into()),
            git_status: Some("M src/main.rs".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

// ===========================================================================
// 1. Basic hashing: deterministic for same inputs
// ===========================================================================

#[test]
fn basic_hash_is_deterministic() {
    let r = minimal_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn basic_hash_deterministic_across_reconstructions() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let h2 = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn basic_hash_via_abp_receipt_compute_hash_matches_core() {
    let r = minimal_receipt();
    let h_core = receipt_hash(&r).unwrap();
    let h_receipt = compute_hash(&r).unwrap();
    assert_eq!(h_core, h_receipt);
}

// ===========================================================================
// 2. Self-referential prevention: receipt_sha256 excluded from hash
// ===========================================================================

#[test]
fn self_ref_none_vs_some_produces_same_hash() {
    let mut r1 = minimal_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("bogus_value".into());
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn self_ref_different_hash_strings_produce_same_hash() {
    let mut ra = minimal_receipt();
    ra.receipt_sha256 = Some("aaaa".into());
    let mut rb = minimal_receipt();
    rb.receipt_sha256 = Some("bbbb".into());
    assert_eq!(receipt_hash(&ra).unwrap(), receipt_hash(&rb).unwrap());
}

#[test]
fn self_ref_hash_then_rehash_is_stable() {
    let r = minimal_receipt().with_hash().unwrap();
    let stored = r.receipt_sha256.clone().unwrap();
    let recomputed = receipt_hash(&r).unwrap();
    assert_eq!(stored, recomputed);
}

#[test]
fn self_ref_populated_receipt_sha256_nullified_in_canonical() {
    let mut r = minimal_receipt();
    r.receipt_sha256 = Some("deadbeefcafe".into());
    let json = canonicalize(&r).unwrap();
    assert!(json.contains("\"receipt_sha256\":null"));
    assert!(!json.contains("deadbeefcafe"));
}

#[test]
fn self_ref_canonical_json_identical_regardless_of_stored_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("anything".into());
    assert_eq!(canonicalize(&r1).unwrap(), canonicalize(&r2).unwrap());
}

// ===========================================================================
// 3. Canonical JSON: BTreeMap ensures deterministic key ordering
// ===========================================================================

#[test]
fn canonical_btreemap_insertion_order_irrelevant() {
    let mut caps_a = CapabilityManifest::new();
    caps_a.insert(Capability::ToolRead, SupportLevel::Native);
    caps_a.insert(Capability::Streaming, SupportLevel::Emulated);

    let mut caps_b = CapabilityManifest::new();
    caps_b.insert(Capability::Streaming, SupportLevel::Emulated);
    caps_b.insert(Capability::ToolRead, SupportLevel::Native);

    let mut r1 = minimal_receipt();
    r1.capabilities = caps_a;
    let mut r2 = minimal_receipt();
    r2.capabilities = caps_b;

    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn canonical_vendor_map_order_irrelevant() {
    let mut v1 = BTreeMap::new();
    v1.insert("z_key".to_string(), serde_json::json!("z_val"));
    v1.insert("a_key".to_string(), serde_json::json!("a_val"));

    let mut v2 = BTreeMap::new();
    v2.insert("a_key".to_string(), serde_json::json!("a_val"));
    v2.insert("z_key".to_string(), serde_json::json!("z_val"));

    let mut r1 = minimal_receipt();
    r1.usage_raw = serde_json::to_value(&v1).unwrap();
    let mut r2 = minimal_receipt();
    r2.usage_raw = serde_json::to_value(&v2).unwrap();

    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn canonical_json_is_compact_no_newlines() {
    let r = fully_populated_receipt();
    let json = canonicalize(&r).unwrap();
    assert!(!json.contains('\n'));
    assert!(!json.contains("  ")); // no indentation
}

#[test]
fn canonical_json_helper_sorts_keys() {
    let json = canonical_json(&serde_json::json!({"z": 1, "a": 2, "m": 3})).unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let m_pos = json.find("\"m\"").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

// ===========================================================================
// 4. with_hash() method: correctly computes and sets hash
// ===========================================================================

#[test]
fn with_hash_sets_receipt_sha256() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn with_hash_is_verifiable() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(verify_hash(&r));
}

#[test]
fn with_hash_value_matches_receipt_hash() {
    let r = minimal_receipt();
    let expected = receipt_hash(&r).unwrap();
    let hashed = r.with_hash().unwrap();
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(expected.as_str()));
}

#[test]
fn with_hash_via_builder() {
    let r = ReceiptBuilder::new("test")
        .outcome(Outcome::Failed)
        .with_hash()
        .unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

#[test]
fn with_hash_on_fully_populated_receipt() {
    let r = fully_populated_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    assert!(verify_hash(&r));
}

// ===========================================================================
// 5. Different receipts produce different hashes
// ===========================================================================

#[test]
fn different_outcome_different_hash() {
    let h_complete = receipt_hash(&Receipt {
        outcome: Outcome::Complete,
        ..minimal_receipt()
    })
    .unwrap();
    let h_failed = receipt_hash(&Receipt {
        outcome: Outcome::Failed,
        ..minimal_receipt()
    })
    .unwrap();
    let h_partial = receipt_hash(&Receipt {
        outcome: Outcome::Partial,
        ..minimal_receipt()
    })
    .unwrap();
    assert_ne!(h_complete, h_failed);
    assert_ne!(h_complete, h_partial);
    assert_ne!(h_failed, h_partial);
}

#[test]
fn different_backend_id_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.id = "other-backend".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_backend_version_different_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.backend_version = Some("v2.0".into());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn different_duration_different_hash() {
    let mut r2 = minimal_receipt();
    r2.meta.duration_ms = 9999;
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

#[test]
fn different_trace_different_hash() {
    let mut r2 = minimal_receipt();
    r2.trace.push(make_event(AgentEventKind::RunStarted {
        message: "go".into(),
    }));
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

#[test]
fn different_mode_different_hash() {
    let mut r2 = minimal_receipt();
    r2.mode = ExecutionMode::Passthrough;
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

#[test]
fn different_artifacts_different_hash() {
    let mut r2 = minimal_receipt();
    r2.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "a.patch".into(),
    });
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

#[test]
fn different_verification_different_hash() {
    let mut r2 = minimal_receipt();
    r2.verification.harness_ok = true;
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

#[test]
fn different_usage_raw_different_hash() {
    let mut r2 = minimal_receipt();
    r2.usage_raw = serde_json::json!({"tokens": 42});
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

// ===========================================================================
// 6. Same receipt data produces same hash across multiple calls
// ===========================================================================

#[test]
fn idempotent_hash_ten_calls() {
    let r = fully_populated_receipt();
    let hashes: Vec<String> = (0..10).map(|_| receipt_hash(&r).unwrap()).collect();
    for h in &hashes {
        assert_eq!(h, &hashes[0]);
    }
}

#[test]
fn idempotent_compute_hash_matches_receipt_hash() {
    let r = fully_populated_receipt();
    for _ in 0..5 {
        assert_eq!(receipt_hash(&r).unwrap(), compute_hash(&r).unwrap());
    }
}

// ===========================================================================
// 7. Hash format validation (hex-encoded SHA-256)
// ===========================================================================

#[test]
fn hash_is_64_hex_chars() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h.len(), 64, "SHA-256 hex digest must be 64 characters");
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hash_is_lowercase_hex() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn sha256_hex_utility_matches() {
    let r = minimal_receipt();
    let canonical = canonicalize(&r).unwrap();
    let expected = sha256_hex(canonical.as_bytes());
    assert_eq!(receipt_hash(&r).unwrap(), expected);
}

#[test]
fn hash_format_for_fully_populated() {
    let h = receipt_hash(&fully_populated_receipt()).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(h, h.to_lowercase());
}

// ===========================================================================
// 8. Receipt roundtrip: serialize → deserialize → hash matches
// ===========================================================================

#[test]
fn roundtrip_serde_json_preserves_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, deserialized.receipt_sha256);
    assert!(verify_hash(&deserialized));
}

#[test]
fn roundtrip_pretty_json_preserves_hash() {
    let r = fully_populated_receipt().with_hash().unwrap();
    let json = serde_json::to_string_pretty(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&deserialized));
}

#[test]
fn roundtrip_value_intermediary_preserves_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    let val = serde_json::to_value(&r).unwrap();
    let deserialized: Receipt = serde_json::from_value(val).unwrap();
    assert_eq!(r.receipt_sha256, deserialized.receipt_sha256);
    assert!(verify_hash(&deserialized));
}

#[test]
fn roundtrip_without_hash_then_add_hash() {
    let r = minimal_receipt();
    let json = serde_json::to_string(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    let hashed = deserialized.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
    assert_eq!(
        hashed.receipt_sha256.as_deref().unwrap(),
        receipt_hash(&r).unwrap()
    );
}

// ===========================================================================
// 9. Empty/minimal receipt hashing
// ===========================================================================

#[test]
fn empty_backend_id_hashes_fine() {
    let mut r = minimal_receipt();
    r.backend.id = String::new();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn minimal_receipt_hash_stable() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let h2 = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn minimal_receipt_with_hash_verifies() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(verify_hash(&r));
}

// ===========================================================================
// 10. Receipt with all fields populated
// ===========================================================================

#[test]
fn fully_populated_hash_is_valid() {
    let h = receipt_hash(&fully_populated_receipt()).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn fully_populated_hash_is_deterministic() {
    let h1 = receipt_hash(&fully_populated_receipt()).unwrap();
    let h2 = receipt_hash(&fully_populated_receipt()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn fully_populated_with_hash_roundtrips() {
    let r = fully_populated_receipt().with_hash().unwrap();
    let json = serde_json::to_string_pretty(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert!(verify_hash(&deserialized));
    assert_eq!(r.receipt_sha256, deserialized.receipt_sha256);
}

#[test]
fn fully_populated_differs_from_minimal() {
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&fully_populated_receipt()).unwrap()
    );
}

// ===========================================================================
// 11. Receipt store integration: store and retrieve by hash
// ===========================================================================

#[test]
fn store_save_and_load_by_hash_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let r = minimal_receipt().with_hash().unwrap();
    let hash = r.receipt_sha256.clone().unwrap();

    store.save_by_hash(&r).unwrap();
    let loaded = store.load_by_hash(&hash).unwrap();
    assert_eq!(loaded.receipt_sha256, r.receipt_sha256);
    assert!(verify_hash(&loaded));
}

#[test]
fn store_verify_integrity_passes_for_valid_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let r = fully_populated_receipt().with_hash().unwrap();
    let hash = r.receipt_sha256.clone().unwrap();

    store.save_by_hash(&r).unwrap();
    assert!(store.verify_integrity(&hash).unwrap());
}

#[test]
fn store_list_hashes_returns_saved() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let r1 = ReceiptBuilder::new("backend-a")
        .outcome(Outcome::Complete)
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .with_hash()
        .unwrap();
    let r2 = ReceiptBuilder::new("backend-b")
        .outcome(Outcome::Failed)
        .started_at(fixed_ts())
        .finished_at(fixed_ts())
        .with_hash()
        .unwrap();

    store.save_by_hash(&r1).unwrap();
    store.save_by_hash(&r2).unwrap();

    let mut hashes = store.list_hashes().unwrap();
    hashes.sort();
    let mut expected = vec![
        r1.receipt_sha256.clone().unwrap(),
        r2.receipt_sha256.clone().unwrap(),
    ];
    expected.sort();
    assert_eq!(hashes, expected);
}

#[test]
fn store_rejects_receipt_without_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());
    let r = minimal_receipt(); // no hash
    assert!(store.save_by_hash(&r).is_err());
}

#[test]
fn store_loaded_receipt_hash_recomputes_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let store = ReceiptStore::new(dir.path());

    let r = fully_populated_receipt().with_hash().unwrap();
    let hash = r.receipt_sha256.clone().unwrap();
    store.save_by_hash(&r).unwrap();

    let loaded = store.load_by_hash(&hash).unwrap();
    let recomputed = receipt_hash(&loaded).unwrap();
    assert_eq!(recomputed, hash);
}

// ===========================================================================
// 12. Timestamp doesn't affect hash in unexpected ways
// ===========================================================================

#[test]
fn timestamp_change_changes_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.started_at = Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn finished_at_change_changes_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.finished_at = fixed_ts_later();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn same_timestamps_same_hash() {
    let r1 = minimal_receipt();
    let r2 = minimal_receipt();
    assert_eq!(r1.meta.started_at, r2.meta.started_at);
    assert_eq!(r1.meta.finished_at, r2.meta.finished_at);
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 13. Agent event ordering affects receipt hash
// ===========================================================================

#[test]
fn event_order_matters_for_hash() {
    let evt_a = make_event(AgentEventKind::RunStarted {
        message: "start".into(),
    });
    let evt_b = make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    });

    let mut r1 = minimal_receipt();
    r1.trace = vec![evt_a.clone(), evt_b.clone()];

    let mut r2 = minimal_receipt();
    r2.trace = vec![evt_b, evt_a];

    // Trace is a Vec (ordered), so different ordering produces different hash.
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn event_content_change_changes_hash() {
    let mut r1 = minimal_receipt();
    r1.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: "hello".into(),
    }));

    let mut r2 = minimal_receipt();
    r2.trace.push(make_event(AgentEventKind::AssistantMessage {
        text: "goodbye".into(),
    }));

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

// ===========================================================================
// 14. Property: for any receipt r, r.with_hash().receipt_sha256.is_some()
// ===========================================================================

#[test]
fn property_with_hash_always_sets_some_minimal() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn property_with_hash_always_sets_some_full() {
    let r = fully_populated_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[test]
fn property_with_hash_always_sets_some_empty_backend() {
    let mut r = minimal_receipt();
    r.backend.id = String::new();
    let hashed = r.with_hash().unwrap();
    assert!(hashed.receipt_sha256.is_some());
}

#[test]
fn property_with_hash_always_sets_some_all_outcomes() {
    for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
        let r = Receipt {
            outcome,
            ..minimal_receipt()
        }
        .with_hash()
        .unwrap();
        assert!(
            r.receipt_sha256.is_some(),
            "with_hash() must set receipt_sha256 for all outcomes"
        );
    }
}

#[test]
fn property_with_hash_always_sets_some_all_modes() {
    for mode in [ExecutionMode::Mapped, ExecutionMode::Passthrough] {
        let r = Receipt {
            mode,
            ..minimal_receipt()
        }
        .with_hash()
        .unwrap();
        assert!(r.receipt_sha256.is_some());
    }
}

#[test]
fn property_with_hash_result_passes_verify() {
    let receipts = vec![
        minimal_receipt(),
        fully_populated_receipt(),
        Receipt {
            outcome: Outcome::Failed,
            ..minimal_receipt()
        },
        Receipt {
            mode: ExecutionMode::Passthrough,
            ..minimal_receipt()
        },
    ];
    for r in receipts {
        let hashed = r.with_hash().unwrap();
        assert!(
            verify_hash(&hashed),
            "with_hash() result must pass verify_hash()"
        );
    }
}

// ===========================================================================
// Additional edge cases
// ===========================================================================

#[test]
fn verify_hash_returns_true_when_no_hash_stored() {
    let r = minimal_receipt();
    assert!(r.receipt_sha256.is_none());
    assert!(verify_hash(&r));
}

#[test]
fn verify_hash_returns_false_for_tampered_outcome() {
    let mut r = minimal_receipt().with_hash().unwrap();
    r.outcome = Outcome::Failed;
    assert!(!verify_hash(&r));
}

#[test]
fn verify_hash_returns_false_for_garbage_hash() {
    let mut r = minimal_receipt();
    r.receipt_sha256 = Some("not_a_real_hash".into());
    assert!(!verify_hash(&r));
}

#[test]
fn unicode_in_backend_id_hashes_correctly() {
    let mut r = minimal_receipt();
    r.backend.id = "バックエンド🚀".into();
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h, h2);
}

#[test]
fn large_trace_hashes_correctly() {
    let mut r = minimal_receipt();
    for i in 0..200 {
        r.trace.push(make_event(AgentEventKind::AssistantDelta {
            text: format!("token {i}"),
        }));
    }
    let hashed = r.with_hash().unwrap();
    assert!(verify_hash(&hashed));
    assert_eq!(hashed.trace.len(), 200);
}

#[test]
fn ext_field_on_event_affects_hash() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".to_string(),
        serde_json::json!({"custom": true}),
    );

    let mut r1 = minimal_receipt();
    r1.trace.push(AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    });

    let mut r2 = minimal_receipt();
    r2.trace.push(AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: Some(ext),
    });

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn contract_error_is_debug_display() {
    // Ensure ContractError implements the expected traits.
    let err =
        ContractError::Json(serde_json::from_str::<serde_json::Value>("invalid").unwrap_err());
    let msg = format!("{}", err);
    assert!(!msg.is_empty());
}

#[test]
fn run_id_change_changes_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.run_id = Uuid::from_u128(42);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn work_order_id_change_changes_hash() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.meta.work_order_id = Uuid::from_u128(99);
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}
