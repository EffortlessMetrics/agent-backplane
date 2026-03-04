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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for receipt storage system — persistence, querying,
//! compression, integrity verification, and canonical hashing.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use abp_core::{
    AgentEvent, AgentEventKind, ArtifactRef, CONTRACT_VERSION, ExecutionMode, Outcome, Receipt,
    UsageNormalized, VerificationReport, receipt_hash,
};
use abp_receipt::{
    ChainError, ReceiptBuilder, ReceiptChain, canonicalize, compute_hash, diff_receipts,
    verify_hash,
};
use abp_runtime::store::{ReceiptStorage, ReceiptStore, StoreError};
use chrono::{DateTime, Duration, TimeZone, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap()
}

fn time_at(offset_secs: i64) -> DateTime<Utc> {
    base_time() + Duration::seconds(offset_secs)
}

/// Build a hashed receipt at a given second-offset.
fn make_receipt(backend: &str, offset: i64, outcome: Outcome) -> Receipt {
    let start = time_at(offset);
    let finish = start + Duration::milliseconds(200);
    ReceiptBuilder::new(backend)
        .outcome(outcome)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap()
}

/// Build a receipt with a specific run_id.
fn make_receipt_with_id(id: Uuid, offset: i64) -> Receipt {
    let start = time_at(offset);
    let finish = start + Duration::milliseconds(100);
    ReceiptBuilder::new("test-backend")
        .run_id(id)
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(finish)
        .with_hash()
        .unwrap()
}

/// Build a receipt without a hash.
fn make_unhashed_receipt(backend: &str, offset: i64) -> Receipt {
    let start = time_at(offset);
    let finish = start + Duration::milliseconds(100);
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .started_at(start)
        .finished_at(finish)
        .build()
}

// ===========================================================================
// 1. Receipt creation with proper hashing
// ===========================================================================

mod receipt_hashing {
    use super::*;

    #[test]
    fn with_hash_produces_64_hex_chars() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let hash = r.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn receipt_hash_nullifies_sha256_before_hashing() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        // Manually verify: canonical form has receipt_sha256 = null
        let canonical = canonicalize(&r).unwrap();
        assert!(canonical.contains("\"receipt_sha256\":null"));
    }

    #[test]
    fn hash_is_independent_of_prior_hash_value() {
        let r1 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(base_time())
            .finished_at(base_time() + Duration::seconds(1))
            .build();
        let mut r2 = r1.clone();
        r2.receipt_sha256 = Some("garbage_value".into());

        // receipt_hash should produce the same result regardless
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_changes_on_outcome_change() {
        let r1 = make_receipt("mock", 0, Outcome::Complete);
        let mut r2 = r1.clone();
        r2.receipt_sha256 = None;
        r2.outcome = Outcome::Failed;
        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_changes_on_backend_change() {
        let t = base_time();
        let r1 = ReceiptBuilder::new("alpha")
            .started_at(t)
            .finished_at(t)
            .build();
        let r2 = ReceiptBuilder::new("beta")
            .started_at(t)
            .finished_at(t)
            .build();
        assert_ne!(receipt_hash(&r1).unwrap(), receipt_hash(&r2).unwrap());
    }

    #[test]
    fn hash_deterministic_across_multiple_calls() {
        let r = make_receipt("mock", 42, Outcome::Partial);
        let h1 = receipt_hash(&r).unwrap();
        let h2 = receipt_hash(&r).unwrap();
        let h3 = receipt_hash(&r).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h2, h3);
    }

    #[test]
    fn compute_hash_matches_receipt_hash() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(base_time())
            .finished_at(base_time())
            .build();
        let h_core = receipt_hash(&r).unwrap();
        let h_receipt = compute_hash(&r).unwrap();
        assert_eq!(h_core, h_receipt);
    }

    #[test]
    fn with_hash_on_builder_vs_manual() {
        let t = base_time();
        let id = Uuid::nil();
        let wo_id = Uuid::nil();

        let r1 = ReceiptBuilder::new("mock")
            .run_id(id)
            .work_order_id(wo_id)
            .outcome(Outcome::Complete)
            .started_at(t)
            .finished_at(t)
            .with_hash()
            .unwrap();

        let mut r2 = ReceiptBuilder::new("mock")
            .run_id(id)
            .work_order_id(wo_id)
            .outcome(Outcome::Complete)
            .started_at(t)
            .finished_at(t)
            .build();
        r2.receipt_sha256 = Some(receipt_hash(&r2).unwrap());

        assert_eq!(r1.receipt_sha256, r2.receipt_sha256);
    }
}

// ===========================================================================
// 2. Receipt storage and retrieval (ReceiptStore file-based)
// ===========================================================================

mod store_persistence {
    use super::*;

    #[test]
    fn save_and_load_by_run_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        let run_id = r.meta.run_id;

