#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(unused_must_use)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use abp_backend_core::Backend;
use abp_backend_mock::MockBackend;
use abp_backend_mock::scenarios::{
    MockBackendRecorder, MockScenario, RecordedCall, ScenarioMockBackend,
};
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome, Receipt,
    RuntimeConfig, SupportLevel, WorkOrder, WorkOrderBuilder,
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn make_wo_with_reqs(task: &str, reqs: CapabilityRequirements) -> WorkOrder {
    WorkOrderBuilder::new(task).requirements(reqs).build()
}

fn make_wo_with_vendor(task: &str, vendor: BTreeMap<String, serde_json::Value>) -> WorkOrder {
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    WorkOrderBuilder::new(task).config(config).build()
}

async fn drive(backend: &impl Backend, task: &str) -> anyhow::Result<(Receipt, Vec<AgentEvent>)> {
    let wo = make_wo(task);
    let (tx, mut rx) = mpsc::channel(256);
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await?;
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    Ok((receipt, events))
}

async fn drive_wo(
    backend: &impl Backend,
    wo: WorkOrder,
) -> anyhow::Result<(Receipt, Vec<AgentEvent>)> {
    let (tx, mut rx) = mpsc::channel(256);
    let receipt = backend.run(Uuid::new_v4(), wo, tx).await?;
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    Ok((receipt, events))
}

fn count_events<F: Fn(&AgentEventKind) -> bool>(events: &[AgentEvent], f: F) -> usize {
    events.iter().filter(|e| f(&e.kind)).count()
}

// ===========================================================================
// 1. MockBackend – construction & identity
// ===========================================================================

#[tokio::test]
async fn mock_backend_debug_impl() {
    let b = MockBackend;
    let dbg = format!("{:?}", b);
    assert!(dbg.contains("MockBackend"));
}

#[tokio::test]
async fn mock_backend_clone_is_independent() {
    let a = MockBackend;
    let b = a.clone();
    // Both should produce the same identity
    assert_eq!(a.identity().id, b.identity().id);
}

#[tokio::test]
async fn mock_identity_id_is_mock() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[tokio::test]
async fn mock_identity_backend_version_is_0_1() {
    assert_eq!(
        MockBackend.identity().backend_version.as_deref(),
        Some("0.1")
    );
}

#[tokio::test]
async fn mock_identity_adapter_version_is_0_1() {
    assert_eq!(
        MockBackend.identity().adapter_version.as_deref(),
        Some("0.1")
    );
}

// ===========================================================================
// 2. MockBackend – capabilities
// ===========================================================================

#[tokio::test]
async fn mock_caps_contains_streaming_native() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[tokio::test]
async fn mock_caps_contains_tool_read_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_caps_contains_tool_write_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_caps_contains_tool_edit_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_caps_contains_tool_bash_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_caps_contains_structured_output_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_caps_has_exactly_six_entries() {
    assert_eq!(MockBackend.capabilities().len(), 6);
}

#[tokio::test]
async fn mock_caps_does_not_include_vision() {
    assert!(
        MockBackend
            .capabilities()
            .get(&Capability::Vision)
            .is_none()
    );
}

// ===========================================================================
// 3. MockBackend – run: event sequence
// ===========================================================================

