// SPDX-License-Identifier: MIT OR Apache-2.0
//! State-machine tests for the daemon run lifecycle.

use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder};
use abp_daemon::{RunStatus, RunTracker};
use chrono::Utc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_receipt(backend: &str, outcome: Outcome) -> abp_core::Receipt {
    ReceiptBuilder::new(backend).outcome(outcome).build()
}

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

// ---------------------------------------------------------------------------
// 1. Run lifecycle: Running → Completed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_running_to_completed() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    assert!(matches!(
        tracker.get_run_status(id).await.unwrap(),
        RunStatus::Running
    ));

    let receipt = make_receipt("mock", Outcome::Complete);
    tracker.complete_run(id, receipt).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert_eq!(receipt.backend.id, "mock");
            assert!(matches!(receipt.outcome, Outcome::Complete));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 2. Run failure: Running → Failed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_running_to_failed() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    tracker
        .fail_run(id, "timeout exceeded".into())
        .await
        .unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Failed { error } => assert_eq!(error, "timeout exceeded"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 3. Run cancellation: simulate cancel by failing with cancel message
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lifecycle_running_to_cancelled() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    tracker
        .fail_run(id, "cancelled by user".into())
        .await
        .unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Failed { error } => assert!(error.contains("cancelled")),
        other => panic!("expected Failed (cancelled), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. Invalid transitions: Completed → start rejected (duplicate ID)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_transition_completed_to_running_rejected() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    let receipt = make_receipt("mock", Outcome::Complete);
    tracker.complete_run(id, receipt).await.unwrap();

    // Attempting to start_run on the same ID must fail.
    let err = tracker.start_run(id).await.unwrap_err();
    assert!(
        err.to_string().contains("already tracked"),
        "expected 'already tracked', got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 5. Concurrent state changes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_state_changes() {
    let tracker = RunTracker::new();
    let ids: Vec<Uuid> = (0..20).map(|_| Uuid::new_v4()).collect();

    // Start all runs concurrently.
    let mut handles = Vec::new();
    for &id in &ids {
        let t = tracker.clone();
        handles.push(tokio::spawn(async move { t.start_run(id).await }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }

    // Complete half, fail the other half concurrently.
    let mut handles = Vec::new();
    for (i, &id) in ids.iter().enumerate() {
        let t = tracker.clone();
        if i % 2 == 0 {
            let receipt = make_receipt("mock", Outcome::Complete);
            handles.push(tokio::spawn(
                async move { t.complete_run(id, receipt).await },
            ));
        } else {
            handles.push(tokio::spawn(async move {
                t.fail_run(id, "concurrent fail".into()).await
            }));
        }
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }

    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 20);
}

// ---------------------------------------------------------------------------
// 6. RunTracker capacity: 100 runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tracker_capacity_100_runs() {
    let tracker = RunTracker::new();
    let ids: Vec<Uuid> = (0..100).map(|_| Uuid::new_v4()).collect();

    for &id in &ids {
        tracker.start_run(id).await.unwrap();
    }

    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 100);

    for &id in &ids {
        assert!(tracker.get_run_status(id).await.is_some());
    }
}

// ---------------------------------------------------------------------------
// 7. Run expiry: remove completed/failed runs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_expiry_removes_completed() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    let receipt = make_receipt("mock", Outcome::Complete);
    tracker.complete_run(id, receipt).await.unwrap();

    // Completed run can be removed.
    let removed = tracker.remove_run(id).await.unwrap();
    assert!(matches!(removed, RunStatus::Completed { .. }));
    assert!(tracker.get_run_status(id).await.is_none());
}

#[tokio::test]
async fn run_expiry_removes_failed() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    tracker.fail_run(id, "boom".into()).await.unwrap();

    let removed = tracker.remove_run(id).await.unwrap();
    assert!(matches!(removed, RunStatus::Failed { .. }));
    assert!(tracker.get_run_status(id).await.is_none());
}

#[tokio::test]
async fn run_expiry_rejects_running() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();

    // Running run cannot be removed.
    let err = tracker.remove_run(id).await.unwrap_err();
    assert_eq!(err, "conflict");
}

// ---------------------------------------------------------------------------
// 8. Run lookup by ID
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_lookup_by_id() {
    let tracker = RunTracker::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    tracker.start_run(id1).await.unwrap();
    tracker.start_run(id2).await.unwrap();
    tracker
        .fail_run(id2, "err".into())
        .await
        .unwrap();

    assert!(matches!(
        tracker.get_run_status(id1).await.unwrap(),
        RunStatus::Running
    ));
    assert!(matches!(
        tracker.get_run_status(id2).await.unwrap(),
        RunStatus::Failed { .. }
    ));
}