        store.save(&r).unwrap();
        let loaded = store.load(run_id).unwrap();
        assert_eq!(loaded.meta.run_id, run_id);
        assert_eq!(loaded.outcome, Outcome::Complete);
        assert_eq!(loaded.receipt_sha256, r.receipt_sha256);
    }

    #[test]
    fn save_creates_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        let path = store.save(&r).unwrap();
        assert!(path.exists());
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("json"));
    }

    #[test]
    fn load_nonexistent_run_id_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let result = store.load(Uuid::new_v4());
        assert!(result.is_err());
    }

    #[test]
    fn list_returns_stored_run_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let r1 = make_receipt("a", 0, Outcome::Complete);
        let r2 = make_receipt("b", 10, Outcome::Failed);
        let id1 = r1.meta.run_id;
        let id2 = r2.meta.run_id;

        store.save(&r1).unwrap();
        store.save(&r2).unwrap();

        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn list_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let ids = store.list().unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn list_nonexistent_root_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path().join("nonexistent"));
        let ids = store.list().unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn save_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let id = Uuid::new_v4();

        let r1 = make_receipt_with_id(id, 0);
        store.save(&r1).unwrap();

        // Save again with different data but same run_id
        let r2 = ReceiptBuilder::new("updated-backend")
            .run_id(id)
            .outcome(Outcome::Failed)
            .started_at(time_at(0))
            .finished_at(time_at(1))
            .with_hash()
            .unwrap();
        store.save(&r2).unwrap();

        let loaded = store.load(id).unwrap();
        assert_eq!(loaded.backend.id, "updated-backend");
        assert_eq!(loaded.outcome, Outcome::Failed);
    }

    #[test]
    fn root_accessor() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        assert_eq!(store.root(), dir.path());
    }

    #[test]
    fn save_multiple_and_list_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let mut expected_ids = Vec::new();
        for i in 0..5 {
            let r = make_receipt("m", i * 10, Outcome::Complete);
            expected_ids.push(r.meta.run_id);
            store.save(&r).unwrap();
        }

        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 5);
        // list() returns sorted IDs
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }
}

// ===========================================================================
// 2b. ReceiptStorage trait (hash-keyed) persistence
// ===========================================================================

mod hash_keyed_storage {
    use super::*;

    #[test]
    fn save_and_load_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        let hash = r.receipt_sha256.clone().unwrap();

        let path = store.save_by_hash(&r).unwrap();
        assert!(path.exists());

        let loaded = store.load_by_hash(&hash).unwrap();
        assert_eq!(loaded.meta.run_id, r.meta.run_id);
        assert_eq!(loaded.receipt_sha256, r.receipt_sha256);
    }

    #[test]
    fn save_by_hash_requires_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_unhashed_receipt("mock", 0);
        let err = store.save_by_hash(&r).unwrap_err();
        assert!(matches!(err, StoreError::MissingHash));
    }

    #[test]
    fn load_by_hash_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let fake_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let err = store.load_by_hash(fake_hash).unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn list_hashes_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        assert!(store.list_hashes().unwrap().is_empty());
    }

    #[test]
    fn list_hashes_returns_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let r1 = make_receipt("a", 0, Outcome::Complete);
        let r2 = make_receipt("b", 10, Outcome::Failed);
        store.save_by_hash(&r1).unwrap();
        store.save_by_hash(&r2).unwrap();

        let hashes = store.list_hashes().unwrap();
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains(&r1.receipt_sha256.clone().unwrap()));
        assert!(hashes.contains(&r2.receipt_sha256.clone().unwrap()));
    }

    #[test]
    fn hash_and_run_id_stores_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);

        store.save(&r).unwrap();
        store.save_by_hash(&r).unwrap();

        // run_id listing should not include hash files and vice versa
        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], r.meta.run_id);

        let hashes = store.list_hashes().unwrap();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0], r.receipt_sha256.clone().unwrap());
    }

    #[test]
    fn save_by_hash_creates_by_hash_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        store.save_by_hash(&r).unwrap();

        let hash_dir = dir.path().join("by_hash");
        assert!(hash_dir.is_dir());
    }
}

// ===========================================================================
// 3. Receipt compression (gzip / zstd)
// ===========================================================================

mod compression {
    use super::*;

    #[test]
    fn gzip_round_trip_receipt_json() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let json = serde_json::to_string_pretty(&r).unwrap();
        let json_bytes = json.as_bytes();

        // Compress with flate2
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(json_bytes).unwrap();
        let compressed = encoder.finish().unwrap();

        // Compressed should be smaller for non-trivial data
        assert!(!compressed.is_empty());

        // Decompress
        let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();

        // Roundtrip: original JSON matches decompressed
        assert_eq!(json, decompressed);