#[tokio::test]
async fn mock_run_produces_four_events() {
    let (_, events) = drive(&MockBackend, "task").await.unwrap();
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn mock_run_first_event_is_run_started() {
    let (_, events) = drive(&MockBackend, "task").await.unwrap();
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn mock_run_last_event_is_run_completed() {
    let (_, events) = drive(&MockBackend, "task").await.unwrap();
    assert!(matches!(
        &events[events.len() - 1].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn mock_run_has_two_assistant_messages() {
    let (_, events) = drive(&MockBackend, "task").await.unwrap();
    let cnt = count_events(&events, |k| {
        matches!(k, AgentEventKind::AssistantMessage { .. })
    });
    assert_eq!(cnt, 2);
}

#[tokio::test]
async fn mock_run_started_message_includes_task() {
    let (_, events) = drive(&MockBackend, "hello world").await.unwrap();
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("hello world"));
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn mock_run_events_ext_is_none() {
    let (_, events) = drive(&MockBackend, "t").await.unwrap();
    for ev in &events {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn mock_run_event_timestamps_are_non_decreasing() {
    let (_, events) = drive(&MockBackend, "t").await.unwrap();
    for pair in events.windows(2) {
        assert!(pair[1].ts >= pair[0].ts);
    }
}

// ===========================================================================
// 4. MockBackend – run: receipt fields
// ===========================================================================

#[tokio::test]
async fn mock_receipt_outcome_is_complete() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mock_receipt_contract_version() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn mock_receipt_backend_id() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.backend.id, "mock");
}

#[tokio::test]
async fn mock_receipt_has_sha256() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert!(r.receipt_sha256.is_some());
}

#[tokio::test]
async fn mock_receipt_sha256_is_64_hex() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    let h = r.receipt_sha256.unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn mock_receipt_started_before_finished() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert!(r.meta.started_at <= r.meta.finished_at);
}

#[tokio::test]
async fn mock_receipt_work_order_id_matches() {
    let wo = make_wo("t");
    let wo_id = wo.id;
    let (tx, _) = mpsc::channel(64);
    let r = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(r.meta.work_order_id, wo_id);
}

#[tokio::test]
async fn mock_receipt_run_id_matches() {
    let wo = make_wo("t");
    let run_id = Uuid::new_v4();
    let (tx, _) = mpsc::channel(64);
    let r = MockBackend.run(run_id, wo, tx).await.unwrap();
    assert_eq!(r.meta.run_id, run_id);
}

#[tokio::test]
async fn mock_receipt_zero_input_tokens() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.usage.input_tokens, Some(0));
}

#[tokio::test]
async fn mock_receipt_zero_output_tokens() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.usage.output_tokens, Some(0));
}

#[tokio::test]
async fn mock_receipt_zero_cost() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn mock_receipt_no_artifacts() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert!(r.artifacts.is_empty());
}

#[tokio::test]
async fn mock_receipt_verification_harness_ok() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert!(r.verification.harness_ok);
}

#[tokio::test]
async fn mock_receipt_no_git_diff() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert!(r.verification.git_diff.is_none());
}

#[tokio::test]
async fn mock_receipt_no_git_status() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert!(r.verification.git_status.is_none());
}

#[tokio::test]
async fn mock_receipt_default_execution_mode() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn mock_receipt_usage_raw_has_mock_note() {
    let (r, _) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.usage_raw, json!({"note": "mock"}));
}

#[tokio::test]
async fn mock_receipt_trace_matches_events() {
    let (r, events) = drive(&MockBackend, "t").await.unwrap();
    assert_eq!(r.trace.len(), events.len());
}

// ===========================================================================
// 5. MockBackend – execution mode passthrough via vendor config
// ===========================================================================

#[tokio::test]
async fn mock_passthrough_mode_via_abp_vendor_key() {
    let mut vendor = BTreeMap::new();
    let mut abp = serde_json::Map::new();
    abp.insert("mode".into(), json!("passthrough"));
    vendor.insert("abp".into(), json!(abp));
    let wo = make_wo_with_vendor("t", vendor);
    let (r, _) = drive_wo(&MockBackend, wo).await.unwrap();
    assert_eq!(r.mode, ExecutionMode::Passthrough);
}

// ===========================================================================
// 6. ScenarioMockBackend – Success scenario
// ===========================================================================

#[tokio::test]
async fn scenario_success_completes() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "ok".into(),
    });
    let (r, _) = drive(&b, "t").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn scenario_success_emits_three_events() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "hi".into(),
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    // RunStarted + AssistantMessage + RunCompleted
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn scenario_success_assistant_message_matches_text() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "custom text".into(),
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    let msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(msgs, vec!["custom text"]);
}

#[tokio::test]
async fn scenario_success_identity_is_scenario_mock() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert_eq!(b.identity().id, "scenario-mock");
}

#[tokio::test]
async fn scenario_success_capabilities_delegate_to_mock() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert_eq!(b.capabilities().len(), MockBackend.capabilities().len());
}

#[tokio::test]
async fn scenario_success_with_delay() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 60,
        text: "delayed".into(),
    });
    let start = Instant::now();
    let (r, _) = drive(&b, "t").await.unwrap();
    assert!(start.elapsed().as_millis() >= 50);
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn scenario_success_receipt_backend_is_scenario_mock() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (r, _) = drive(&b, "t").await.unwrap();
    assert_eq!(r.backend.id, "scenario-mock");
}

