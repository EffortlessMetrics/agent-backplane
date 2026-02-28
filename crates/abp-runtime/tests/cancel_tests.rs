// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the cancellation module.

use abp_runtime::cancel::{CancellableRun, CancellationReason, CancellationToken};

#[test]
fn token_starts_not_cancelled() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn cancel_flips_state() {
    let token = CancellationToken::new();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn clone_shares_state() {
    let a = CancellationToken::new();
    let b = a.clone();
    a.cancel();
    assert!(b.is_cancelled());
}

#[tokio::test]
async fn await_cancelled_future_completes_after_cancel() {
    let token = CancellationToken::new();
    let clone = token.clone();

    let handle = tokio::spawn(async move {
        clone.cancelled().await;
        true
    });

    // Give the spawned task a moment to start waiting.
    tokio::task::yield_now().await;
    token.cancel();

    let result = handle.await.unwrap();
    assert!(result, "cancelled() future should have resolved");
}

#[tokio::test]
async fn cancelled_future_resolves_immediately_if_already_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    // Should not hang — completes immediately.
    token.cancelled().await;
    assert!(token.is_cancelled());
}

#[test]
fn all_cancellation_reason_variants_have_descriptions() {
    let reasons = [
        CancellationReason::UserRequested,
        CancellationReason::Timeout,
        CancellationReason::BudgetExhausted,
        CancellationReason::PolicyViolation,
        CancellationReason::SystemShutdown,
    ];
    for r in &reasons {
        let desc = r.description();
        assert!(!desc.is_empty(), "{r:?} should have a non-empty description");
    }
}

#[test]
fn cancellable_run_tracks_reason() {
    let run = CancellableRun::new(CancellationToken::new());
    assert!(!run.is_cancelled());
    assert!(run.reason().is_none());

    run.cancel(CancellationReason::UserRequested);
    assert!(run.is_cancelled());
    assert_eq!(run.reason(), Some(CancellationReason::UserRequested));
}

#[test]
fn multiple_cancels_are_idempotent() {
    let token = CancellationToken::new();
    token.cancel();
    token.cancel();
    token.cancel();
    assert!(token.is_cancelled());
}

#[test]
fn cancellable_run_keeps_first_reason() {
    let run = CancellableRun::new(CancellationToken::new());
    run.cancel(CancellationReason::BudgetExhausted);
    run.cancel(CancellationReason::Timeout);
    assert_eq!(run.reason(), Some(CancellationReason::BudgetExhausted));
}

#[tokio::test]
async fn concurrent_cancel_and_check() {
    let token = CancellationToken::new();
    let mut handles = Vec::new();

    for _ in 0..10 {
        let t = token.clone();
        handles.push(tokio::spawn(async move {
            t.cancel();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert!(token.is_cancelled());
}

#[test]
fn serde_roundtrip_for_cancellation_reason() {
    let reasons = [
        CancellationReason::UserRequested,
        CancellationReason::Timeout,
        CancellationReason::BudgetExhausted,
        CancellationReason::PolicyViolation,
        CancellationReason::SystemShutdown,
    ];
    for reason in &reasons {
        let json = serde_json::to_string(reason).unwrap();
        let back: CancellationReason = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, reason);
    }
}

#[test]
fn cancel_token_drop_behaviour() {
    let token = CancellationToken::new();
    let clone = token.clone();
    drop(token);
    // Clone still works after the original is dropped.
    assert!(!clone.is_cancelled());
    clone.cancel();
    assert!(clone.is_cancelled());
}

#[test]
fn cancellable_run_clone_shares_state() {
    let run = CancellableRun::new(CancellationToken::new());
    let run2 = run.clone();
    run.cancel(CancellationReason::SystemShutdown);
    assert!(run2.is_cancelled());
    assert_eq!(run2.reason(), Some(CancellationReason::SystemShutdown));
}