        // Deserialize back to Receipt
        let loaded: Receipt = serde_json::from_str(&decompressed).unwrap();
        assert_eq!(loaded.meta.run_id, r.meta.run_id);
        assert_eq!(loaded.receipt_sha256, r.receipt_sha256);
    }

    #[test]
    fn zstd_round_trip_receipt_json() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let json = serde_json::to_string_pretty(&r).unwrap();

        // Compress
        let compressed = zstd::encode_all(json.as_bytes(), 3).unwrap();
        assert!(!compressed.is_empty());

        // Decompress
        let decompressed = zstd::decode_all(&compressed[..]).unwrap();
        let decompressed_str = String::from_utf8(decompressed).unwrap();
        assert_eq!(json, decompressed_str);

        // Deserialize
        let loaded: Receipt = serde_json::from_str(&decompressed_str).unwrap();
        assert_eq!(loaded.meta.run_id, r.meta.run_id);
    }

    #[test]
    fn gzip_preserves_hash_after_round_trip() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let json = serde_json::to_string(&r).unwrap();

        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(json.as_bytes()).unwrap();
        let compressed = enc.finish().unwrap();

        let mut dec = flate2::read::GzDecoder::new(&compressed[..]);
        let mut out = String::new();
        dec.read_to_string(&mut out).unwrap();

        let loaded: Receipt = serde_json::from_str(&out).unwrap();
        assert!(verify_hash(&loaded));
    }

    #[test]
    fn zstd_preserves_hash_after_round_trip() {
        let r = make_receipt("mock", 5, Outcome::Partial);
        let json = serde_json::to_string(&r).unwrap();

        let compressed = zstd::encode_all(json.as_bytes(), 10).unwrap();
        let decompressed = zstd::decode_all(&compressed[..]).unwrap();

        let loaded: Receipt =
            serde_json::from_str(std::str::from_utf8(&decompressed).unwrap()).unwrap();
        assert!(verify_hash(&loaded));
    }

    #[test]
    fn compressed_receipt_is_smaller_for_large_trace() {
        let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
        for i in 0..200 {
            builder = builder.add_trace_event(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token number {i} with some extra filler text to increase size"),
                },
                ext: None,
            });
        }
        let r = builder.with_hash().unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let json_len = json.len();

        let compressed = zstd::encode_all(json.as_bytes(), 3).unwrap();
        assert!(compressed.len() < json_len, "compressed should be smaller");
    }
}

// ===========================================================================
// 4. Receipt integrity verification
// ===========================================================================

mod integrity_verification {
    use super::*;

    #[test]
    fn verify_valid_receipt_returns_true() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        assert!(verify_hash(&r));
    }

    #[test]
    fn verify_unhashed_receipt_returns_true() {
        let r = make_unhashed_receipt("mock", 0);
        assert!(verify_hash(&r));
    }

    #[test]
    fn verify_tampered_outcome_returns_false() {
        let mut r = make_receipt("mock", 0, Outcome::Complete);
        r.outcome = Outcome::Failed;
        assert!(!verify_hash(&r));
    }

    #[test]
    fn verify_tampered_backend_id_returns_false() {
        let mut r = make_receipt("mock", 0, Outcome::Complete);
        r.backend.id = "evil".into();
        assert!(!verify_hash(&r));
    }

    #[test]
    fn verify_tampered_usage_raw_returns_false() {
        let mut r = make_receipt("mock", 0, Outcome::Complete);
        r.usage_raw = serde_json::json!({"injected": true});
        assert!(!verify_hash(&r));
    }

    #[test]
    fn verify_tampered_trace_returns_false() {
        let mut r = make_receipt("mock", 0, Outcome::Complete);
        r.trace.push(AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "injected".into(),
            },
            ext: None,
        });
        assert!(!verify_hash(&r));
    }

    #[test]
    fn verify_garbage_hash_returns_false() {
        let mut r = make_unhashed_receipt("mock", 0);
        r.receipt_sha256 = Some("not_a_valid_sha256".into());
        assert!(!verify_hash(&r));
    }

    #[test]
    fn store_verify_valid_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        let run_id = r.meta.run_id;
        store.save(&r).unwrap();
        assert!(store.verify(run_id).unwrap());
    }

    #[test]
    fn store_verify_tampered_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        let run_id = r.meta.run_id;
        let path = store.save(&r).unwrap();

        // Tamper with the file on disk
        let mut val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        val["outcome"] = serde_json::json!("failed");
        std::fs::write(&path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

        assert!(!store.verify(run_id).unwrap());
    }

    #[test]
    fn trait_verify_integrity_valid() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        let hash = r.receipt_sha256.clone().unwrap();
        store.save_by_hash(&r).unwrap();
        assert!(store.verify_integrity(&hash).unwrap());
    }

    #[test]
    fn trait_verify_integrity_tampered() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let r = make_receipt("mock", 0, Outcome::Complete);
        let hash = r.receipt_sha256.clone().unwrap();
        store.save_by_hash(&r).unwrap();

        // Tamper with the stored file
        let by_hash_dir = dir.path().join("by_hash");
        let path = by_hash_dir.join(format!("{hash}.json"));
        let mut val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        val["usage_raw"] = serde_json::json!({"tampered": true});
        std::fs::write(&path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

        assert!(!store.verify_integrity(&hash).unwrap());
    }

    #[test]
    fn trait_verify_integrity_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let fake_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let err = store.verify_integrity(fake_hash).unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
    }
}