#[tokio::test]
async fn scenario_success_receipt_usage_raw_has_scenario_note() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (r, _) = drive(&b, "t").await.unwrap();
    assert_eq!(r.usage_raw, json!({"note": "scenario-mock"}));
}

// ===========================================================================
// 7. ScenarioMockBackend – StreamingSuccess scenario
// ===========================================================================

#[tokio::test]
async fn streaming_emits_correct_chunk_order() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["one".into(), "two".into(), "three".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["one", "two", "three"]);
}

#[tokio::test]
async fn streaming_event_count_is_chunks_plus_two() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    // RunStarted + 2 deltas + RunCompleted = 4
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn streaming_empty_chunks_only_lifecycle() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec![],
        chunk_delay_ms: 0,
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    assert_eq!(events.len(), 2); // RunStarted + RunCompleted only
}

#[tokio::test]
async fn streaming_single_chunk() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["solo".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["solo"]);
}

#[tokio::test]
async fn streaming_receipt_outcome_is_complete() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["x".into()],
        chunk_delay_ms: 0,
    });
    let (r, _) = drive(&b, "t").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_receipt_trace_matches_event_count() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into()],
        chunk_delay_ms: 0,
    });
    let (r, events) = drive(&b, "t").await.unwrap();
    assert_eq!(r.trace.len(), events.len());
}

#[tokio::test]
async fn streaming_many_chunks() {
    let chunks: Vec<String> = (0..50).map(|i| format!("chunk-{i}")).collect();
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: chunks.clone(),
        chunk_delay_ms: 0,
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas.len(), 50);
    assert_eq!(deltas[0], "chunk-0");
    assert_eq!(deltas[49], "chunk-49");
}

// ===========================================================================
// 8. ScenarioMockBackend – TransientError scenario
// ===========================================================================

#[tokio::test]
async fn transient_error_fails_n_times_then_succeeds() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 3,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "recovered".into(),
        }),
    });
    for _ in 0..3 {
        assert!(drive(&b, "t").await.is_err());
    }
    let (r, _) = drive(&b, "t").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
    assert_eq!(b.call_count(), 4);
}

#[tokio::test]
async fn transient_error_zero_fails_succeeds_immediately() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 0,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "instant".into(),
        }),
    });
    let (r, _) = drive(&b, "t").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn transient_error_message_includes_attempt() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 2,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    let e1 = drive(&b, "t").await.unwrap_err();
    assert!(e1.to_string().contains("1/2"));
    let e2 = drive(&b, "t").await.unwrap_err();
    assert!(e2.to_string().contains("2/2"));
}

#[tokio::test]
async fn transient_error_last_error_tracks() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    assert!(b.last_error().await.is_none());
    let _ = drive(&b, "t").await;
    assert!(b.last_error().await.is_some());
}

#[tokio::test]
async fn transient_then_streaming() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::StreamingSuccess {
            chunks: vec!["a".into(), "b".into()],
            chunk_delay_ms: 0,
        }),
    });
    assert!(drive(&b, "t").await.is_err());
    let (_, events) = drive(&b, "t").await.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["a", "b"]);
}

// ===========================================================================
// 9. ScenarioMockBackend – PermanentError scenario
// ===========================================================================

#[tokio::test]
async fn permanent_error_always_fails_multiple_calls() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "ERR-42".into(),
        message: "broken forever".into(),
    });
    for i in 0..5 {
        let err = drive(&b, &format!("t{i}")).await.unwrap_err();
        assert!(err.to_string().contains("ERR-42"));
        assert!(err.to_string().contains("broken forever"));
    }
    assert_eq!(b.call_count(), 5);
}

#[tokio::test]
async fn permanent_error_sets_last_error() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "X".into(),
        message: "bad".into(),
    });
    let _ = drive(&b, "t").await;
    let le = b.last_error().await.unwrap();
    assert!(le.contains("X"));
}

#[tokio::test]
async fn permanent_error_records_all_calls() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "E".into(),
        message: "m".into(),
    });
    for _ in 0..3 {
        let _ = drive(&b, "t").await;
    }
    let calls = b.calls().await;
    assert_eq!(calls.len(), 3);
    for c in &calls {
        assert!(c.result.is_err());
    }
}