// ---------------------------------------------------------------------------
// 9. List filtering by state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_filtering_by_state() {
    let tracker = RunTracker::new();
    let running_id = Uuid::new_v4();
    let completed_id = Uuid::new_v4();
    let failed_id = Uuid::new_v4();

    tracker.start_run(running_id).await.unwrap();
    tracker.start_run(completed_id).await.unwrap();
    tracker.start_run(failed_id).await.unwrap();

    let receipt = make_receipt("mock", Outcome::Complete);
    tracker.complete_run(completed_id, receipt).await.unwrap();
    tracker
        .fail_run(failed_id, "err".into())
        .await
        .unwrap();

    let runs = tracker.list_runs().await;

    let running: Vec<_> = runs
        .iter()
        .filter(|(_, s)| matches!(s, RunStatus::Running))
        .collect();
    let completed: Vec<_> = runs
        .iter()
        .filter(|(_, s)| matches!(s, RunStatus::Completed { .. }))
        .collect();
    let failed: Vec<_> = runs
        .iter()
        .filter(|(_, s)| matches!(s, RunStatus::Failed { .. }))
        .collect();

    assert_eq!(running.len(), 1);
    assert_eq!(completed.len(), 1);
    assert_eq!(failed.len(), 1);

    assert_eq!(running[0].0, running_id);
    assert_eq!(completed[0].0, completed_id);
    assert_eq!(failed[0].0, failed_id);
}

// ---------------------------------------------------------------------------
// 10. Event accumulation via receipt trace
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_accumulation_in_receipt_trace() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    let events = [
        make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        }),
        make_event(AgentEventKind::AssistantMessage {
            text: "hello world".into(),
        }),
        make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }),
    ];

    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .add_trace_event(events[0].clone())
        .add_trace_event(events[1].clone())
        .add_trace_event(events[2].clone())
        .build();

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert_eq!(receipt.trace.len(), 3);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 11. Receipt attachment
// ---------------------------------------------------------------------------