// ===========================================================================
// 5. Receipt querying by various criteria
// ===========================================================================

mod querying {
    use super::*;

    #[test]
    fn query_receipts_by_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let r_complete = make_receipt("a", 0, Outcome::Complete);
        let r_failed = make_receipt("b", 10, Outcome::Failed);
        let r_partial = make_receipt("c", 20, Outcome::Partial);

        store.save(&r_complete).unwrap();
        store.save(&r_failed).unwrap();
        store.save(&r_partial).unwrap();

        let all_ids = store.list().unwrap();
        let mut failed_ids = Vec::new();
        for id in &all_ids {
            let loaded = store.load(*id).unwrap();
            if loaded.outcome == Outcome::Failed {
                failed_ids.push(loaded.meta.run_id);
            }
        }
        assert_eq!(failed_ids.len(), 1);
        assert_eq!(failed_ids[0], r_failed.meta.run_id);
    }

    #[test]
    fn query_receipts_by_backend() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        store
            .save(&make_receipt("alpha", 0, Outcome::Complete))
            .unwrap();
        store
            .save(&make_receipt("beta", 10, Outcome::Complete))
            .unwrap();
        store
            .save(&make_receipt("alpha", 20, Outcome::Partial))
            .unwrap();

        let all_ids = store.list().unwrap();
        let alpha_count = all_ids
            .iter()
            .filter(|id| store.load(**id).unwrap().backend.id == "alpha")
            .count();
        assert_eq!(alpha_count, 2);
    }

    #[test]
    fn query_receipts_by_time_range() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let r1 = make_receipt("m", 0, Outcome::Complete);
        let r2 = make_receipt("m", 100, Outcome::Complete);
        let r3 = make_receipt("m", 200, Outcome::Complete);
        store.save(&r1).unwrap();
        store.save(&r2).unwrap();
        store.save(&r3).unwrap();

        let cutoff = time_at(50);
        let all_ids = store.list().unwrap();
        let after_cutoff: Vec<_> = all_ids
            .iter()
            .map(|id| store.load(*id).unwrap())
            .filter(|r| r.meta.started_at > cutoff)
            .collect();
        assert_eq!(after_cutoff.len(), 2);
    }

    #[test]
    fn query_receipt_by_work_order_id() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let wo_id = Uuid::new_v4();

        let r = ReceiptBuilder::new("mock")
            .work_order_id(wo_id)
            .outcome(Outcome::Complete)
            .started_at(base_time())
            .finished_at(base_time() + Duration::seconds(1))
            .with_hash()
            .unwrap();
        store.save(&r).unwrap();
        store
            .save(&make_receipt("other", 10, Outcome::Complete))
            .unwrap();

        let all_ids = store.list().unwrap();
        let matching: Vec<_> = all_ids
            .iter()
            .map(|id| store.load(*id).unwrap())
            .filter(|r| r.meta.work_order_id == wo_id)
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].meta.run_id, r.meta.run_id);
    }

    #[test]
    fn list_hashes_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        for i in 0..5 {
            let r = make_receipt("m", i * 10, Outcome::Complete);
            store.save_by_hash(&r).unwrap();
        }

        let hashes = store.list_hashes().unwrap();
        let mut sorted = hashes.clone();
        sorted.sort();
        assert_eq!(hashes, sorted);
    }
}

// ===========================================================================
// 6. Receipt metadata and audit trail
// ===========================================================================

mod metadata_audit_trail {
    use super::*;

    #[test]
    fn receipt_preserves_contract_version() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn receipt_preserves_timestamps_through_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let start = base_time();
        let finish = start + Duration::seconds(5);
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(start)
            .finished_at(finish)
            .with_hash()
            .unwrap();
        let run_id = r.meta.run_id;
        store.save(&r).unwrap();