// ===========================================================================
// 10. ScenarioMockBackend – Timeout scenario
// ===========================================================================

#[tokio::test]
async fn timeout_sleeps_then_fails() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 60 });
    let start = Instant::now();
    let err = drive(&b, "t").await.unwrap_err();
    assert!(start.elapsed().as_millis() >= 50);
    assert!(err.to_string().contains("timeout"));
}

#[tokio::test]
async fn timeout_zero_ms_fails_immediately() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 0 });
    let start = Instant::now();
    let err = drive(&b, "t").await.unwrap_err();
    assert!(start.elapsed().as_millis() < 200);
    assert!(err.to_string().contains("timeout"));
}

#[tokio::test]
async fn timeout_error_message_includes_duration() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 123 });
    let err = drive(&b, "t").await.unwrap_err();
    assert!(err.to_string().contains("123"));
}

// ===========================================================================
// 11. ScenarioMockBackend – RateLimited scenario
// ===========================================================================

#[tokio::test]
async fn rate_limited_returns_retry_hint() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 2000,
    });
    let err = drive(&b, "t").await.unwrap_err();
    assert!(err.to_string().contains("rate limited"));
    assert!(err.to_string().contains("2000"));
}

#[tokio::test]
async fn rate_limited_is_instant() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 10000,
    });
    let start = Instant::now();
    let _ = drive(&b, "t").await;
    assert!(start.elapsed().as_millis() < 500);
}

// ===========================================================================
// 12. ScenarioMockBackend – call tracking
// ===========================================================================

#[tokio::test]
async fn call_count_starts_at_zero() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert_eq!(b.call_count(), 0);
}

#[tokio::test]
async fn call_count_increments_on_each_run() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    for i in 1..=4 {
        let _ = drive(&b, "t").await;
        assert_eq!(b.call_count(), i);
    }
}

#[tokio::test]
async fn calls_initially_empty() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert!(b.calls().await.is_empty());
}

#[tokio::test]
async fn last_call_initially_none() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert!(b.last_call().await.is_none());
}

#[tokio::test]
async fn last_error_initially_none() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert!(b.last_error().await.is_none());
}

#[tokio::test]
async fn recorded_call_has_correct_task() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = drive(&b, "my-task").await;
    let last = b.last_call().await.unwrap();
    assert_eq!(last.work_order.task, "my-task");
}

#[tokio::test]
async fn recorded_call_result_is_ok_on_success() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = drive(&b, "t").await.unwrap();
    let last = b.last_call().await.unwrap();
    assert!(last.result.is_ok());
    assert_eq!(last.result.unwrap(), Outcome::Complete);
}

#[tokio::test]
async fn recorded_call_result_is_err_on_failure() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "X".into(),
        message: "m".into(),
    });
    let _ = drive(&b, "t").await;
    let last = b.last_call().await.unwrap();
    assert!(last.result.is_err());
}

// ===========================================================================
// 13. ScenarioMockBackend – Clone behavior
// ===========================================================================

#[tokio::test]
async fn scenario_clone_shares_calls_arc() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let b2 = b.clone();
    let _ = drive(&b, "from-original").await;
    let calls = b2.calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].work_order.task, "from-original");
}

#[tokio::test]
async fn scenario_clone_shares_last_error_arc() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "E".into(),
        message: "fail".into(),
    });
    let b2 = b.clone();
    let _ = drive(&b, "t").await;
    assert!(b2.last_error().await.is_some());
}

// ===========================================================================
// 14. MockBackendRecorder – wrapping MockBackend
// ===========================================================================

#[tokio::test]
async fn recorder_records_successful_call() {
    let rec = MockBackendRecorder::new(MockBackend);
    let _ = drive(&rec, "t").await.unwrap();
    assert_eq!(rec.call_count().await, 1);
}

#[tokio::test]
async fn recorder_identity_delegates() {
    let rec = MockBackendRecorder::new(MockBackend);
    assert_eq!(rec.identity().id, "mock");
}