#[tokio::test]
async fn receipt_attached_to_completed_run() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    let receipt = ReceiptBuilder::new("openai")
        .outcome(Outcome::Complete)
        .backend_version("v1.2.3")
        .build();

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt.clone()).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt: stored } => {
            assert_eq!(stored.backend.id, "openai");
            assert_eq!(
                stored.backend.backend_version.as_deref(),
                Some("v1.2.3")
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 12. Idempotent completion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn idempotent_completion_overwrites() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();

    let r1 = ReceiptBuilder::new("first").outcome(Outcome::Complete).build();
    tracker.complete_run(id, r1).await.unwrap();

    // Completing again overwrites (the API allows it since the key exists).
    let r2 = ReceiptBuilder::new("second").outcome(Outcome::Complete).build();
    tracker.complete_run(id, r2).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert_eq!(receipt.backend.id, "second");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 13. Run metadata: all receipt fields preserved
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_metadata_all_fields_preserved() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    let wo_id = Uuid::new_v4();

    let receipt = ReceiptBuilder::new("anthropic")
        .work_order_id(wo_id)
        .outcome(Outcome::Partial)
        .backend_version("claude-3")
        .adapter_version("0.5.0")
        .usage_raw(serde_json::json!({"tokens": 42}))
        .build();

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert_eq!(receipt.meta.work_order_id, wo_id);
            assert_eq!(receipt.backend.id, "anthropic");
            assert_eq!(receipt.backend.backend_version.as_deref(), Some("claude-3"));
            assert_eq!(
                receipt.backend.adapter_version.as_deref(),
                Some("0.5.0")
            );
            assert!(matches!(receipt.outcome, Outcome::Partial));
            assert_eq!(receipt.usage_raw, serde_json::json!({"tokens": 42}));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 14. Run timing: started_at / finished_at populated
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_timing_populated() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    let before = Utc::now();

    let receipt = ReceiptBuilder::new("mock")
        .started_at(before)
        .finished_at(Utc::now())
        .outcome(Outcome::Complete)
        .build();

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert!(receipt.meta.started_at <= receipt.meta.finished_at);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 15. Empty tracker
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_tracker_returns_empty_list() {
    let tracker = RunTracker::new();

    let runs = tracker.list_runs().await;
    assert!(runs.is_empty());
}

#[tokio::test]
async fn empty_tracker_lookup_returns_none() {
    let tracker = RunTracker::new();
    assert!(tracker.get_run_status(Uuid::new_v4()).await.is_none());
}

// ---------------------------------------------------------------------------
// 16. Single run full lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn single_run_full_lifecycle() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    // Before start: not tracked.
    assert!(tracker.get_run_status(id).await.is_none());

    // Start.
    tracker.start_run(id).await.unwrap();
    assert!(matches!(
        tracker.get_run_status(id).await.unwrap(),
        RunStatus::Running
    ));

    // Complete.
    let receipt = make_receipt("mock", Outcome::Complete);
    tracker.complete_run(id, receipt).await.unwrap();
    assert!(matches!(
        tracker.get_run_status(id).await.unwrap(),
        RunStatus::Completed { .. }
    ));

    // Remove.
    tracker.remove_run(id).await.unwrap();
    assert!(tracker.get_run_status(id).await.is_none());
}

// ---------------------------------------------------------------------------
// 17. Backend tracking
// ---------------------------------------------------------------------------

#[tokio::test]
async fn backend_tracking_in_receipt() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    let receipt = ReceiptBuilder::new("sidecar:node")
        .outcome(Outcome::Complete)
        .backend_version("18.0.0")
        .build();

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert_eq!(receipt.backend.id, "sidecar:node");
            assert_eq!(
                receipt.backend.backend_version.as_deref(),
                Some("18.0.0")
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 18. Work order preservation via receipt metadata
// ---------------------------------------------------------------------------

#[tokio::test]
async fn work_order_id_preserved_in_receipt() {
    let tracker = RunTracker::new();
    let run_id = Uuid::new_v4();
    let wo_id = Uuid::new_v4();

    let receipt = ReceiptBuilder::new("mock")
        .work_order_id(wo_id)
        .outcome(Outcome::Complete)
        .build();

    tracker.start_run(run_id).await.unwrap();
    tracker.complete_run(run_id, receipt).await.unwrap();

    match tracker.get_run_status(run_id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert_eq!(receipt.meta.work_order_id, wo_id);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 19. Error details on failed run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn error_details_on_failed_run() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    tracker
        .fail_run(id, "connection refused: backend unreachable".into())
        .await
        .unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Failed { error } => {
            assert!(error.contains("connection refused"));
            assert!(error.contains("backend unreachable"));
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 20. Stats computation: count, success rate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stats_computation() {
    let tracker = RunTracker::new();

    // Create 10 runs: 6 completed, 3 failed, 1 still running.
    let mut ids = Vec::new();
    for _ in 0..10 {
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        ids.push(id);
    }

    // Complete first 6.
    for &id in &ids[..6] {
        let receipt = make_receipt("mock", Outcome::Complete);
        tracker.complete_run(id, receipt).await.unwrap();
    }

    // Fail next 3.
    for &id in &ids[6..9] {
        tracker
            .fail_run(id, "error".into())
            .await
            .unwrap();
    }
    // ids[9] stays Running.

    let runs = tracker.list_runs().await;
    let total = runs.len();
    let completed = runs
        .iter()
        .filter(|(_, s)| matches!(s, RunStatus::Completed { .. }))
        .count();
    let failed = runs
        .iter()
        .filter(|(_, s)| matches!(s, RunStatus::Failed { .. }))
        .count();
    let running = runs
        .iter()
        .filter(|(_, s)| matches!(s, RunStatus::Running))
        .count();

    assert_eq!(total, 10);
    assert_eq!(completed, 6);
    assert_eq!(failed, 3);
    assert_eq!(running, 1);

    // Success rate: 6 out of 9 terminal runs.
    let terminal = completed + failed;
    let success_rate = completed as f64 / terminal as f64;
    assert!((success_rate - 0.6667).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// Additional edge-case tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duplicate_start_rejected() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    let err = tracker.start_run(id).await.unwrap_err();
    assert!(err.to_string().contains("already tracked"));
}

#[tokio::test]
async fn complete_untracked_run_rejected() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    let receipt = make_receipt("mock", Outcome::Complete);

    let err = tracker.complete_run(id, receipt).await.unwrap_err();
    assert!(err.to_string().contains("not tracked"));
}

#[tokio::test]
async fn fail_untracked_run_rejected() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    let err = tracker.fail_run(id, "boom".into()).await.unwrap_err();
    assert!(err.to_string().contains("not tracked"));
}

#[tokio::test]
async fn remove_nonexistent_run_returns_not_found() {
    let tracker = RunTracker::new();
    let err = tracker.remove_run(Uuid::new_v4()).await.unwrap_err();
    assert_eq!(err, "not found");
}

#[tokio::test]
async fn failed_run_preserves_outcome_enum() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    // Build a receipt with Failed outcome and attach via complete_run.
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Failed)
        .build();

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt } => {
            assert!(matches!(receipt.outcome, Outcome::Failed));
        }
        other => panic!("expected Completed with Failed outcome, got {other:?}"),
    }
}

#[tokio::test]
async fn receipt_with_hash_stored_correctly() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .with_hash()
        .expect("hash should succeed");

    assert!(receipt.receipt_sha256.is_some());

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt.clone()).await.unwrap();

    match tracker.get_run_status(id).await.unwrap() {
        RunStatus::Completed { receipt: stored } => {
            assert_eq!(stored.receipt_sha256, receipt.receipt_sha256);
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn many_runs_with_mixed_backends() {
    let tracker = RunTracker::new();
    let backends = ["mock", "sidecar:node", "openai", "anthropic", "gemini"];

    let mut ids = Vec::new();
    for (i, &backend) in backends.iter().cycle().take(25).enumerate() {
        let id = Uuid::new_v4();
        tracker.start_run(id).await.unwrap();
        let receipt = ReceiptBuilder::new(backend)
            .outcome(if i % 3 == 0 {
                Outcome::Failed
            } else {
                Outcome::Complete
            })
            .build();
        tracker.complete_run(id, receipt).await.unwrap();
        ids.push(id);
    }

    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 25);

    // Verify all are completed (terminal).
    for (_, status) in &runs {
        assert!(matches!(status, RunStatus::Completed { .. }));
    }
}