        let loaded = store.load(run_id).unwrap();
        assert_eq!(loaded.meta.started_at, start);
        assert_eq!(loaded.meta.finished_at, finish);
        assert_eq!(loaded.meta.duration_ms, 5000);
    }

    #[test]
    fn receipt_preserves_usage_normalized() {
        let usage = UsageNormalized {
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_tokens: Some(200),
            cache_write_tokens: Some(100),
            request_units: Some(42),
            estimated_cost_usd: Some(0.05),
        };
        let r = ReceiptBuilder::new("mock")
            .usage(usage)
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store.save(&r).unwrap();

        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(loaded.usage.input_tokens, Some(1000));
        assert_eq!(loaded.usage.output_tokens, Some(500));
        assert_eq!(loaded.usage.cache_read_tokens, Some(200));
        assert_eq!(loaded.usage.estimated_cost_usd, Some(0.05));
    }

    #[test]
    fn receipt_preserves_verification_report() {
        let vr = VerificationReport {
            git_diff: Some("diff --git a/foo b/foo\n+bar".into()),
            git_status: Some("M foo.rs".into()),
            harness_ok: true,
        };
        let r = ReceiptBuilder::new("mock")
            .verification(vr)
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store.save(&r).unwrap();

        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(
            loaded.verification.git_diff.as_deref(),
            Some("diff --git a/foo b/foo\n+bar")
        );
        assert!(loaded.verification.harness_ok);
    }

    #[test]
    fn receipt_preserves_artifacts() {
        let r = ReceiptBuilder::new("mock")
            .add_artifact(ArtifactRef {
                kind: "patch".into(),
                path: "output.patch".into(),
            })
            .add_artifact(ArtifactRef {
                kind: "log".into(),
                path: "run.log".into(),
            })
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store.save(&r).unwrap();

        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(loaded.artifacts.len(), 2);
        assert_eq!(loaded.artifacts[0].kind, "patch");
        assert_eq!(loaded.artifacts[1].path, "run.log");
    }

    #[test]
    fn receipt_preserves_trace_events() {
        let r = ReceiptBuilder::new("mock")
            .add_trace_event(AgentEvent {
                ts: base_time(),
                kind: AgentEventKind::RunStarted {
                    message: "starting".into(),
                },
                ext: None,
            })
            .add_trace_event(AgentEvent {
                ts: base_time() + Duration::seconds(1),
                kind: AgentEventKind::ToolCall {
                    tool_name: "read_file".into(),
                    tool_use_id: Some("tu_1".into()),
                    parent_tool_use_id: None,
                    input: serde_json::json!({"path": "foo.rs"}),
                },
                ext: None,
            })
            .add_trace_event(AgentEvent {
                ts: base_time() + Duration::seconds(2),
                kind: AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            })
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store.save(&r).unwrap();

        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(loaded.trace.len(), 3);
    }

    #[test]
    fn receipt_preserves_execution_mode() {
        let r = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Passthrough)
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store.save(&r).unwrap();

        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(loaded.mode, ExecutionMode::Passthrough);
    }
}

// ===========================================================================
// 7. Canonical JSON serialization for deterministic hashing
// ===========================================================================

mod canonical_json {
    use super::*;