#[tokio::test]
async fn recorder_capabilities_delegate() {
    let rec = MockBackendRecorder::new(MockBackend);
    assert_eq!(rec.capabilities().len(), MockBackend.capabilities().len());
}

#[tokio::test]
async fn recorder_preserves_work_order_task() {
    let rec = MockBackendRecorder::new(MockBackend);
    let wo = make_wo("specific-task");
    let (tx, _rx) = mpsc::channel(64);
    let _ = rec.run(Uuid::new_v4(), wo, tx).await.unwrap();
    let last = rec.last_call().await.unwrap();
    assert_eq!(last.work_order.task, "specific-task");
}

#[tokio::test]
async fn recorder_last_call_none_initially() {
    let rec = MockBackendRecorder::new(MockBackend);
    assert!(rec.last_call().await.is_none());
}

#[tokio::test]
async fn recorder_tracks_multiple_calls() {
    let rec = MockBackendRecorder::new(MockBackend);
    for i in 0..4 {
        let _ = drive(&rec, &format!("task-{i}")).await.unwrap();
    }
    assert_eq!(rec.call_count().await, 4);
}

#[tokio::test]
async fn recorder_wrapping_scenario_records_errors() {
    let inner = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "X".into(),
        message: "bad".into(),
    });
    let rec = MockBackendRecorder::new(inner);
    let _ = drive(&rec, "t").await;
    let calls = rec.calls().await;
    assert_eq!(calls.len(), 1);
    assert!(calls[0].result.is_err());
}

#[tokio::test]
async fn recorder_clone_shares_calls() {
    let rec = MockBackendRecorder::new(MockBackend);
    let rec2 = rec.clone();
    let _ = drive(&rec, "t").await.unwrap();
    assert_eq!(rec2.call_count().await, 1);
}

// ===========================================================================
// 15. Serde roundtrips – MockScenario variants
// ===========================================================================

#[test]
fn serde_success_roundtrip() {
    let s = MockScenario::Success {
        delay_ms: 42,
        text: "hello".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"success\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::Success { delay_ms, text } => {
            assert_eq!(delay_ms, 42);
            assert_eq!(text, "hello");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn serde_streaming_success_roundtrip() {
    let s = MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into()],
        chunk_delay_ms: 10,
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"streaming_success\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::StreamingSuccess {
            chunks,
            chunk_delay_ms,
        } => {
            assert_eq!(chunks, vec!["a", "b"]);
            assert_eq!(chunk_delay_ms, 10);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn serde_transient_error_roundtrip() {
    let s = MockScenario::TransientError {
        fail_count: 5,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"transient_error\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::TransientError { fail_count, then } => {
            assert_eq!(fail_count, 5);
            assert!(matches!(*then, MockScenario::Success { .. }));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn serde_permanent_error_roundtrip() {
    let s = MockScenario::PermanentError {
        code: "ABP-B001".into(),
        message: "boom".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"permanent_error\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::PermanentError { code, message } => {
            assert_eq!(code, "ABP-B001");
            assert_eq!(message, "boom");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn serde_timeout_roundtrip() {
    let s = MockScenario::Timeout { after_ms: 500 };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"timeout\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::Timeout { after_ms } => assert_eq!(after_ms, 500),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn serde_rate_limited_roundtrip() {
    let s = MockScenario::RateLimited {
        retry_after_ms: 3000,
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"rate_limited\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::RateLimited { retry_after_ms } => assert_eq!(retry_after_ms, 3000),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn serde_nested_transient_error_roundtrip() {
    let s = MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::TransientError {
            fail_count: 2,
            then: Box::new(MockScenario::StreamingSuccess {
                chunks: vec!["x".into()],
                chunk_delay_ms: 5,
            }),
        }),
    };
    let json = serde_json::to_string(&s).unwrap();
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::TransientError { fail_count, then } => {
            assert_eq!(fail_count, 1);
            assert!(matches!(*then, MockScenario::TransientError { .. }));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn serde_scenario_from_json_literal() {
    let json = r#"{"type":"success","delay_ms":0,"text":"from json"}"#;
    let s: MockScenario = serde_json::from_str(json).unwrap();
    match s {
        MockScenario::Success { text, .. } => assert_eq!(text, "from json"),
        _ => panic!("wrong variant"),
    }
}

// ===========================================================================
// 16. Serde roundtrips – RecordedCall
// ===========================================================================

#[tokio::test]
async fn recorded_call_serializes_to_json() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = drive(&b, "ser-task").await.unwrap();
    let call = b.last_call().await.unwrap();
    let json = serde_json::to_value(&call).unwrap();
    assert!(json.get("work_order").is_some());
    assert!(json.get("timestamp").is_some());
    assert!(json.get("duration_ms").is_some());
    assert!(json.get("result").is_some());
}

#[tokio::test]
async fn recorded_call_roundtrip() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = drive(&b, "rt-task").await.unwrap();
    let call = b.last_call().await.unwrap();
    let json = serde_json::to_string(&call).unwrap();
    let d: RecordedCall = serde_json::from_str(&json).unwrap();
    assert_eq!(d.work_order.task, "rt-task");
    assert!(d.result.is_ok());
}

#[tokio::test]
async fn recorded_call_error_roundtrip() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "E".into(),
        message: "m".into(),
    });
    let _ = drive(&b, "t").await;
    let call = b.last_call().await.unwrap();
    let json = serde_json::to_string(&call).unwrap();
    let d: RecordedCall = serde_json::from_str(&json).unwrap();
    assert!(d.result.is_err());
}

// ===========================================================================
// 17. Capability requirements
// ===========================================================================

#[tokio::test]
async fn empty_requirements_pass() {
    let wo = make_wo_with_reqs("t", CapabilityRequirements { required: vec![] });
    let (r, _) = drive_wo(&MockBackend, wo).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn satisfied_requirement_streaming_native() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = make_wo_with_reqs("t", reqs);
    let r = drive_wo(&MockBackend, wo).await;
    assert!(r.is_ok());
}

#[tokio::test]
async fn satisfied_requirement_tool_read_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = make_wo_with_reqs("t", reqs);
    assert!(drive_wo(&MockBackend, wo).await.is_ok());
}

#[tokio::test]
async fn unsatisfied_requirement_rejects_run() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }],
    };
    let wo = make_wo_with_reqs("t", reqs);
    let err = drive_wo(&MockBackend, wo).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn native_requirement_on_emulated_capability_rejected() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = make_wo_with_reqs("t", reqs);
    let err = drive_wo(&MockBackend, wo).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn scenario_backend_rejects_unsatisfied_requirement() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }],
    };
    let wo = make_wo_with_reqs("t", reqs);
    assert!(drive_wo(&b, wo).await.is_err());
}

