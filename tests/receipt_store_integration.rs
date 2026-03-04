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

//! Deep integration tests for the receipt store and verification modules.

use abp_receipt::store::{InMemoryReceiptStore, ReceiptFilter, ReceiptStore};
use abp_receipt::verify::{ReceiptAuditor, verify_receipt};
use abp_receipt::{CONTRACT_VERSION, Outcome, Receipt, ReceiptBuilder, compute_hash, verify_hash};
use chrono::{Duration, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a valid, hashed receipt for the given backend with sensible defaults.
fn make_receipt(backend: &str) -> Receipt {
    ReceiptBuilder::new(backend)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

/// Build a receipt with a specific run_id.
fn make_receipt_with_id(backend: &str, id: Uuid) -> Receipt {
    ReceiptBuilder::new(backend)
        .run_id(id)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

/// Build a receipt with a specific outcome (hashed).
fn make_receipt_with_outcome(backend: &str, outcome: Outcome) -> Receipt {
    let builder = ReceiptBuilder::new(backend).outcome(outcome.clone());
    // Use the error helper for Failed to ensure trace consistency
    if outcome == Outcome::Failed {
        ReceiptBuilder::new(backend)
            .error("test failure")
            .with_hash()
            .unwrap()
    } else {
        builder.with_hash().unwrap()
    }
}

/// Build a receipt with explicit timestamps.
fn make_receipt_at(backend: &str, started: chrono::DateTime<Utc>, dur_ms: i64) -> Receipt {
    let finished = started + Duration::milliseconds(dur_ms);
    ReceiptBuilder::new(backend)
        .started_at(started)
        .finished_at(finished)
        .outcome(Outcome::Complete)
        .with_hash()
        .unwrap()
}

// ===========================================================================
// Module: store_crud
// ===========================================================================

mod store_crud {
    use super::*;

    #[test]
    fn store_and_retrieve_by_run_id() {
        let mut store = InMemoryReceiptStore::new();
        let receipt = make_receipt("mock");
        let run_id = receipt.meta.run_id;
        store.store(receipt).unwrap();

        let fetched = store.get(run_id).unwrap().unwrap();
        assert_eq!(fetched.meta.run_id, run_id);
        assert_eq!(fetched.backend.id, "mock");
    }

    #[test]
    fn store_multiple_and_retrieve_each() {
        let mut store = InMemoryReceiptStore::new();
        let ids: Vec<Uuid> = (0..5)
            .map(|i| {
                let r = make_receipt(&format!("backend-{i}"));
                let id = r.meta.run_id;
                store.store(r).unwrap();
                id
            })
            .collect();

        for (i, id) in ids.iter().enumerate() {
            let r = store.get(*id).unwrap().unwrap();
            assert_eq!(r.backend.id, format!("backend-{i}"));
        }
    }

    #[test]
    fn retrieve_nonexistent_returns_none() {
        let store = InMemoryReceiptStore::new();
        let result = store.get(Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn list_all_receipts() {
        let mut store = InMemoryReceiptStore::new();
        for _ in 0..4 {
            store.store(make_receipt("mock")).unwrap();
        }
        let all = store.list(&ReceiptFilter::default()).unwrap();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn count_receipts() {
        let mut store = InMemoryReceiptStore::new();
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());

        store.store(make_receipt("a")).unwrap();
        store.store(make_receipt("b")).unwrap();
        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());
    }

    #[test]
    fn duplicate_run_id_rejected() {
        let mut store = InMemoryReceiptStore::new();
        let id = Uuid::new_v4();
        let r1 = make_receipt_with_id("mock", id);
        let r2 = make_receipt_with_id("mock", id);

        store.store(r1).unwrap();
        let err = store.store(r2).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn store_receipt_with_hash_preserved() {
        let mut store = InMemoryReceiptStore::new();
        let receipt = make_receipt("mock");
        let original_hash = receipt.receipt_sha256.clone();
        assert!(original_hash.is_some());

        let run_id = receipt.meta.run_id;
        store.store(receipt).unwrap();

        let fetched = store.get(run_id).unwrap().unwrap();
        assert_eq!(fetched.receipt_sha256, original_hash);
    }

    #[test]
    fn store_receipt_without_hash() {
        let mut store = InMemoryReceiptStore::new();
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        assert!(receipt.receipt_sha256.is_none());

        let run_id = receipt.meta.run_id;
        store.store(receipt).unwrap();

        let fetched = store.get(run_id).unwrap().unwrap();
        assert!(fetched.receipt_sha256.is_none());
    }

    #[test]
    fn store_returns_correct_id() {
        let mut store = InMemoryReceiptStore::new();
        let receipt = make_receipt("mock");
        let expected_id = receipt.meta.run_id;
        let returned_id = store.store(receipt).unwrap();
        assert_eq!(returned_id, expected_id);
    }

    #[test]
    fn list_summaries_contain_correct_fields() {
        let mut store = InMemoryReceiptStore::new();
        let receipt = make_receipt_with_outcome("mock", Outcome::Complete);
        let run_id = receipt.meta.run_id;
        let started = receipt.meta.started_at;
        let finished = receipt.meta.finished_at;
        store.store(receipt).unwrap();

        let summaries = store.list(&ReceiptFilter::default()).unwrap();
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(s.id, run_id);
        assert_eq!(s.backend_id, "mock");
        assert_eq!(s.outcome, Outcome::Complete);
        assert_eq!(s.started_at, started);
        assert_eq!(s.finished_at, finished);
    }
}

// ===========================================================================
// Module: store_queries
// ===========================================================================

mod store_queries {
    use super::*;

    fn populated_store() -> InMemoryReceiptStore {
        let mut store = InMemoryReceiptStore::new();
        let base = Utc::now() - Duration::hours(10);

        // 3 from "alpha", 2 from "beta"
        for i in 0..3 {
            store
                .store(make_receipt_at("alpha", base + Duration::hours(i), 100))
                .unwrap();
        }
        // 1 failed from beta
        store
            .store(make_receipt_with_outcome("beta", Outcome::Failed))
            .unwrap();
        // 1 complete from beta
        store
            .store(make_receipt_with_outcome("beta", Outcome::Complete))
            .unwrap();
        store
    }

    #[test]
    fn query_by_backend_name() {
        let store = populated_store();
        let filter = ReceiptFilter {
            backend_id: Some("alpha".into()),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|s| s.backend_id == "alpha"));
    }

    #[test]
    fn query_by_outcome_success() {
        let store = populated_store();
        let filter = ReceiptFilter {
            outcome: Some(Outcome::Complete),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        // 3 alpha complete + 1 beta complete = 4
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn query_by_outcome_failure() {
        let store = populated_store();
        let filter = ReceiptFilter {
            outcome: Some(Outcome::Failed),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, Outcome::Failed);
    }

    #[test]
    fn query_by_date_range_after() {
        let mut store = InMemoryReceiptStore::new();
        let old = Utc::now() - Duration::days(5);
        let recent = Utc::now() - Duration::minutes(30);

        store.store(make_receipt_at("a", old, 100)).unwrap();
        store.store(make_receipt_at("b", recent, 100)).unwrap();

        let filter = ReceiptFilter {
            after: Some(Utc::now() - Duration::hours(1)),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].backend_id, "b");
    }

    #[test]
    fn query_by_date_range_before() {
        let mut store = InMemoryReceiptStore::new();
        let old = Utc::now() - Duration::days(5);
        let recent = Utc::now() - Duration::minutes(30);

        store.store(make_receipt_at("a", old, 100)).unwrap();
        store.store(make_receipt_at("b", recent, 100)).unwrap();

        let filter = ReceiptFilter {
            before: Some(Utc::now() - Duration::days(1)),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].backend_id, "a");
    }

    #[test]
    fn query_by_contract_version() {
        // All receipts from the builder use CONTRACT_VERSION, so listing all
        // should return only receipts with that version.
        let store = populated_store();
        let all = store.list(&ReceiptFilter::default()).unwrap();
        assert!(!all.is_empty());

        // Verify via get that all have the correct version.
        for summary in &all {
            let receipt = store.get(summary.id).unwrap().unwrap();
            assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        }
    }

    #[test]
    fn combined_query_backend_and_outcome() {
        let store = populated_store();
        let filter = ReceiptFilter {
            backend_id: Some("beta".into()),
            outcome: Some(Outcome::Failed),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].backend_id, "beta");
        assert_eq!(results[0].outcome, Outcome::Failed);
    }

    #[test]
    fn combined_query_backend_and_date() {
        let mut store = InMemoryReceiptStore::new();
        let old = Utc::now() - Duration::days(5);
        let recent = Utc::now() - Duration::minutes(30);

        store.store(make_receipt_at("alpha", old, 100)).unwrap();
        store.store(make_receipt_at("alpha", recent, 100)).unwrap();
        store.store(make_receipt_at("beta", recent, 100)).unwrap();

        let filter = ReceiptFilter {
            backend_id: Some("alpha".into()),
            after: Some(Utc::now() - Duration::hours(1)),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].backend_id, "alpha");
    }

    #[test]
    fn empty_store_returns_empty_results() {
        let store = InMemoryReceiptStore::new();
        let results = store.list(&ReceiptFilter::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn query_nonmatching_backend_returns_empty() {
        let store = populated_store();
        let filter = ReceiptFilter {
            backend_id: Some("nonexistent".into()),
            ..Default::default()
        };
        let results = store.list(&filter).unwrap();
        assert!(results.is_empty());
    }
}

// ===========================================================================
// Module: verification_pipeline
// ===========================================================================

mod verification_pipeline {
    use super::*;
    use abp_receipt::AgentEvent;
    use abp_receipt::AgentEventKind;

    #[test]
    fn verify_valid_receipt_passes() {
        let receipt = make_receipt("mock");
        let result = verify_receipt(&receipt);
        assert!(result.is_verified());
        assert!(result.hash_valid);
        assert!(result.contract_valid);
        assert!(result.timestamps_valid);
        assert!(result.outcome_consistent);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn verify_receipt_with_tampered_hash_fails() {
        let mut receipt = make_receipt("mock");
        receipt.receipt_sha256 = Some("deadbeef".into());

        let result = verify_receipt(&receipt);
        assert!(!result.is_verified());
        assert!(!result.hash_valid);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.contains("hash") || i.contains("Hash"))
        );
    }

    #[test]
    fn verify_receipt_with_wrong_contract_version_flags() {
        let mut receipt = make_receipt("mock");
        receipt.meta.contract_version = "abp/v999".into();
        // Recompute hash after modifying version
        receipt.receipt_sha256 = Some(compute_hash(&receipt).unwrap());

        let result = verify_receipt(&receipt);
        assert!(!result.is_verified());
        assert!(!result.contract_valid);
        assert!(result.issues.iter().any(|i| i.contains("contract version")));
    }

    #[test]
    fn verify_receipt_with_inconsistent_outcome_flags() {
        // Complete outcome but trace contains an error event → inconsistent
        let now = Utc::now();
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .add_event(AgentEvent {
                ts: now,
                kind: AgentEventKind::Error {
                    message: "something broke".into(),
                    error_code: None,
                },
                ext: None,
            })
            .with_hash()
            .unwrap();

        let result = verify_receipt(&receipt);
        assert!(!result.is_verified());
        assert!(!result.outcome_consistent);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.contains("Complete") || i.contains("error"))
        );
    }

    #[test]
    fn verify_receipt_without_hash_passes() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build();
        assert!(receipt.receipt_sha256.is_none());

        let result = verify_receipt(&receipt);
        assert!(result.hash_valid); // No hash → valid
    }

    #[test]
    fn batch_verify_all_valid_clean_audit() {
        let auditor = ReceiptAuditor::new();
        let receipts: Vec<Receipt> = (0..5).map(|i| make_receipt(&format!("b-{i}"))).collect();

        let report = auditor.audit_batch(&receipts);
        assert!(report.is_clean());
        assert_eq!(report.total, 5);
        assert_eq!(report.valid, 5);
        assert_eq!(report.invalid, 0);
        assert!(report.duplicate_hashes.is_empty());
    }

    #[test]
    fn batch_verify_one_invalid_catches_it() {
        let auditor = ReceiptAuditor::new();
        let mut receipts: Vec<Receipt> = (0..3).map(|i| make_receipt(&format!("b-{i}"))).collect();

        // Tamper with the second receipt's hash
        receipts[1].receipt_sha256 = Some("tampered".into());

        let report = auditor.audit_batch(&receipts);
        assert!(!report.is_clean());
        assert_eq!(report.total, 3);
        assert_eq!(report.valid, 2);
        assert_eq!(report.invalid, 1);
    }

    #[test]
    fn audit_detects_duplicate_hashes() {
        let auditor = ReceiptAuditor::new();

        // Create two receipts with the same hash (artificially)
        let mut r1 = make_receipt("mock-a");
        let mut r2 = make_receipt("mock-b");
        let shared_hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";

        // Both invalid hashes but identical → duplicate hash detection
        r1.receipt_sha256 = Some(shared_hash.into());
        r2.receipt_sha256 = Some(shared_hash.into());

        let report = auditor.audit_batch(&[r1, r2]);
        assert!(!report.duplicate_hashes.is_empty());
        assert!(report.duplicate_hashes.contains(&shared_hash.to_string()));
    }

    #[test]
    fn audit_detects_duplicate_run_ids() {
        let auditor = ReceiptAuditor::new();

        let id = Uuid::new_v4();
        let r1 = make_receipt_with_id("mock-a", id);
        let r2 = make_receipt_with_id("mock-b", id);

        let report = auditor.audit_batch(&[r1, r2]);
        assert!(!report.is_clean());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.description.contains("duplicate run_id"))
        );
    }

    #[test]
    fn verify_hash_helper_function() {
        let receipt = make_receipt("mock");
        assert!(verify_hash(&receipt));

        let mut tampered = make_receipt("mock");
        tampered.receipt_sha256 = Some("wrong".into());
        assert!(!verify_hash(&tampered));
    }
}