    #[test]
    fn canonical_json_is_deterministic() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let j1 = canonicalize(&r).unwrap();
        let j2 = canonicalize(&r).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn canonical_json_is_compact_no_newlines() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let json = canonicalize(&r).unwrap();
        assert!(!json.contains('\n'));
        assert!(!json.contains("  "));
    }

    #[test]
    fn canonical_json_nullifies_receipt_sha256() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let json = canonicalize(&r).unwrap();
        assert!(json.contains("\"receipt_sha256\":null"));
    }

    #[test]
    fn canonical_json_same_with_or_without_hash() {
        let r_no_hash = make_unhashed_receipt("mock", 0);
        let mut r_with_hash = r_no_hash.clone();
        r_with_hash.receipt_sha256 = Some("abc123".into());

        let j1 = canonicalize(&r_no_hash).unwrap();
        let j2 = canonicalize(&r_with_hash).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn canonical_json_keys_are_sorted() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let json = canonicalize(&r).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();

        if let serde_json::Value::Object(map) = val {
            let keys: Vec<_> = map.keys().collect();
            let mut sorted_keys = keys.clone();
            sorted_keys.sort();
            assert_eq!(keys, sorted_keys, "top-level keys should be sorted");
        } else {
            panic!("expected JSON object");
        }
    }

    #[test]
    fn sha256_of_canonical_json_is_stable() {
        let t = base_time();
        let id = Uuid::nil();
        let r = ReceiptBuilder::new("mock")
            .run_id(id)
            .work_order_id(id)
            .outcome(Outcome::Complete)
            .started_at(t)
            .finished_at(t)
            .build();

        let json = canonicalize(&r).unwrap();
        let mut hasher1 = Sha256::new();
        hasher1.update(json.as_bytes());
        let hash1 = format!("{:x}", hasher1.finalize());

        let json2 = canonicalize(&r).unwrap();
        let mut hasher2 = Sha256::new();
        hasher2.update(json2.as_bytes());
        let hash2 = format!("{:x}", hasher2.finalize());

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn canonical_json_handles_unicode() {
        let r = ReceiptBuilder::new("バックエンド🚀")
            .backend_version("版本 1.0")
            .outcome(Outcome::Complete)
            .build();
        let json = canonicalize(&r).unwrap();
        assert!(json.contains("バックエンド🚀"));
        let hash = compute_hash(&r).unwrap();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn canonical_json_handles_empty_fields() {
        let r = ReceiptBuilder::new("").build();
        let json = canonicalize(&r).unwrap();
        assert!(!json.is_empty());
        let hash = compute_hash(&r).unwrap();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn canonical_json_handles_large_usage_raw() {
        let large_raw = serde_json::json!({
            "model": "gpt-4",
            "tokens": {
                "prompt": 50000,
                "completion": 25000,
                "total": 75000
            },
            "metadata": {
                "region": "us-east-1",
                "latency_ms": 1234,
                "cache_hit": false
            }
        });
        let r = ReceiptBuilder::new("mock")
            .usage_raw(large_raw)
            .outcome(Outcome::Complete)
            .build();
        let json = canonicalize(&r).unwrap();
        assert!(json.contains("\"prompt\":50000"));
    }
}

// ===========================================================================
// 8. BTreeMap usage for deterministic key ordering
// ===========================================================================

mod btreemap_determinism {
    use super::*;

    #[test]
    fn vendor_config_btreemap_produces_sorted_keys() {
        let mut vendor = BTreeMap::new();
        vendor.insert("zebra".to_string(), serde_json::json!("z"));
        vendor.insert("alpha".to_string(), serde_json::json!("a"));
        vendor.insert("middle".to_string(), serde_json::json!("m"));

        let json = serde_json::to_string(&vendor).unwrap();
        let alpha_pos = json.find("\"alpha\"").unwrap();
        let middle_pos = json.find("\"middle\"").unwrap();
        let zebra_pos = json.find("\"zebra\"").unwrap();
        assert!(alpha_pos < middle_pos);
        assert!(middle_pos < zebra_pos);
    }

    #[test]
    fn capability_manifest_btreemap_is_deterministic() {
        use abp_core::{Capability, CapabilityManifest, SupportLevel};

        let mut m1 = CapabilityManifest::new();
        m1.insert(Capability::ToolWrite, SupportLevel::Native);
        m1.insert(Capability::Streaming, SupportLevel::Emulated);
        m1.insert(Capability::ToolRead, SupportLevel::Native);

        let mut m2 = CapabilityManifest::new();
        // Insert in different order
        m2.insert(Capability::Streaming, SupportLevel::Emulated);
        m2.insert(Capability::ToolRead, SupportLevel::Native);
        m2.insert(Capability::ToolWrite, SupportLevel::Native);

        let j1 = serde_json::to_string(&m1).unwrap();
        let j2 = serde_json::to_string(&m2).unwrap();
        assert_eq!(
            j1, j2,
            "BTreeMap should produce identical JSON regardless of insertion order"
        );
    }

    #[test]
    fn receipt_with_capabilities_hashes_deterministically() {
        use abp_core::{Capability, CapabilityManifest, SupportLevel};

        let t = base_time();
        let id = Uuid::nil();

        let mut caps = CapabilityManifest::new();
        caps.insert(Capability::ToolWrite, SupportLevel::Native);
        caps.insert(Capability::Streaming, SupportLevel::Emulated);
        caps.insert(Capability::ToolRead, SupportLevel::Native);

        let r1 = ReceiptBuilder::new("mock")
            .run_id(id)
            .work_order_id(id)
            .capabilities(caps.clone())
            .started_at(t)
            .finished_at(t)
            .build();

        // Build again with caps inserted in different order
        let mut caps2 = CapabilityManifest::new();
        caps2.insert(Capability::Streaming, SupportLevel::Emulated);
        caps2.insert(Capability::ToolRead, SupportLevel::Native);
        caps2.insert(Capability::ToolWrite, SupportLevel::Native);

        let r2 = ReceiptBuilder::new("mock")
            .run_id(id)
            .work_order_id(id)
            .capabilities(caps2)
            .started_at(t)
            .finished_at(t)
            .build();

        let h1 = receipt_hash(&r1).unwrap();
        let h2 = receipt_hash(&r2).unwrap();
        assert_eq!(
            h1, h2,
            "Same capabilities in different order should hash identically"
        );
    }

    #[test]
    fn ext_btreemap_in_agent_event_is_deterministic() {
        let mut ext = BTreeMap::new();
        ext.insert("z_key".to_string(), serde_json::json!("z_val"));
        ext.insert("a_key".to_string(), serde_json::json!("a_val"));
        ext.insert("m_key".to_string(), serde_json::json!("m_val"));

        let event = AgentEvent {
            ts: base_time(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: Some(ext),
        };

        let json = serde_json::to_string(&event).unwrap();
        let a_pos = json.find("\"a_key\"").unwrap();
        let m_pos = json.find("\"m_key\"").unwrap();
        let z_pos = json.find("\"z_key\"").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }
}

// ===========================================================================
// Chain verification through the store
// ===========================================================================

mod chain_verification {
    use super::*;

    #[test]
    fn verify_chain_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        let result = store.verify_chain().unwrap();
        assert!(result.is_valid);
        assert_eq!(result.valid_count, 0);
        assert!(result.invalid_hashes.is_empty());
        assert!(result.gaps.is_empty());
    }

    #[test]
    fn verify_chain_single_valid_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store
            .save(&make_receipt("mock", 0, Outcome::Complete))
            .unwrap();

        let result = store.verify_chain().unwrap();
        assert!(result.is_valid);
        assert_eq!(result.valid_count, 1);
    }

    #[test]
    fn verify_chain_multiple_valid_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        for i in 0..3 {
            store
                .save(&make_receipt("m", i * 100, Outcome::Complete))
                .unwrap();
        }

        let result = store.verify_chain().unwrap();
        assert!(result.is_valid);
        assert_eq!(result.valid_count, 3);
        assert_eq!(result.gaps.len(), 2); // gaps between consecutive receipts
    }

    #[test]
    fn verify_chain_detects_tampered_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let r = make_receipt("mock", 0, Outcome::Complete);
        let run_id = r.meta.run_id;
        let path = store.save(&r).unwrap();

        // Tamper with the stored file
        let mut val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        val["outcome"] = serde_json::json!("failed");
        std::fs::write(&path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

        let result = store.verify_chain().unwrap();
        assert!(!result.is_valid);
        assert!(result.invalid_hashes.contains(&run_id));
    }

    #[test]
    fn verify_chain_records_time_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        let r1 = make_receipt("m", 0, Outcome::Complete);
        let r2 = make_receipt("m", 1000, Outcome::Complete);
        store.save(&r1).unwrap();
        store.save(&r2).unwrap();

        let result = store.verify_chain().unwrap();
        assert!(result.is_valid);
        assert_eq!(result.gaps.len(), 1);
        // Gap should be between r1's finished_at and r2's started_at
        let (gap_start, gap_end) = &result.gaps[0];
        assert!(gap_end > gap_start);
    }
}