// ===========================================================================
// 18. Send + Sync assertions
// ===========================================================================

#[tokio::test]
async fn mock_backend_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

#[tokio::test]
async fn scenario_backend_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ScenarioMockBackend>();
}

#[tokio::test]
async fn recorder_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackendRecorder<MockBackend>>();
}

// ===========================================================================
// 19. dyn Backend trait object
// ===========================================================================

#[tokio::test]
async fn mock_as_dyn_backend() {
    let b: Box<dyn Backend> = Box::new(MockBackend);
    assert_eq!(b.identity().id, "mock");
}

#[tokio::test]
async fn scenario_as_dyn_backend() {
    let b: Box<dyn Backend> = Box::new(ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    }));
    assert_eq!(b.identity().id, "scenario-mock");
}

// ===========================================================================
// 20. Concurrent runs
// ===========================================================================

#[tokio::test]
async fn concurrent_mock_backend_runs() {
    let b = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..10 {
        let b = Arc::clone(&b);
        handles.push(tokio::spawn(async move {
            let wo = make_wo(&format!("concurrent-{i}"));
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo, tx).await.unwrap()
        }));
    }
    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }
    assert_eq!(results.len(), 10);
    for r in &results {
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn concurrent_scenario_runs_track_all_calls() {
    let b = Arc::new(ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    }));
    let mut handles = Vec::new();
    for i in 0..8 {
        let b = Arc::clone(&b);
        handles.push(tokio::spawn(async move {
            let wo = make_wo(&format!("c-{i}"));
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo, tx).await.unwrap()
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(b.call_count(), 8);
    assert_eq!(b.calls().await.len(), 8);
}

// ===========================================================================
// 21. Edge cases – special strings
// ===========================================================================

#[tokio::test]
async fn empty_task_string_succeeds() {
    let (r, _) = drive(&MockBackend, "").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn unicode_task_string() {
    let (r, _) = drive(&MockBackend, "日本語テスト 🚀").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn newline_in_task_string() {
    let (r, _) = drive(&MockBackend, "line1\nline2").await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn very_long_task_string() {
    let long = "x".repeat(10_000);
    let (r, _) = drive(&MockBackend, &long).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_with_empty_string_chunk() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["".into(), "notempty".into(), "".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas.len(), 3);
    assert_eq!(deltas[0], "");
    assert_eq!(deltas[1], "notempty");
}

#[tokio::test]
async fn success_text_with_unicode() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "こんにちは 🌍".into(),
    });
    let (_, events) = drive(&b, "t").await.unwrap();
    let msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(msgs.contains(&"こんにちは 🌍".to_string()));
}

// ===========================================================================
// 22. Channel behavior
// ===========================================================================

#[tokio::test]
async fn small_channel_buffer_completes() {
    let wo = make_wo("t");
    let (tx, mut rx) = mpsc::channel(1);
    // Drain the receiver concurrently to avoid blocking the sender
    let handle = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        events
    });
    let r = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
    let events = handle.await.unwrap();
    assert!(!events.is_empty());
}