// ===========================================================================
// Module: receipt_lifecycle
// ===========================================================================

mod receipt_lifecycle {
    use super::*;

    #[test]
    fn full_lifecycle_create_hash_store_retrieve_verify() {
        // 1. Create
        let receipt = ReceiptBuilder::new("lifecycle-backend")
            .outcome(Outcome::Complete)
            .backend_version("1.0.0")
            .build();
        assert!(receipt.receipt_sha256.is_none());

        // 2. Hash
        let hash = compute_hash(&receipt).unwrap();
        let mut receipt = receipt;
        receipt.receipt_sha256 = Some(hash.clone());
        assert_eq!(receipt.receipt_sha256.as_deref(), Some(hash.as_str()));

        // 3. Store
        let mut store = InMemoryReceiptStore::new();
        let run_id = receipt.meta.run_id;
        store.store(receipt).unwrap();

        // 4. Retrieve
        let fetched = store.get(run_id).unwrap().unwrap();
        assert_eq!(fetched.backend.id, "lifecycle-backend");

        // 5. Verify
        let result = verify_receipt(fetched);
        assert!(result.is_verified());
    }

    #[test]
    fn serialization_roundtrip_preserves_hash() {
        let receipt = make_receipt("roundtrip");
        let original_hash = receipt.receipt_sha256.clone().unwrap();

        // Serialize → deserialize
        let json = serde_json::to_string(&receipt).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json).unwrap();