// ===========================================================================
// Receipt chain (in-memory)
// ===========================================================================

mod receipt_chain_tests {
    use super::*;

    #[test]
    fn chain_empty_verify_returns_error() {
        let chain = ReceiptChain::new();
        assert_eq!(chain.verify(), Err(ChainError::EmptyChain));
    }

    #[test]
    fn chain_push_and_verify() {
        let mut chain = ReceiptChain::new();
        chain
            .push(make_receipt("mock", 0, Outcome::Complete))
            .unwrap();
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn chain_rejects_duplicate_run_id() {
        let mut chain = ReceiptChain::new();
        let id = Uuid::new_v4();
        let r1 = make_receipt_with_id(id, 0);
        let r2 = make_receipt_with_id(id, 10);
        chain.push(r1).unwrap();
        assert_eq!(chain.push(r2), Err(ChainError::DuplicateId { id }));
    }

    #[test]
    fn chain_rejects_out_of_order() {
        let mut chain = ReceiptChain::new();
        chain
            .push(make_receipt("m", 100, Outcome::Complete))
            .unwrap();
        let r_early = make_receipt("m", 0, Outcome::Complete);
        assert!(matches!(
            chain.push(r_early),
            Err(ChainError::BrokenLink { .. })
        ));
    }

    #[test]
    fn chain_rejects_tampered_hash() {
        let mut chain = ReceiptChain::new();
        let mut r = make_receipt("mock", 0, Outcome::Complete);
        r.outcome = Outcome::Failed; // tamper without rehashing
        assert!(matches!(
            chain.push(r),
            Err(ChainError::HashMismatch { .. })
        ));
    }

    #[test]
    fn chain_multiple_receipts_in_order() {
        let mut chain = ReceiptChain::new();
        for i in 0..5 {
            chain
                .push(make_receipt("m", i * 10, Outcome::Complete))
                .unwrap();
        }
        assert_eq!(chain.len(), 5);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn chain_iter_preserves_insertion_order() {
        let mut chain = ReceiptChain::new();
        let backends = ["alpha", "beta", "gamma"];
        for (i, b) in backends.iter().enumerate() {
            chain
                .push(make_receipt(b, (i as i64) * 10, Outcome::Complete))
                .unwrap();
        }
        let collected: Vec<_> = chain.iter().map(|r| r.backend.id.as_str()).collect();
        assert_eq!(collected, backends);
    }

    #[test]
    fn chain_latest_is_last() {
        let mut chain = ReceiptChain::new();
        let last = make_receipt("last-backend", 100, Outcome::Failed);
        let last_id = last.meta.run_id;
        chain
            .push(make_receipt("first", 0, Outcome::Complete))
            .unwrap();
        chain.push(last).unwrap();
        assert_eq!(chain.latest().unwrap().meta.run_id, last_id);
    }

    #[test]
    fn chain_accepts_unhashed_receipt() {
        let mut chain = ReceiptChain::new();
        let r = make_unhashed_receipt("mock", 0);
        assert!(r.receipt_sha256.is_none());
        chain.push(r).unwrap();
        assert_eq!(chain.len(), 1);
    }
}

// ===========================================================================
// Receipt diffing
// ===========================================================================

mod receipt_diffing {
    use super::*;

    #[test]
    fn diff_identical_receipts_is_empty() {
        let r = make_receipt("mock", 0, Outcome::Complete);
        let d = diff_receipts(&r, &r.clone());
        assert!(d.is_empty());
    }

    #[test]
    fn diff_detects_outcome_change() {
        let r1 = make_receipt("mock", 0, Outcome::Complete);
        let mut r2 = r1.clone();
        r2.outcome = Outcome::Failed;
        let d = diff_receipts(&r1, &r2);
        assert!(d.changes.iter().any(|c| c.field == "outcome"));
    }

    #[test]
    fn diff_detects_backend_change() {
        let r1 = ReceiptBuilder::new("old").build();
        let mut r2 = r1.clone();
        r2.backend.id = "new".into();
        let d = diff_receipts(&r1, &r2);
        assert!(d.changes.iter().any(|c| c.field == "backend.id"));
    }

    #[test]
    fn diff_detects_mode_change() {
        let r1 = ReceiptBuilder::new("mock")
            .mode(ExecutionMode::Mapped)
            .build();
        let mut r2 = r1.clone();
        r2.mode = ExecutionMode::Passthrough;
        let d = diff_receipts(&r1, &r2);
        assert!(d.changes.iter().any(|c| c.field == "mode"));
    }
}

// ===========================================================================
// Edge cases and stress tests
// ===========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn store_many_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        for i in 0..50 {
            store
                .save(&make_receipt("m", i * 5, Outcome::Complete))
                .unwrap();
        }