#[tokio::test]
async fn dropped_receiver_does_not_panic() {
    let wo = make_wo("t");
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let r = MockBackend.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn scenario_dropped_receiver_does_not_panic() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into()],
        chunk_delay_ms: 0,
    });
    let wo = make_wo("t");
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let r = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

// ===========================================================================
// 23. Receipt hash behavior
// ===========================================================================

#[tokio::test]
async fn two_receipts_from_same_backend_have_different_hashes() {
    let (r1, _) = drive(&MockBackend, "t1").await.unwrap();
    let (r2, _) = drive(&MockBackend, "t2").await.unwrap();
    // Different timestamps/work_order_ids → different hashes
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn receipt_hash_present_on_scenario_success() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (r, _) = drive(&b, "t").await.unwrap();
    assert!(r.receipt_sha256.is_some());
    assert_eq!(r.receipt_sha256.as_ref().unwrap().len(), 64);
}

#[tokio::test]
async fn receipt_hash_present_on_streaming_success() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into()],
        chunk_delay_ms: 0,
    });
    let (r, _) = drive(&b, "t").await.unwrap();
    assert!(r.receipt_sha256.is_some());
}

// ===========================================================================
// 24. Debug impls
// ===========================================================================

#[tokio::test]
async fn scenario_mock_backend_debug() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let dbg = format!("{:?}", b);
    assert!(dbg.contains("ScenarioMockBackend"));
}

#[tokio::test]
async fn mock_scenario_debug() {
    let s = MockScenario::PermanentError {
        code: "E".into(),
        message: "m".into(),
    };
    let dbg = format!("{:?}", s);
    assert!(dbg.contains("PermanentError"));
}

#[tokio::test]
async fn recorded_call_debug() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = drive(&b, "t").await;
    let call = b.last_call().await.unwrap();
    let dbg = format!("{:?}", call);
    assert!(dbg.contains("RecordedCall"));
}

// ===========================================================================
// 25. Nested scenario – transient then permanent
// ===========================================================================

#[tokio::test]
async fn nested_transient_then_permanent() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::PermanentError {
            code: "P".into(),
            message: "perm".into(),
        }),
    });
    // First call: transient
    let e1 = drive(&b, "t").await.unwrap_err();
    assert!(e1.to_string().contains("transient"));
    // All subsequent: permanent
    let e2 = drive(&b, "t").await.unwrap_err();
    assert!(e2.to_string().contains("perm"));
    let e3 = drive(&b, "t").await.unwrap_err();
    assert!(e3.to_string().contains("perm"));
}

// ===========================================================================
// 26. Scenario MockScenario Clone
// ===========================================================================

#[test]
fn mock_scenario_clone() {
    let s = MockScenario::Success {
        delay_ms: 10,
        text: "cloned".into(),
    };
    let s2 = s.clone();
    match s2 {
        MockScenario::Success { delay_ms, text } => {
            assert_eq!(delay_ms, 10);
            assert_eq!(text, "cloned");
        }
        _ => panic!("wrong variant"),
    }
}
