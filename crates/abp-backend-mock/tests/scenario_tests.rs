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
use abp_backend_core::Backend;
use abp_backend_mock::scenarios::{MockBackendRecorder, MockScenario, ScenarioMockBackend};
use abp_backend_mock::MockBackend;
use abp_core::{Outcome, WorkOrderBuilder};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Helper: build a simple work order.
fn test_work_order(task: &str) -> abp_core::WorkOrder {
    WorkOrderBuilder::new(task).build()
}

/// Helper: run a backend with a fresh channel.
async fn run_backend(
    backend: &impl Backend,
    task: &str,
) -> anyhow::Result<(abp_core::Receipt, Vec<abp_core::AgentEvent>)> {
    let wo = test_work_order(task);
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await?;
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    Ok((receipt, events))
}

// ===== Success scenario =====

#[tokio::test]
async fn success_returns_complete_receipt() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "hello".into(),
    });
    let (receipt, _) = run_backend(&b, "t1").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn success_emits_assistant_message() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "the answer".into(),
    });
    let (_, events) = run_backend(&b, "t2").await.unwrap();
    let msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            abp_core::AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(msgs.contains(&"the answer"));
}

#[tokio::test]
async fn success_with_delay_still_completes() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 50,
        text: "delayed".into(),
    });
    let start = std::time::Instant::now();
    let (receipt, _) = run_backend(&b, "t3").await.unwrap();
    assert!(start.elapsed().as_millis() >= 40);
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn success_increments_call_count() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert_eq!(b.call_count(), 0);
    let _ = run_backend(&b, "t4").await.unwrap();
    assert_eq!(b.call_count(), 1);
    let _ = run_backend(&b, "t4b").await.unwrap();
    assert_eq!(b.call_count(), 2);
}

#[tokio::test]
async fn success_identity_is_scenario_mock() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert_eq!(b.identity().id, "scenario-mock");
}

// ===== Streaming scenario =====

#[tokio::test]
async fn streaming_emits_all_chunks() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into(), "c".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = run_backend(&b, "stream").await.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            abp_core::AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn streaming_emits_run_started_and_completed() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["x".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = run_backend(&b, "stream2").await.unwrap();
    let has_started = events
        .iter()
        .any(|e| matches!(&e.kind, abp_core::AgentEventKind::RunStarted { .. }));
    let has_completed = events
        .iter()
        .any(|e| matches!(&e.kind, abp_core::AgentEventKind::RunCompleted { .. }));
    assert!(has_started);
    assert!(has_completed);
}

#[tokio::test]
async fn streaming_empty_chunks_still_completes() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec![],
        chunk_delay_ms: 0,
    });
    let (receipt, _) = run_backend(&b, "empty-stream").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_with_delay_respects_timing() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into(), "c".into()],
        chunk_delay_ms: 30,
    });
    let start = std::time::Instant::now();
    let _ = run_backend(&b, "timed-stream").await.unwrap();
    // 3 chunks × 30ms = ~90ms minimum
    assert!(start.elapsed().as_millis() >= 70);
}

// ===== Transient error scenario =====

#[tokio::test]
async fn transient_error_fails_then_succeeds() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 2,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "recovered".into(),
        }),
    });

    // First two calls fail
    let r1 = run_backend(&b, "te1").await;
    assert!(r1.is_err());
    let r2 = run_backend(&b, "te2").await;
    assert!(r2.is_err());

    // Third call succeeds
    let (receipt, _) = run_backend(&b, "te3").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(b.call_count(), 3);
}

#[tokio::test]
async fn transient_error_message_contains_attempt_info() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    let err = run_backend(&b, "te-msg").await.unwrap_err();
    assert!(err.to_string().contains("transient error"));
    assert!(err.to_string().contains("1/1"));
}

#[tokio::test]
async fn transient_error_tracks_last_error() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    let _ = run_backend(&b, "le").await; // fails
    let err = b.last_error().await;
    assert!(err.is_some());
    assert!(err.unwrap().contains("transient"));
}

// ===== Permanent error scenario =====

#[tokio::test]
async fn permanent_error_always_fails() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "E001".into(),
        message: "always broken".into(),
    });
    for _ in 0..3 {
        let err = run_backend(&b, "pe").await.unwrap_err();
        assert!(err.to_string().contains("E001"));
        assert!(err.to_string().contains("always broken"));
    }
    assert_eq!(b.call_count(), 3);
}

#[tokio::test]
async fn permanent_error_records_all_failures() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "E002".into(),
        message: "nope".into(),
    });
    let _ = run_backend(&b, "pe-rec1").await;
    let _ = run_backend(&b, "pe-rec2").await;
    let calls = b.calls().await;
    assert_eq!(calls.len(), 2);
    assert!(calls[0].result.is_err());
    assert!(calls[1].result.is_err());
}

// ===== Timeout scenario =====

#[tokio::test]
async fn timeout_fails_after_delay() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 50 });
    let start = std::time::Instant::now();
    let err = run_backend(&b, "timeout").await.unwrap_err();
    assert!(start.elapsed().as_millis() >= 40);
    assert!(err.to_string().contains("timeout"));
}