        let ids = store.list().unwrap();
        assert_eq!(ids.len(), 50);
    }

    #[test]
    fn store_many_hash_keyed_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());

        for i in 0..20 {
            store
                .save_by_hash(&make_receipt("m", i * 5, Outcome::Complete))
                .unwrap();
        }

        let hashes = store.list_hashes().unwrap();
        assert_eq!(hashes.len(), 20);

        // All hashes should be unique
        let unique: std::collections::HashSet<_> = hashes.iter().collect();
        assert_eq!(unique.len(), 20);
    }

    #[test]
    fn receipt_with_large_trace_roundtrips() {
        let mut builder = ReceiptBuilder::new("mock").outcome(Outcome::Complete);
        for i in 0..300 {
            builder = builder.add_trace_event(AgentEvent {
                ts: base_time() + Duration::milliseconds(i),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("token_{i}"),
                },
                ext: None,
            });
        }
        let r = builder.with_hash().unwrap();
        assert!(verify_hash(&r));

        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store.save(&r).unwrap();

        let loaded = store.load(r.meta.run_id).unwrap();
        assert_eq!(loaded.trace.len(), 300);
        assert!(verify_hash(&loaded));
    }

    #[test]
    fn receipt_with_special_chars_in_backend_id() {
        let r = ReceiptBuilder::new("sidecar:node/v2.0@latest")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));

        let json = canonicalize(&r).unwrap();
        assert!(json.contains("sidecar:node/v2.0@latest"));
    }

    #[test]
    fn receipt_serde_roundtrip_preserves_all_fields() {
        let r = ReceiptBuilder::new("mock")
            .outcome(Outcome::Partial)
            .backend_version("3.0.1")
            .adapter_version("0.2.0")
            .mode(ExecutionMode::Passthrough)
            .usage_raw(serde_json::json!({"model": "gpt-4", "tokens": 1234}))
            .usage(UsageNormalized {
                input_tokens: Some(800),
                output_tokens: Some(400),
                ..Default::default()
            })
            .verification(VerificationReport {
                git_diff: Some("diff".into()),
                git_status: Some("M file.rs".into()),
                harness_ok: true,
            })
            .add_artifact(ArtifactRef {
                kind: "patch".into(),
                path: "result.patch".into(),
            })
            .with_hash()
            .unwrap();

        let json = serde_json::to_string(&r).unwrap();
        let loaded: Receipt = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.backend.id, "mock");
        assert_eq!(loaded.outcome, Outcome::Partial);
        assert_eq!(loaded.backend.backend_version.as_deref(), Some("3.0.1"));
        assert_eq!(loaded.backend.adapter_version.as_deref(), Some("0.2.0"));
        assert_eq!(loaded.mode, ExecutionMode::Passthrough);
        assert_eq!(loaded.usage.input_tokens, Some(800));
        assert_eq!(loaded.artifacts.len(), 1);
        assert!(loaded.verification.harness_ok);
        assert_eq!(loaded.receipt_sha256, r.receipt_sha256);
    }

    #[test]
    fn store_error_display_messages() {
        let err = StoreError::MissingHash;
        assert!(err.to_string().contains("no hash"));

        let err = StoreError::NotFound("abc".into());
        assert!(err.to_string().contains("abc"));
    }

    #[test]
    fn receipt_with_nil_uuids() {
        let r = ReceiptBuilder::new("mock")
            .run_id(Uuid::nil())
            .work_order_id(Uuid::nil())
            .started_at(base_time())
            .finished_at(base_time())
            .with_hash()
            .unwrap();
        assert!(verify_hash(&r));

        let dir = tempfile::tempdir().unwrap();
        let store = ReceiptStore::new(dir.path());
        store.save(&r).unwrap();
        let loaded = store.load(Uuid::nil()).unwrap();
        assert_eq!(loaded.meta.run_id, Uuid::nil());
    }
}
