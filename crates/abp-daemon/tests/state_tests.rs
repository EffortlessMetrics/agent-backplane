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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
use abp_core::{Outcome, ReceiptBuilder};
use abp_daemon::{RunStatus, RunTracker};
use uuid::Uuid;

#[tokio::test]
async fn start_run_sets_running_status() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();

    let status = tracker.get_run_status(id).await.unwrap();
    assert!(matches!(status, RunStatus::Running));
}

#[tokio::test]
async fn complete_run_stores_receipt() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();

    tracker.start_run(id).await.unwrap();
    tracker.complete_run(id, receipt.clone()).await.unwrap();

    let status = tracker.get_run_status(id).await.unwrap();
    match status {
        RunStatus::Completed { receipt: r } => {
            assert_eq!(r.backend.id, "mock");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[tokio::test]
async fn fail_run_stores_error() {
    let tracker = RunTracker::new();
    let id = Uuid::new_v4();

    tracker.start_run(id).await.unwrap();
    tracker
        .fail_run(id, "backend exploded".into())
        .await
        .unwrap();

    let status = tracker.get_run_status(id).await.unwrap();
    match status {
        RunStatus::Failed { error } => {
            assert_eq!(error, "backend exploded");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn list_runs_shows_all_tracked() {
    let tracker = RunTracker::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();

    tracker.start_run(id1).await.unwrap();
    tracker.start_run(id2).await.unwrap();
    tracker.fail_run(id2, "oops".into()).await.unwrap();

    let runs = tracker.list_runs().await;
    assert_eq!(runs.len(), 2);

    let ids: Vec<Uuid> = runs.iter().map(|(id, _)| *id).collect();
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}

#[tokio::test]
async fn get_nonexistent_run_returns_none() {
    let tracker = RunTracker::new();
    assert!(tracker.get_run_status(Uuid::new_v4()).await.is_none());
}
