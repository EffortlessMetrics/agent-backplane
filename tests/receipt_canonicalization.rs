//! Comprehensive receipt canonicalization and hashing test suite.
//!
//! Covers deterministic hashing, self-referential prevention, canonical JSON,
//! receipt chain integrity, and edge cases.

use std::collections::BTreeMap;

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, BackendIdentity, CONTRACT_VERSION, Capability,
    CapabilityManifest, ExecutionMode, Outcome, Receipt, ReceiptBuilder, RunMetadata, SupportLevel,
    UsageNormalized, VerificationReport, receipt_hash,
};
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fixed timestamp for deterministic tests.
fn fixed_ts() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
}

/// Build a minimal receipt with all fields fully specified for deterministic hashing.
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

/// Build a receipt identical to `minimal_receipt` but with a specific field mutated.
fn receipt_with_outcome(outcome: Outcome) -> Receipt {
    Receipt {
        outcome,
        ..minimal_receipt()
    }
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: fixed_ts(),
        kind,
        ext: None,
    }
}

// ===========================================================================
// (a) Deterministic hashing — 8+ tests
// ===========================================================================

#[test]
fn deterministic_same_receipt_same_hash() {
    let r = minimal_receipt();
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn deterministic_reconstructed_receipt_same_hash() {
    let h1 = receipt_hash(&minimal_receipt()).unwrap();
    let h2 = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn deterministic_hash_changes_when_outcome_changes() {
    let h_complete = receipt_hash(&receipt_with_outcome(Outcome::Complete)).unwrap();
    let h_failed = receipt_hash(&receipt_with_outcome(Outcome::Failed)).unwrap();
    let h_partial = receipt_hash(&receipt_with_outcome(Outcome::Partial)).unwrap();
    assert_ne!(h_complete, h_failed);
    assert_ne!(h_complete, h_partial);
    assert_ne!(h_failed, h_partial);
}

#[test]
fn deterministic_hash_changes_when_backend_id_changes() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.id = "other-backend".into();
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn deterministic_hash_changes_when_model_version_changes() {
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.backend.backend_version = Some("v1.0.0".into());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn deterministic_hash_changes_when_duration_ms_changes() {
    let mut r2 = minimal_receipt();
    r2.meta.duration_ms = 9999;
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

#[test]
fn deterministic_hash_changes_when_timing_changes() {
    let mut r2 = minimal_receipt();
    r2.meta.started_at = Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap();
    assert_ne!(
        receipt_hash(&minimal_receipt()).unwrap(),
        receipt_hash(&r2).unwrap()
    );
}

#[test]
fn deterministic_hash_changes_when_events_added() {
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
fn deterministic_hash_format_is_valid_sha256() {
    let h = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    // SHA-256 hex is always lowercase
    assert_eq!(h, h.to_lowercase());
}

#[test]
fn deterministic_hash_with_none_fields() {
    // All optional fields are None — should still hash fine
    let r = minimal_receipt();
    assert!(r.backend.backend_version.is_none());
    assert!(r.backend.adapter_version.is_none());
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn deterministic_btreemap_order_independence() {
    // Build two capability maps with different insertion orders.
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

// ===========================================================================
// (b) Self-referential prevention — 5+ tests
// ===========================================================================

#[test]
fn self_ref_receipt_sha256_excluded_from_hash() {
    let mut r1 = minimal_receipt();
    r1.receipt_sha256 = None;
    let mut r2 = minimal_receipt();
    r2.receipt_sha256 = Some("bogus_value".into());
    assert_eq!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn self_ref_different_hash_values_dont_change_hash() {
    let mut ra = minimal_receipt();
    ra.receipt_sha256 = Some("aaaa".into());
    let mut rb = minimal_receipt();
    rb.receipt_sha256 = Some("bbbb".into());
    let mut rc = minimal_receipt();
    rc.receipt_sha256 =
        Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into());
    let base = receipt_hash(&minimal_receipt()).unwrap();
    assert_eq!(base, receipt_hash(&ra).unwrap());
    assert_eq!(base, receipt_hash(&rb).unwrap());
    assert_eq!(base, receipt_hash(&rc).unwrap());
}

#[test]
fn self_ref_with_hash_fills_field() {
    let r = minimal_receipt().with_hash().unwrap();
    assert!(r.receipt_sha256.is_some());
    let stored = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(stored.len(), 64);
}

#[test]
fn self_ref_double_hash_is_idempotent() {
    let r1 = minimal_receipt().with_hash().unwrap();
    let r2 = r1.clone().with_hash().unwrap();
    assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
}

#[test]
fn self_ref_manual_hash_setting_does_not_affect_recomputation() {
    let mut r = minimal_receipt();
    r.receipt_sha256 = Some("manually_set_wrong_value".into());
    let correct = receipt_hash(&r).unwrap();
    let r_hashed = r.with_hash().unwrap();
    assert_eq!(r_hashed.receipt_sha256.as_deref(), Some(correct.as_str()));
}

#[test]
fn self_ref_with_hash_matches_receipt_hash() {
    let r = minimal_receipt();
    let expected = receipt_hash(&r).unwrap();
    let hashed = r.with_hash().unwrap();
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(expected.as_str()));
}

// ===========================================================================
// (c) Canonical JSON — 5+ tests
// ===========================================================================

#[test]
fn canonical_json_deterministic_serialization() {
    let r = minimal_receipt();
    let j1 = serde_json::to_string(&r).unwrap();
    let j2 = serde_json::to_string(&r).unwrap();
    assert_eq!(j1, j2);
}

#[test]
fn canonical_json_field_order_is_alphabetical() {
    let r = minimal_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    if let serde_json::Value::Object(map) = &v {
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "Top-level keys should be sorted (BTreeMap)");
    } else {
        panic!("Receipt should serialize to a JSON object");
    }
}

#[test]
fn canonical_json_duration_ms_is_integer() {
    let r = minimal_receipt();
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    let meta = &v["meta"];
    let dur = &meta["duration_ms"];
    assert!(
        dur.is_u64(),
        "duration_ms must serialize as integer, got {dur}"
    );
}

#[test]
fn canonical_json_unicode_stability() {
    let mut r1 = minimal_receipt();
    r1.backend.id = "バックエンド-🚀".into();
    let h1 = receipt_hash(&r1).unwrap();
    let h2 = receipt_hash(&r1).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
}

#[test]
fn canonical_json_empty_vec_vs_none_different_hashes() {
    // Empty trace (vec![]) vs receipt with verification git_diff = None vs Some("")
    let r1 = minimal_receipt();
    let mut r2 = minimal_receipt();
    r2.verification.git_diff = Some(String::new());
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn canonical_json_roundtrip_preserves_hash() {
    let r = minimal_receipt().with_hash().unwrap();
    let json = serde_json::to_string(&r).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, deserialized.receipt_sha256);
    // Recomputing hash on the deserialized receipt should match
    let recomputed = receipt_hash(&deserialized).unwrap();
    assert_eq!(r.receipt_sha256.as_deref(), Some(recomputed.as_str()));
}

// ===========================================================================
// (d) Receipt chain integrity — 5+ tests
// ===========================================================================

#[test]
fn chain_sequential_receipts_get_unique_run_ids() {
    let ts = fixed_ts();
    let r1 = ReceiptBuilder::new("mock")
        .started_at(ts)
        .finished_at(ts)
        .build();
    let r2 = ReceiptBuilder::new("mock")
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[test]
fn chain_work_order_id_links_receipt_to_order() {
    let wo_id = Uuid::new_v4();
    let ts = fixed_ts();
    let r = ReceiptBuilder::new("mock")
        .work_order_id(wo_id)
        .started_at(ts)
        .finished_at(ts)
        .build();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[test]
fn chain_each_receipt_hash_is_valid() {
    let ts = fixed_ts();
    let chain: Vec<Receipt> = (0..5)
        .map(|i| {
            let end = ts + chrono::Duration::seconds(i + 1);
            ReceiptBuilder::new("mock")
                .started_at(ts)
                .finished_at(end)
                .with_hash()
                .unwrap()
        })
        .collect();

    for r in &chain {
        let stored = r.receipt_sha256.as_ref().expect("hash should be set");
        let recomputed = receipt_hash(r).unwrap();
        assert_eq!(
            stored, &recomputed,
            "Hash mismatch in chain for run_id {}",
            r.meta.run_id
        );
    }
}

#[test]
fn chain_timestamps_monotonic() {
    let base = fixed_ts();
    let chain: Vec<Receipt> = (0..5)
        .map(|i| {
            let start = base + chrono::Duration::seconds(i * 10);
            let end = start + chrono::Duration::seconds(5);
            ReceiptBuilder::new("mock")
                .started_at(start)
                .finished_at(end)
                .build()
        })
        .collect();

    for window in chain.windows(2) {
        assert!(
            window[1].meta.started_at >= window[0].meta.finished_at,
            "Receipt timestamps should be monotonic"
        );
    }
}

#[test]
fn chain_contract_version_present_in_all_receipts() {
    let ts = fixed_ts();
    let chain: Vec<Receipt> = (0..3)
        .map(|_| {
            ReceiptBuilder::new("mock")
                .started_at(ts)
                .finished_at(ts)
                .build()
        })
        .collect();
    for r in &chain {
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }
}

#[test]
fn chain_all_hashes_unique() {
    // Different run_ids → different hashes
    let ts = fixed_ts();
    let chain: Vec<Receipt> = (0..10)
        .map(|_| {
            ReceiptBuilder::new("mock")
                .started_at(ts)
                .finished_at(ts)
                .with_hash()
                .unwrap()
        })
        .collect();
    let hashes: Vec<&str> = chain
        .iter()
        .map(|r| r.receipt_sha256.as_deref().unwrap())
        .collect();
    let unique: std::collections::HashSet<&str> = hashes.iter().copied().collect();
    assert_eq!(
        hashes.len(),
        unique.len(),
        "All hashes in a chain must be unique"
    );
}

// ===========================================================================
// (e) Edge cases — 7+ tests
// ===========================================================================

#[test]
fn edge_max_length_strings() {
    let long = "x".repeat(100_000);
    let mut r = minimal_receipt();
    r.backend.id = long.clone();
    r.verification.git_diff = Some(long);
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_zero_events() {
    let r = minimal_receipt();
    assert!(r.trace.is_empty());
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_all_optional_fields_none() {
    let r = Receipt {
        meta: RunMetadata {
            run_id: Uuid::nil(),
            work_order_id: Uuid::nil(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts(),
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
        usage_raw: serde_json::json!(null),
        usage: UsageNormalized {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            request_units: None,
            estimated_cost_usd: None,
        },
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: false,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn edge_all_optional_fields_some() {
    let mut caps = CapabilityManifest::new();
    caps.insert(Capability::ToolRead, SupportLevel::Native);
    caps.insert(Capability::ToolWrite, SupportLevel::Emulated);
    caps.insert(Capability::Streaming, SupportLevel::Unsupported);

    let r = Receipt {
        meta: RunMetadata {
            run_id: Uuid::new_v4(),
            work_order_id: Uuid::new_v4(),
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: fixed_ts(),
            finished_at: fixed_ts() + chrono::Duration::seconds(120),
            duration_ms: 120_000,
        },
        backend: BackendIdentity {
            id: "sidecar:node".into(),
            backend_version: Some("1.2.3".into()),
            adapter_version: Some("0.9.0".into()),
        },
        capabilities: caps,
        mode: ExecutionMode::Passthrough,
        usage_raw: serde_json::json!({"model": "gpt-4", "prompt_tokens": 100}),
        usage: UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cache_read_tokens: Some(10),
            cache_write_tokens: Some(5),
            request_units: Some(1),
            estimated_cost_usd: Some(0.05),
        },
        trace: vec![
            make_event(AgentEventKind::RunStarted {
                message: "starting".into(),
            }),
            make_event(AgentEventKind::AssistantMessage {
                text: "hello world".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ],
        artifacts: vec![ArtifactRef {
            kind: "patch".into(),
            path: "output.diff".into(),
        }],
        verification: VerificationReport {
            git_diff: Some("--- a/file\n+++ b/file\n@@ ...\n+line".into()),
            git_status: Some("M file".into()),
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    };
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    // Should be hashable and with_hash-able
    let hashed = r.with_hash().unwrap();
    assert_eq!(hashed.receipt_sha256.as_deref(), Some(h.as_str()));
}

#[test]
fn edge_special_characters_in_backend_id() {
    let specials = [
        "backend/with/slashes",
        "backend with spaces",
        "backend\twith\ttabs",
        "backend\"with\"quotes",
        "backend\\with\\backslashes",
        "<backend>&amp;{special}",
        "backend\nwith\nnewlines",
    ];
    let hashes: Vec<String> = specials
        .iter()
        .map(|id| {
            let mut r = minimal_receipt();
            r.backend.id = (*id).to_string();
            receipt_hash(&r).unwrap()
        })
        .collect();
    // All should be valid SHA-256 and unique
    for h in &hashes {
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
    let unique: std::collections::HashSet<&String> = hashes.iter().collect();
    assert_eq!(hashes.len(), unique.len());
}

#[test]
fn edge_very_large_events_count() {
    let mut r = minimal_receipt();
    for i in 0..1000 {
        r.trace.push(make_event(AgentEventKind::AssistantDelta {
            text: format!("token-{i}"),
        }));
    }
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);
    // Adding one more event changes the hash
    let h_before = h;
    r.trace.push(make_event(AgentEventKind::AssistantDelta {
        text: "one-more".into(),
    }));
    assert_ne!(h_before, receipt_hash(&r).unwrap());
}

#[test]
fn edge_metadata_with_nested_values() {
    let mut r = minimal_receipt();
    r.usage_raw = serde_json::json!({
        "model": "claude-3",
        "details": {
            "prompt_tokens": 500,
            "completion_tokens": 200,
            "nested": {
                "deep": true,
                "array": [1, 2, 3]
            }
        }
    });
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);

    // Same nested structure produces same hash
    let mut r2 = minimal_receipt();
    r2.usage_raw = serde_json::json!({
        "model": "claude-3",
        "details": {
            "prompt_tokens": 500,
            "completion_tokens": 200,
            "nested": {
                "deep": true,
                "array": [1, 2, 3]
            }
        }
    });
    assert_eq!(h, receipt_hash(&r2).unwrap());
}

#[test]
fn edge_event_with_ext_field() {
    let mut ext = BTreeMap::new();
    ext.insert("custom_key".to_string(), serde_json::json!("custom_value"));

    let mut r = minimal_receipt();
    r.trace.push(AgentEvent {
        ts: fixed_ts(),
        kind: AgentEventKind::RunStarted {
            message: "with ext".into(),
        },
        ext: Some(ext),
    });
    let h = receipt_hash(&r).unwrap();
    assert_eq!(h.len(), 64);

    // Without ext → different hash
    let mut r2 = minimal_receipt();
    r2.trace.push(make_event(AgentEventKind::RunStarted {
        message: "with ext".into(),
    }));
    assert_ne!(h, receipt_hash(&r2).unwrap());
}

#[test]
fn edge_artifact_differences_change_hash() {
    let mut r1 = minimal_receipt();
    r1.artifacts.push(ArtifactRef {
        kind: "patch".into(),
        path: "a.diff".into(),
    });

    let mut r2 = minimal_receipt();
    r2.artifacts.push(ArtifactRef {
        kind: "log".into(),
        path: "a.diff".into(),
    });

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn edge_execution_mode_changes_hash() {
    let mut r1 = minimal_receipt();
    r1.mode = ExecutionMode::Passthrough;
    let mut r2 = minimal_receipt();
    r2.mode = ExecutionMode::Mapped;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn edge_usage_normalized_changes_hash() {
    let mut r1 = minimal_receipt();
    r1.usage.input_tokens = Some(100);

    let mut r2 = minimal_receipt();
    r2.usage.input_tokens = Some(200);

    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}

#[test]
fn edge_verification_harness_ok_changes_hash() {
    let mut r1 = minimal_receipt();
    r1.verification.harness_ok = false;
    let mut r2 = minimal_receipt();
    r2.verification.harness_ok = true;
    assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
}