        assert_eq!(
            deserialized.receipt_sha256.as_deref(),
            Some(original_hash.as_str())
        );
        assert!(verify_hash(&deserialized));
    }

    #[test]
    fn receipt_from_builder_store_verify() {
        let receipt = ReceiptBuilder::new("mock-backend")
            .outcome(Outcome::Complete)
            .usage_tokens(100, 50)
            .backend_version("2.0.0")
            .with_hash()
            .unwrap();

        let mut store = InMemoryReceiptStore::new();
        let run_id = receipt.meta.run_id;
        store.store(receipt).unwrap();

        let fetched = store.get(run_id).unwrap().unwrap();
        let result = verify_receipt(fetched);
        assert!(result.is_verified());
        assert_eq!(fetched.usage.input_tokens, Some(100));
        assert_eq!(fetched.usage.output_tokens, Some(50));
    }

    #[test]
    fn multiple_backends_store_all_audit_all() {
        let backends = ["openai", "anthropic", "gemini", "copilot"];
        let mut store = InMemoryReceiptStore::new();
        let mut all_receipts = Vec::new();

        for backend in &backends {
            let receipt = make_receipt(backend);
            all_receipts.push(receipt.clone());
            store.store(receipt).unwrap();
        }

        assert_eq!(store.len(), 4);

        // Verify all individually from store
        for receipt in all_receipts.iter() {
            let fetched = store.get(receipt.meta.run_id).unwrap().unwrap();
            assert!(verify_receipt(fetched).is_verified());
        }

        // Batch audit
        let auditor = ReceiptAuditor::new();
        let report = auditor.audit_batch(&all_receipts);
        assert!(report.is_clean());
        assert_eq!(report.total, 4);
    }

    #[test]
    fn lifecycle_with_failed_receipt() {
        let receipt = ReceiptBuilder::new("failing-backend")
            .error("disk full")
            .with_hash()
            .unwrap();

        assert_eq!(receipt.outcome, Outcome::Failed);

        let mut store = InMemoryReceiptStore::new();
        let run_id = receipt.meta.run_id;
        store.store(receipt).unwrap();

        let fetched = store.get(run_id).unwrap().unwrap();
        let result = verify_receipt(fetched);
        assert!(result.is_verified());
        assert_eq!(fetched.outcome, Outcome::Failed);
    }

    #[test]
    fn lifecycle_store_query_by_outcome() {
        let mut store = InMemoryReceiptStore::new();

        store
            .store(make_receipt_with_outcome("a", Outcome::Complete))
            .unwrap();
        store
            .store(make_receipt_with_outcome("b", Outcome::Failed))
            .unwrap();
        store
            .store(make_receipt_with_outcome("c", Outcome::Complete))
            .unwrap();

        let successes = store
            .list(&ReceiptFilter {
                outcome: Some(Outcome::Complete),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(successes.len(), 2);

        let failures = store
            .list(&ReceiptFilter {
                outcome: Some(Outcome::Failed),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn audit_report_display_clean() {
        let auditor = ReceiptAuditor::new();
        let receipts = vec![make_receipt("mock")];
        let report = auditor.audit_batch(&receipts);
        let display = format!("{report}");
        assert!(display.contains("total: 1"));
        assert!(display.contains("valid: 1"));
    }

    #[test]
    fn verification_result_display_verified() {
        let receipt = make_receipt("mock");
        let result = verify_receipt(&receipt);
        let display = format!("{result}");
        assert!(display.contains("verified"));
    }
}