#[tokio::test]
async fn timeout_records_the_call() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 10 });
    let _ = run_backend(&b, "timeout-rec").await;
    assert_eq!(b.call_count(), 1);
    let last = b.last_call().await.unwrap();
    assert!(last.result.is_err());
}

// ===== Rate-limited scenario =====

#[tokio::test]
async fn rate_limited_returns_retry_info() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 1000,
    });
    let err = run_backend(&b, "rl").await.unwrap_err();
    assert!(err.to_string().contains("rate limited"));
    assert!(err.to_string().contains("1000"));
}

#[tokio::test]
async fn rate_limited_does_not_delay() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 5000,
    });
    let start = std::time::Instant::now();
    let _ = run_backend(&b, "rl-fast").await;
    // Should return immediately (no sleep)
    assert!(start.elapsed().as_millis() < 500);
}

// ===== MockBackendRecorder =====

#[tokio::test]
async fn recorder_records_successful_calls() {
    let inner = MockBackend;
    let recorder = MockBackendRecorder::new(inner);
    let _ = run_backend(&recorder, "rec1").await.unwrap();
    assert_eq!(recorder.call_count().await, 1);
    let last = recorder.last_call().await.unwrap();
    assert!(last.result.is_ok());
}

#[tokio::test]
async fn recorder_preserves_work_order() {
    let recorder = MockBackendRecorder::new(MockBackend);
    let wo = test_work_order("specific-task");
    let (tx, _rx) = mpsc::channel(16);
    let _ = recorder.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let calls = recorder.calls().await;
    assert_eq!(calls[0].work_order.task, "specific-task");
}

#[tokio::test]
async fn recorder_wrapping_scenario_records_errors() {
    let scenario = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "X".into(),
        message: "fail".into(),
    });
    let recorder = MockBackendRecorder::new(scenario);
    let _ = run_backend(&recorder, "rec-err").await;
    let calls = recorder.calls().await;
    assert_eq!(calls.len(), 1);
    assert!(calls[0].result.is_err());
}

#[tokio::test]
async fn recorder_identity_delegates_to_inner() {
    let recorder = MockBackendRecorder::new(MockBackend);
    assert_eq!(recorder.identity().id, "mock");
}

#[tokio::test]
async fn recorder_multiple_calls_tracked() {
    let recorder = MockBackendRecorder::new(MockBackend);
    for i in 0..5 {
        let _ = run_backend(&recorder, &format!("task-{i}")).await.unwrap();
    }
    assert_eq!(recorder.call_count().await, 5);
}

// ===== Serialization =====

#[tokio::test]
async fn scenario_serializes_to_json() {
    let s = MockScenario::Success {
        delay_ms: 100,
        text: "hi".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"success\""));
    let deser: MockScenario = serde_json::from_str(&json).unwrap();
    match deser {
        MockScenario::Success { delay_ms, text } => {
            assert_eq!(delay_ms, 100);
            assert_eq!(text, "hi");
        }
        _ => panic!("wrong variant"),
    }
}

#[tokio::test]
async fn transient_scenario_round_trips() {
    let s = MockScenario::TransientError {
        fail_count: 3,
        then: Box::new(MockScenario::StreamingSuccess {
            chunks: vec!["a".into()],
            chunk_delay_ms: 10,
        }),
    };
    let json = serde_json::to_string(&s).unwrap();
    let deser: MockScenario = serde_json::from_str(&json).unwrap();
    match deser {
        MockScenario::TransientError { fail_count, then } => {
            assert_eq!(fail_count, 3);
            assert!(matches!(*then, MockScenario::StreamingSuccess { .. }));
        }
        _ => panic!("wrong variant"),
    }
}

#[tokio::test]
async fn recorded_call_serializes() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "ok".into(),
    });
    let _ = run_backend(&b, "ser").await.unwrap();
    let call = b.last_call().await.unwrap();
    let json = serde_json::to_value(&call).unwrap();
    assert!(json.get("work_order").is_some());
    assert!(json.get("timestamp").is_some());
    assert!(json.get("duration_ms").is_some());
    assert!(json.get("result").is_some());
}

// ===== Clone =====

#[tokio::test]
async fn scenario_backend_clone_shares_call_records() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "shared".into(),
    });
    let b2 = b.clone();
    let _ = run_backend(&b, "c1").await.unwrap();
    // Clone sees the call recorded by the original via shared Arc.
    let calls = b2.calls().await;
    assert_eq!(calls.len(), 1);
}

// ===== Receipt integrity =====

#[tokio::test]
async fn success_receipt_has_valid_hash() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "hash-check".into(),
    });
    let (receipt, _) = run_backend(&b, "hash").await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars
}

#[tokio::test]
async fn streaming_receipt_has_valid_hash() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["x".into()],
        chunk_delay_ms: 0,
    });
    let (receipt, _) = run_backend(&b, "hash-stream").await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}
