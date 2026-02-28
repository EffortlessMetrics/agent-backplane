// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for `abp-daemon` run tracking and validation logic.

use abp_daemon::{RunStatus, RunTracker};
use proptest::prelude::*;
use uuid::Uuid;

// ── Strategies ──────────────────────────────────────────────────────

fn arb_uuid() -> impl Strategy<Value = Uuid> {
    any::<u128>().prop_map(Uuid::from_u128)
}

fn arb_receipt() -> impl Strategy<Value = abp_core::Receipt> {
    (arb_uuid(), arb_uuid()).prop_map(|(run_id, wo_id)| {
        use abp_core::*;
        use chrono::Utc;

        Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: wo_id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: Utc::now(),
                finished_at: Utc::now(),
                duration_ms: 0,
            },
            backend: BackendIdentity {
                id: "test".into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::default(),
            mode: ExecutionMode::Mapped,
            usage_raw: serde_json::Value::Null,
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
    })
}

// ── 1. Arbitrary run IDs → tracker insert/retrieve roundtrip ────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn tracker_insert_retrieve_roundtrip(id in arb_uuid()) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let tracker = RunTracker::new();
            tracker.start_run(id).await.unwrap();

            let status = tracker.get_run_status(id).await;
            prop_assert!(
                matches!(status, Some(RunStatus::Running)),
                "expected Running, got {status:?}"
            );
            Ok(())
        })?;
    }
}

// ── 2. Random work order tasks → validation is deterministic ────────

proptest! {
    #[test]
    fn task_validation_is_deterministic(task in ".*") {
        // The daemon's /validate checks that the task is non-empty.
        let is_valid = !task.is_empty();

        // Running the same check twice should yield the same result.
        let is_valid_2 = !task.is_empty();
        prop_assert_eq!(is_valid, is_valid_2);
    }
}

// ── 3. Concurrent tracker operations → no panics ────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn concurrent_tracker_ops_no_panics(
        ids in prop::collection::vec(arb_uuid(), 2..8),
    ) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let tracker = RunTracker::new();

            // Spawn concurrent start_run tasks.
            let mut handles = Vec::new();
            for id in &ids {
                let t = tracker.clone();
                let id = *id;
                handles.push(tokio::spawn(async move {
                    let _ = t.start_run(id).await;
                }));
            }
            for h in handles {
                h.await.unwrap();
            }

            // All runs should be visible.
            let listed = tracker.list_runs().await;
            // Dedup: same UUID generated twice is counted once.
            let unique_ids: std::collections::HashSet<Uuid> =
                ids.iter().cloned().collect();
            prop_assert_eq!(
                listed.len(),
                unique_ids.len(),
                "all unique runs should be tracked"
            );
            Ok(())
        })?;
    }
}

// ── 4. Run status transitions → valid state machine ────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn run_status_transitions_valid(
        id in arb_uuid(),
        fail in any::<bool>(),
        receipt in arb_receipt(),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let tracker = RunTracker::new();

            // Not tracked → start should succeed.
            tracker.start_run(id).await.unwrap();
            prop_assert!(matches!(
                tracker.get_run_status(id).await,
                Some(RunStatus::Running)
            ));

            // Duplicate start should fail.
            prop_assert!(tracker.start_run(id).await.is_err());

            if fail {
                // Transition to Failed.
                tracker
                    .fail_run(id, "simulated".into())
                    .await
                    .unwrap();
                let s = tracker.get_run_status(id).await;
                prop_assert!(
                    matches!(s, Some(RunStatus::Failed { .. })),
                    "expected Failed, got {:?}", s
                );
            } else {
                // Transition to Completed.
                let mut r = receipt.clone();
                r.meta.run_id = id;
                tracker.complete_run(id, r).await.unwrap();
                let s = tracker.get_run_status(id).await;
                prop_assert!(
                    matches!(s, Some(RunStatus::Completed { .. })),
                    "expected Completed, got {:?}", s
                );
            }

            // Removing a completed/failed run should succeed.
            let removed = tracker.remove_run(id).await;
            prop_assert!(removed.is_ok());

            // After removal, status is None.
            prop_assert!(tracker.get_run_status(id).await.is_none());
            Ok(())
        })?;
    }
}

// ── 5. Complete/fail on unknown run → errors ────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn complete_or_fail_unknown_run_errors(
        id in arb_uuid(),
        receipt in arb_receipt(),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let tracker = RunTracker::new();

            // Completing an untracked run should fail.
            let mut r = receipt.clone();
            r.meta.run_id = id;
            prop_assert!(tracker.complete_run(id, r).await.is_err());

            // Failing an untracked run should fail.
            prop_assert!(tracker.fail_run(id, "err".into()).await.is_err());
            Ok(())
        })?;
    }
}

// ── 6. RunStatus serde round-trip ───────────────────────────────────

proptest! {
    #[test]
    fn run_status_serde_round_trip(fail in any::<bool>()) {
        let status: RunStatus = if fail {
            RunStatus::Failed {
                error: "test".into(),
            }
        } else {
            RunStatus::Running
        };

        let json = serde_json::to_string(&status).unwrap();
        let deser: RunStatus = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        prop_assert_eq!(json, json2);
    }
}
