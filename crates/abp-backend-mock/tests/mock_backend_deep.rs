#![allow(clippy::useless_vec)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use abp_backend_core::Backend;
use abp_backend_mock::MockBackend;
use abp_backend_mock::scenarios::{MockBackendRecorder, MockScenario, ScenarioMockBackend};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ExecutionMode, MinSupport, Outcome, Receipt, RuntimeConfig, SupportLevel, WorkOrder,
    WorkOrderBuilder, CONTRACT_VERSION,
};
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn simple_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

fn wo_with_vendor(task: &str, vendor: BTreeMap<String, serde_json::Value>) -> WorkOrder {
    let config = RuntimeConfig {
        vendor,
        ..Default::default()
    };
    WorkOrderBuilder::new(task).config(config).build()
}

fn wo_with_requirements(task: &str, reqs: CapabilityRequirements) -> WorkOrder {
    WorkOrderBuilder::new(task).requirements(reqs).build()
}

async fn run_backend_wo(
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

async fn run_simple(
    backend: &impl Backend,
    task: &str,
) -> anyhow::Result<(Receipt, Vec<AgentEvent>)> {
    run_backend_wo(backend, simple_wo(task)).await
}

fn count_kind(events: &[AgentEvent], pred: fn(&AgentEventKind) -> bool) -> usize {
    events.iter().filter(|e| pred(&e.kind)).count()
}

fn is_run_started(k: &AgentEventKind) -> bool {
    matches!(k, AgentEventKind::RunStarted { .. })
}

fn is_run_completed(k: &AgentEventKind) -> bool {
    matches!(k, AgentEventKind::RunCompleted { .. })
}

fn is_assistant_message(k: &AgentEventKind) -> bool {
    matches!(k, AgentEventKind::AssistantMessage { .. })
}

fn is_assistant_delta(k: &AgentEventKind) -> bool {
    matches!(k, AgentEventKind::AssistantDelta { .. })
}

// ===========================================================================
// Module 1: MockBackend creation and identity
// ===========================================================================

#[tokio::test]
async fn mock_backend_identity_id() {
    let b = MockBackend;
    assert_eq!(b.identity().id, "mock");
}

#[tokio::test]
async fn mock_backend_identity_backend_version() {
    let b = MockBackend;
    assert_eq!(b.identity().backend_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn mock_backend_identity_adapter_version() {
    let b = MockBackend;
    assert_eq!(b.identity().adapter_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn mock_backend_capabilities_has_streaming() {
    let b = MockBackend;
    let caps = b.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[tokio::test]
async fn mock_backend_capabilities_has_tool_read() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_backend_capabilities_has_tool_write() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_backend_capabilities_has_tool_edit() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_backend_capabilities_has_tool_bash() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_backend_capabilities_has_structured_output() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

#[tokio::test]
async fn mock_backend_capabilities_count() {
    let caps = MockBackend.capabilities();
    assert_eq!(caps.len(), 6); // Streaming + ToolRead + ToolWrite + ToolEdit + ToolBash + StructuredOutput
}

#[tokio::test]
async fn mock_backend_is_clone() {
    let b = MockBackend;
    let b2 = b.clone();
    assert_eq!(b2.identity().id, "mock");
}

// ===========================================================================
// Module 2: MockBackend basic run
// ===========================================================================

#[tokio::test]
async fn mock_run_returns_complete_outcome() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn mock_run_receipt_has_sha256() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn mock_run_receipt_sha256_is_64_hex_chars() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn mock_run_receipt_contract_version() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn mock_run_receipt_backend_identity() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn mock_run_receipt_default_execution_mode() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn mock_run_receipt_work_order_id_matches() {
    let wo = simple_wo("test");
    let expected_id = wo.id;
    let (receipt, _) = run_backend_wo(&MockBackend, wo).await.unwrap();
    assert_eq!(receipt.meta.work_order_id, expected_id);
}

#[tokio::test]
async fn mock_run_receipt_duration_is_plausible() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(receipt.meta.duration_ms < 5000);
}

#[tokio::test]
async fn mock_run_receipt_started_before_finished() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(receipt.meta.started_at <= receipt.meta.finished_at);
}

#[tokio::test]
async fn mock_run_receipt_zero_usage() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn mock_run_receipt_verification_harness_ok() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn mock_run_receipt_no_artifacts() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(receipt.artifacts.is_empty());
}

#[tokio::test]
async fn mock_run_receipt_trace_has_four_events() {
    let (receipt, _) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(receipt.trace.len(), 4);
}

// ===========================================================================
// Module 3: MockBackend event stream ordering and completeness
// ===========================================================================

#[tokio::test]
async fn mock_run_events_count_matches_trace() {
    let (receipt, events) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(events.len(), receipt.trace.len());
}

#[tokio::test]
async fn mock_run_first_event_is_run_started() {
    let (_, events) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(is_run_started(&events[0].kind));
}

#[tokio::test]
async fn mock_run_last_event_is_run_completed() {
    let (_, events) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(is_run_completed(&events.last().unwrap().kind));
}

#[tokio::test]
async fn mock_run_exactly_one_run_started() {
    let (_, events) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(count_kind(&events, is_run_started), 1);
}

#[tokio::test]
async fn mock_run_exactly_one_run_completed() {
    let (_, events) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(count_kind(&events, is_run_completed), 1);
}

#[tokio::test]
async fn mock_run_two_assistant_messages() {
    let (_, events) = run_simple(&MockBackend, "test").await.unwrap();
    assert_eq!(count_kind(&events, is_assistant_message), 2);
}

#[tokio::test]
async fn mock_run_event_timestamps_are_non_decreasing() {
    let (_, events) = run_simple(&MockBackend, "test").await.unwrap();
    for pair in events.windows(2) {
        assert!(pair[0].ts <= pair[1].ts);
    }
}

#[tokio::test]
async fn mock_run_events_all_have_no_ext() {
    let (_, events) = run_simple(&MockBackend, "test").await.unwrap();
    assert!(events.iter().all(|e| e.ext.is_none()));
}

#[tokio::test]
async fn mock_run_started_message_contains_task() {
    let (_, events) = run_simple(&MockBackend, "my special task").await.unwrap();
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("my special task"));
    } else {
        panic!("first event should be RunStarted");
    }
}

// ===========================================================================
// Module 4: MockBackend with passthrough mode
// ===========================================================================

#[tokio::test]
async fn mock_run_passthrough_mode_via_nested_vendor() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp".to_string(),
        serde_json::json!({"mode": "passthrough"}),
    );
    let wo = wo_with_vendor("pass-test", vendor);
    let (receipt, _) = run_backend_wo(&MockBackend, wo).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn mock_run_passthrough_mode_via_flat_vendor() {
    let mut vendor = BTreeMap::new();
    vendor.insert(
        "abp.mode".to_string(),
        serde_json::json!("passthrough"),
    );
    let wo = wo_with_vendor("pass-test2", vendor);
    let (receipt, _) = run_backend_wo(&MockBackend, wo).await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[tokio::test]
async fn mock_run_mapped_mode_is_default() {
    let (receipt, _) = run_simple(&MockBackend, "default-mode").await.unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

// ===========================================================================
// Module 5: Receipt hash integrity
// ===========================================================================

#[tokio::test]
async fn receipt_hash_is_deterministic_for_same_receipt() {
    // Two identical receipts from the same work order should yield same hash
    // when trace timestamps are the same. Since we can't guarantee that,
    // just verify the hash recomputes correctly.
    let (receipt, _) = run_simple(&MockBackend, "hash-test").await.unwrap();
    let hash1 = receipt.receipt_sha256.clone().unwrap();

    // Recompute by re-hashing
    let mut receipt2 = receipt;
    receipt2.receipt_sha256 = None;
    let receipt2 = receipt2.with_hash().unwrap();
    let hash2 = receipt2.receipt_sha256.unwrap();
    assert_eq!(hash1, hash2);
}

#[tokio::test]
async fn receipt_hash_changes_with_different_outcome() {
    let (mut receipt, _) = run_simple(&MockBackend, "hash-diff").await.unwrap();
    let hash1 = receipt.receipt_sha256.clone().unwrap();
    receipt.outcome = Outcome::Failed;
    receipt.receipt_sha256 = None;
    let receipt = receipt.with_hash().unwrap();
    let hash2 = receipt.receipt_sha256.unwrap();
    assert_ne!(hash1, hash2);
}

#[tokio::test]
async fn receipt_hash_with_null_field_does_not_panic() {
    let (receipt, _) = run_simple(&MockBackend, "null-hash").await.unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

// ===========================================================================
// Module 6: ScenarioMockBackend - Success scenario
// ===========================================================================

#[tokio::test]
async fn scenario_success_returns_complete() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "hello world".into(),
    });
    let (receipt, _) = run_simple(&b, "s1").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn scenario_success_emits_correct_text() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "expected text".into(),
    });
    let (_, events) = run_simple(&b, "s2").await.unwrap();
    let msgs: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(msgs.contains(&"expected text"));
}

#[tokio::test]
async fn scenario_success_has_three_events() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (_, events) = run_simple(&b, "s3").await.unwrap();
    // RunStarted + AssistantMessage + RunCompleted
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn scenario_success_delay_observed() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 80,
        text: "delayed".into(),
    });
    let start = Instant::now();
    let _ = run_simple(&b, "s4").await.unwrap();
    assert!(start.elapsed().as_millis() >= 60);
}

#[tokio::test]
async fn scenario_success_zero_delay_is_fast() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "fast".into(),
    });
    let start = Instant::now();
    let _ = run_simple(&b, "s5").await.unwrap();
    assert!(start.elapsed().as_millis() < 500);
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
async fn scenario_success_capabilities_same_as_mock() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let mock_caps = MockBackend.capabilities();
    // SupportLevel doesn't impl PartialEq, compare via debug
    assert_eq!(format!("{mock_caps:?}"), format!("{:?}", b.capabilities()));
}

#[tokio::test]
async fn scenario_success_receipt_has_valid_hash() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (receipt, _) = run_simple(&b, "s6").await.unwrap();
    let hash = receipt.receipt_sha256.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn scenario_success_receipt_backend_is_scenario_mock() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (receipt, _) = run_simple(&b, "s7").await.unwrap();
    assert_eq!(receipt.backend.id, "scenario-mock");
}

#[tokio::test]
async fn scenario_success_run_started_contains_task() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (_, events) = run_simple(&b, "important-task").await.unwrap();
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("important-task"));
    } else {
        panic!("expected RunStarted");
    }
}

// ===========================================================================
// Module 7: ScenarioMockBackend - StreamingSuccess scenario
// ===========================================================================

#[tokio::test]
async fn streaming_emits_all_chunks_in_order() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["alpha".into(), "beta".into(), "gamma".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = run_simple(&b, "stream").await.unwrap();
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["alpha", "beta", "gamma"]);
}

#[tokio::test]
async fn streaming_event_count_equals_chunks_plus_two() {
    let chunks = vec!["a".into(), "b".into(), "c".into(), "d".into()];
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: chunks.clone(),
        chunk_delay_ms: 0,
    });
    let (_, events) = run_simple(&b, "stream-count").await.unwrap();
    // RunStarted + N chunks + RunCompleted
    assert_eq!(events.len(), chunks.len() + 2);
}

#[tokio::test]
async fn streaming_empty_chunks_yields_only_lifecycle_events() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec![],
        chunk_delay_ms: 0,
    });
    let (_, events) = run_simple(&b, "empty-stream").await.unwrap();
    assert_eq!(events.len(), 2); // RunStarted + RunCompleted
    assert!(is_run_started(&events[0].kind));
    assert!(is_run_completed(&events[1].kind));
}

#[tokio::test]
async fn streaming_single_chunk() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["only".into()],
        chunk_delay_ms: 0,
    });
    let (_, events) = run_simple(&b, "single-chunk").await.unwrap();
    assert_eq!(count_kind(&events, is_assistant_delta), 1);
}

#[tokio::test]
async fn streaming_with_delay_takes_expected_time() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into(), "c".into()],
        chunk_delay_ms: 30,
    });
    let start = Instant::now();
    let _ = run_simple(&b, "timed-stream").await.unwrap();
    // 3 chunks * 30ms = ~90ms
    assert!(start.elapsed().as_millis() >= 70);
}

#[tokio::test]
async fn streaming_receipt_trace_matches_events() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into()],
        chunk_delay_ms: 0,
    });
    let (receipt, events) = run_simple(&b, "trace-match").await.unwrap();
    assert_eq!(receipt.trace.len(), events.len());
}

#[tokio::test]
async fn streaming_receipt_outcome_complete() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["chunk".into()],
        chunk_delay_ms: 0,
    });
    let (receipt, _) = run_simple(&b, "outcome").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_many_chunks() {
    let chunks: Vec<String> = (0..100).map(|i| format!("chunk-{i}")).collect();
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: chunks.clone(),
        chunk_delay_ms: 0,
    });
    let (_, events) = run_simple(&b, "many-chunks").await.unwrap();
    assert_eq!(count_kind(&events, is_assistant_delta), 100);
}

// ===========================================================================
// Module 8: ScenarioMockBackend - TransientError scenario
// ===========================================================================

#[tokio::test]
async fn transient_error_fails_exact_count_times() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 3,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    assert!(run_simple(&b, "t1").await.is_err());
    assert!(run_simple(&b, "t2").await.is_err());
    assert!(run_simple(&b, "t3").await.is_err());
    let (receipt, _) = run_simple(&b, "t4").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(b.call_count(), 4);
}

#[tokio::test]
async fn transient_error_zero_fail_count_succeeds_immediately() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 0,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "immediate".into(),
        }),
    });
    let (receipt, _) = run_simple(&b, "t-zero").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn transient_error_message_includes_attempt_number() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 2,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    let err1 = run_simple(&b, "t-msg1").await.unwrap_err();
    assert!(err1.to_string().contains("1/2"));
    let err2 = run_simple(&b, "t-msg2").await.unwrap_err();
    assert!(err2.to_string().contains("2/2"));
}

#[tokio::test]
async fn transient_error_then_streaming_success() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::StreamingSuccess {
            chunks: vec!["recovered".into()],
            chunk_delay_ms: 0,
        }),
    });
    assert!(run_simple(&b, "te-fail").await.is_err());
    let (_, events) = run_simple(&b, "te-ok").await.unwrap();
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["recovered"]);
}

#[tokio::test]
async fn transient_error_records_failures_in_calls() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 2,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    let _ = run_simple(&b, "rec1").await;
    let _ = run_simple(&b, "rec2").await;
    let _ = run_simple(&b, "rec3").await;
    let calls = b.calls().await;
    assert_eq!(calls.len(), 3);
    assert!(calls[0].result.is_err());
    assert!(calls[1].result.is_err());
    assert!(calls[2].result.is_ok());
}

#[tokio::test]
async fn transient_error_last_error_is_set_on_failure() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    });
    let _ = run_simple(&b, "le-fail").await;
    let err = b.last_error().await;
    assert!(err.is_some());
    assert!(err.unwrap().contains("transient"));
}

// ===========================================================================
// Module 9: ScenarioMockBackend - PermanentError scenario
// ===========================================================================

#[tokio::test]
async fn permanent_error_always_fails() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "ERR-42".into(),
        message: "permanent failure".into(),
    });
    for i in 0..5 {
        let err = run_simple(&b, &format!("pe-{i}")).await.unwrap_err();
        assert!(err.to_string().contains("ERR-42"));
        assert!(err.to_string().contains("permanent failure"));
    }
    assert_eq!(b.call_count(), 5);
}

#[tokio::test]
async fn permanent_error_sets_last_error() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "X".into(),
        message: "boom".into(),
    });
    let _ = run_simple(&b, "pe-le").await;
    let err = b.last_error().await.unwrap();
    assert!(err.contains("boom"));
}

#[tokio::test]
async fn permanent_error_records_all_calls() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "Z".into(),
        message: "no".into(),
    });
    let _ = run_simple(&b, "pe1").await;
    let _ = run_simple(&b, "pe2").await;
    let calls = b.calls().await;
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.result.is_err()));
}

// ===========================================================================
// Module 10: ScenarioMockBackend - Timeout scenario
// ===========================================================================

#[tokio::test]
async fn timeout_fails_after_specified_delay() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 60 });
    let start = Instant::now();
    let err = run_simple(&b, "timeout").await.unwrap_err();
    assert!(start.elapsed().as_millis() >= 40);
    assert!(err.to_string().contains("timeout"));
}

#[tokio::test]
async fn timeout_error_message_includes_duration() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 123 });
    let err = run_simple(&b, "to-msg").await.unwrap_err();
    assert!(err.to_string().contains("123"));
}

#[tokio::test]
async fn timeout_zero_ms_fails_immediately() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 0 });
    let start = Instant::now();
    let err = run_simple(&b, "to-zero").await.unwrap_err();
    assert!(start.elapsed().as_millis() < 500);
    assert!(err.to_string().contains("timeout"));
}

#[tokio::test]
async fn timeout_records_call() {
    let b = ScenarioMockBackend::new(MockScenario::Timeout { after_ms: 10 });
    let _ = run_simple(&b, "to-rec").await;
    let last = b.last_call().await.unwrap();
    assert!(last.result.is_err());
}

// ===========================================================================
// Module 11: ScenarioMockBackend - RateLimited scenario
// ===========================================================================

#[tokio::test]
async fn rate_limited_fails_with_retry_info() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 2000,
    });
    let err = run_simple(&b, "rl").await.unwrap_err();
    assert!(err.to_string().contains("rate limited"));
    assert!(err.to_string().contains("2000"));
}

#[tokio::test]
async fn rate_limited_returns_immediately() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 10000,
    });
    let start = Instant::now();
    let _ = run_simple(&b, "rl-fast").await;
    assert!(start.elapsed().as_millis() < 500);
}

#[tokio::test]
async fn rate_limited_records_call_as_error() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 100,
    });
    let _ = run_simple(&b, "rl-rec").await;
    let call = b.last_call().await.unwrap();
    assert!(call.result.is_err());
}

#[tokio::test]
async fn rate_limited_call_count_increments() {
    let b = ScenarioMockBackend::new(MockScenario::RateLimited {
        retry_after_ms: 100,
    });
    let _ = run_simple(&b, "rl-cc1").await;
    let _ = run_simple(&b, "rl-cc2").await;
    assert_eq!(b.call_count(), 2);
}

// ===========================================================================
// Module 12: ScenarioMockBackend - call_count and recording
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
async fn calls_list_starts_empty() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert!(b.calls().await.is_empty());
}

#[tokio::test]
async fn last_call_is_none_initially() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert!(b.last_call().await.is_none());
}

#[tokio::test]
async fn last_error_is_none_initially() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    assert!(b.last_error().await.is_none());
}

#[tokio::test]
async fn recorded_call_has_correct_work_order_task() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = run_simple(&b, "recorded-task").await.unwrap();
    let call = b.last_call().await.unwrap();
    assert_eq!(call.work_order.task, "recorded-task");
}

#[tokio::test]
async fn recorded_call_has_plausible_duration() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 50,
        text: "x".into(),
    });
    let _ = run_simple(&b, "dur").await.unwrap();
    let call = b.last_call().await.unwrap();
    assert!(call.duration_ms >= 30);
}

#[tokio::test]
async fn recorded_call_result_is_ok_for_success() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = run_simple(&b, "ok-result").await.unwrap();
    let call = b.last_call().await.unwrap();
    assert_eq!(call.result, Ok(Outcome::Complete));
}

// ===========================================================================
// Module 13: MockBackendRecorder
// ===========================================================================

#[tokio::test]
async fn recorder_records_mock_backend_calls() {
    let recorder = MockBackendRecorder::new(MockBackend);
    let _ = run_simple(&recorder, "r1").await.unwrap();
    assert_eq!(recorder.call_count().await, 1);
}

#[tokio::test]
async fn recorder_identity_delegates_to_inner() {
    let recorder = MockBackendRecorder::new(MockBackend);
    assert_eq!(recorder.identity().id, "mock");
}

#[tokio::test]
async fn recorder_capabilities_delegate_to_inner() {
    let recorder = MockBackendRecorder::new(MockBackend);
    assert_eq!(
        format!("{:?}", recorder.capabilities()),
        format!("{:?}", MockBackend.capabilities())
    );
}

#[tokio::test]
async fn recorder_preserves_work_order_task() {
    let recorder = MockBackendRecorder::new(MockBackend);
    let _ = run_simple(&recorder, "preserved-task").await.unwrap();
    let call = recorder.last_call().await.unwrap();
    assert_eq!(call.work_order.task, "preserved-task");
}

#[tokio::test]
async fn recorder_last_call_is_none_initially() {
    let recorder = MockBackendRecorder::new(MockBackend);
    assert!(recorder.last_call().await.is_none());
}

#[tokio::test]
async fn recorder_tracks_multiple_calls() {
    let recorder = MockBackendRecorder::new(MockBackend);
    for i in 0..10 {
        let _ = run_simple(&recorder, &format!("task-{i}")).await.unwrap();
    }
    assert_eq!(recorder.call_count().await, 10);
    let calls = recorder.calls().await;
    assert_eq!(calls.len(), 10);
}

#[tokio::test]
async fn recorder_wrapping_scenario_records_errors() {
    let scenario = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "ERR".into(),
        message: "fail".into(),
    });
    let recorder = MockBackendRecorder::new(scenario);
    let _ = run_simple(&recorder, "rec-err").await;
    let call = recorder.last_call().await.unwrap();
    assert!(call.result.is_err());
}

#[tokio::test]
async fn recorder_clone_shares_call_list() {
    let recorder = MockBackendRecorder::new(MockBackend);
    let recorder2 = recorder.clone();
    let _ = run_simple(&recorder, "shared").await.unwrap();
    assert_eq!(recorder2.call_count().await, 1);
}

// ===========================================================================
// Module 14: ScenarioMockBackend Clone behavior
// ===========================================================================

#[tokio::test]
async fn scenario_clone_shares_calls_arc() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let b2 = b.clone();
    let _ = run_simple(&b, "clone-test").await.unwrap();
    assert_eq!(b2.calls().await.len(), 1);
}

#[tokio::test]
async fn scenario_clone_shares_last_error() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "X".into(),
        message: "shared-err".into(),
    });
    let b2 = b.clone();
    let _ = run_simple(&b, "err").await;
    let err = b2.last_error().await;
    assert!(err.is_some());
    assert!(err.unwrap().contains("shared-err"));
}

#[tokio::test]
async fn scenario_clone_copies_call_count() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let _ = run_simple(&b, "pre-clone").await.unwrap();
    let b2 = b.clone();
    // Clone copies the atomic count at the point of clone
    assert_eq!(b2.call_count(), 1);
}

// ===========================================================================
// Module 15: Serialization round-trips
// ===========================================================================

#[tokio::test]
async fn scenario_success_serialization_round_trip() {
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

#[tokio::test]
async fn scenario_streaming_serialization_round_trip() {
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

#[tokio::test]
async fn scenario_permanent_error_serialization_round_trip() {
    let s = MockScenario::PermanentError {
        code: "E99".into(),
        message: "bad".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"permanent_error\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::PermanentError { code, message } => {
            assert_eq!(code, "E99");
            assert_eq!(message, "bad");
        }
        _ => panic!("wrong variant"),
    }
}

#[tokio::test]
async fn scenario_timeout_serialization_round_trip() {
    let s = MockScenario::Timeout { after_ms: 500 };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"type\":\"timeout\""));
    let d: MockScenario = serde_json::from_str(&json).unwrap();
    match d {
        MockScenario::Timeout { after_ms } => assert_eq!(after_ms, 500),
        _ => panic!("wrong variant"),
    }
}

#[tokio::test]
async fn scenario_rate_limited_serialization_round_trip() {
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

#[tokio::test]
async fn scenario_transient_error_serialization_round_trip() {
    let s = MockScenario::TransientError {
        fail_count: 5,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "recovered".into(),
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

#[tokio::test]
async fn recorded_call_serializes_to_json() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "ser-test".into(),
    });
    let _ = run_simple(&b, "ser").await.unwrap();
    let call = b.last_call().await.unwrap();
    let json = serde_json::to_value(&call).unwrap();
    assert!(json.get("work_order").is_some());
    assert!(json.get("timestamp").is_some());
    assert!(json.get("duration_ms").is_some());
    assert!(json.get("result").is_some());
}

// ===========================================================================
// Module 16: Concurrent mock runs
// ===========================================================================

#[tokio::test]
async fn concurrent_runs_on_mock_backend() {
    let b = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..10 {
        let b = Arc::clone(&b);
        handles.push(tokio::spawn(async move {
            run_simple(&*b, &format!("concurrent-{i}")).await
        }));
    }
    for h in handles {
        let result = h.await.unwrap();
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn concurrent_runs_on_scenario_backend() {
    let b = Arc::new(ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 5,
        text: "concurrent".into(),
    }));
    let mut handles = Vec::new();
    for i in 0..10 {
        let b = Arc::clone(&b);
        handles.push(tokio::spawn(async move {
            run_simple(&*b, &format!("sc-{i}")).await
        }));
    }
    for h in handles {
        let result = h.await.unwrap();
        assert!(result.is_ok());
    }
    assert_eq!(b.call_count(), 10);
}

#[tokio::test]
async fn concurrent_recorder_calls_are_all_tracked() {
    let recorder = Arc::new(MockBackendRecorder::new(MockBackend));
    let mut handles = Vec::new();
    for i in 0..20 {
        let r = Arc::clone(&recorder);
        handles.push(tokio::spawn(async move {
            run_simple(&*r, &format!("rec-{i}")).await
        }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }
    assert_eq!(recorder.call_count().await, 20);
}

#[tokio::test]
async fn concurrent_transient_errors_track_correctly() {
    let b = Arc::new(ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 5,
        then: Box::new(MockScenario::Success {
            delay_ms: 0,
            text: "ok".into(),
        }),
    }));
    let mut handles = Vec::new();
    for i in 0..10 {
        let b = Arc::clone(&b);
        handles.push(tokio::spawn(async move {
            run_simple(&*b, &format!("ct-{i}")).await
        }));
    }
    let mut successes = 0;
    let mut failures = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }
    // At least some should fail and some succeed (exact split depends on scheduling)
    assert_eq!(successes + failures, 10);
    assert_eq!(b.call_count(), 10);
}

// ===========================================================================
// Module 17: Edge cases - empty task, special characters, large payloads
// ===========================================================================

#[tokio::test]
async fn empty_task_string() {
    let (receipt, events) = run_simple(&MockBackend, "").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert!(!events.is_empty());
}

#[tokio::test]
async fn task_with_special_characters() {
    let task = r#"fix "bug" in <module> & deploy — «test» 日本語 🚀"#;
    let (receipt, _) = run_simple(&MockBackend, task).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn task_with_newlines() {
    let task = "line1\nline2\nline3";
    let (receipt, _) = run_simple(&MockBackend, task).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn task_with_very_long_string() {
    let task = "x".repeat(10_000);
    let (receipt, _) = run_simple(&MockBackend, &task).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn streaming_chunk_with_empty_string() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["".into(), "".into()],
        chunk_delay_ms: 0,
    });
    let (receipt, events) = run_simple(&b, "empty-chunks").await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(count_kind(&events, is_assistant_delta), 2);
}

#[tokio::test]
async fn streaming_chunk_with_special_characters() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec![
            "Hello 🌍".into(),
            "café résumé".into(),
            "<script>alert('xss')</script>".into(),
        ],
        chunk_delay_ms: 0,
    });
    let (_, events) = run_simple(&b, "special-chunks").await.unwrap();
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas.len(), 3);
    assert_eq!(deltas[0], "Hello 🌍");
    assert_eq!(deltas[2], "<script>alert('xss')</script>");
}

#[tokio::test]
async fn success_text_with_unicode() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "こんにちは世界 🎉".into(),
    });
    let (_, events) = run_simple(&b, "unicode").await.unwrap();
    let msgs: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(msgs.contains(&"こんにちは世界 🎉"));
}

#[tokio::test]
async fn permanent_error_with_special_chars_in_message() {
    let b = ScenarioMockBackend::new(MockScenario::PermanentError {
        code: "ERR-™".into(),
        message: "error with \"quotes\" & <angle brackets>".into(),
    });
    let err = run_simple(&b, "special-err").await.unwrap_err();
    assert!(err.to_string().contains("ERR-™"));
}

// ===========================================================================
// Module 18: Capability requirements checking
// ===========================================================================

#[tokio::test]
async fn satisfied_requirement_streaming_native() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let wo = wo_with_requirements("cap-test", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn satisfied_requirement_tool_read_emulated() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = wo_with_requirements("cap-read", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn unsatisfied_requirement_rejects_run() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Native,
        }],
    };
    let wo = wo_with_requirements("cap-reject", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("capability requirements not satisfied"));
}

#[tokio::test]
async fn unsatisfied_requirement_on_scenario_backend() {
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
    let wo = wo_with_requirements("cap-scenario", reqs);
    let result = run_backend_wo(&b, wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn unsatisfied_requirement_streaming_for_scenario_streaming() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into()],
        chunk_delay_ms: 0,
    });
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Vision,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = wo_with_requirements("cap-stream-reject", reqs);
    let result = run_backend_wo(&b, wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn emulated_requirement_on_native_capability_is_satisfied() {
    // Streaming is Native, requiring Emulated should still pass
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let wo = wo_with_requirements("cap-native-as-emulated", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn native_requirement_on_emulated_capability_is_rejected() {
    // ToolRead is Emulated, requiring Native should fail
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let wo = wo_with_requirements("cap-emulated-native", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn multiple_satisfied_requirements() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::ToolWrite,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let wo = wo_with_requirements("multi-cap", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn one_unsatisfied_among_multiple_requirements_rejects() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            },
            CapabilityRequirement {
                capability: Capability::Vision, // not supported
                min_support: MinSupport::Native,
            },
        ],
    };
    let wo = wo_with_requirements("mixed-cap", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn empty_requirements_always_satisfied() {
    let reqs = CapabilityRequirements {
        required: vec![],
    };
    let wo = wo_with_requirements("no-req", reqs);
    let result = run_backend_wo(&MockBackend, wo).await;
    assert!(result.is_ok());
}

// ===========================================================================
// Module 19: Backend trait compliance
// ===========================================================================

#[tokio::test]
async fn mock_backend_implements_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

#[tokio::test]
async fn scenario_backend_implements_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ScenarioMockBackend>();
}

#[tokio::test]
async fn recorder_implements_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackendRecorder<MockBackend>>();
}

#[tokio::test]
async fn mock_backend_can_be_used_as_dyn_backend() {
    let b: Box<dyn Backend> = Box::new(MockBackend);
    let id = b.identity();
    assert_eq!(id.id, "mock");
}

#[tokio::test]
async fn scenario_backend_can_be_used_as_dyn_backend() {
    let b: Box<dyn Backend> = Box::new(ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "dyn".into(),
    }));
    let id = b.identity();
    assert_eq!(id.id, "scenario-mock");
}

#[tokio::test]
async fn recorder_can_be_used_as_dyn_backend() {
    let b: Box<dyn Backend> = Box::new(MockBackendRecorder::new(MockBackend));
    let id = b.identity();
    assert_eq!(id.id, "mock");
}

// ===========================================================================
// Module 20: Channel behavior
// ===========================================================================

#[tokio::test]
async fn small_channel_buffer_still_completes() {
    let b = MockBackend;
    let wo = simple_wo("small-buf");
    // Use a large enough buffer to avoid backpressure deadlock (4 events emitted)
    let (tx, mut rx) = mpsc::channel(4);
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(!events.is_empty());
}

#[tokio::test]
async fn large_channel_buffer_works() {
    let b = MockBackend;
    let wo = simple_wo("large-buf");
    let (tx, mut rx) = mpsc::channel(1024);
    let receipt = b.run(Uuid::new_v4(), wo, tx).await.unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn dropped_receiver_does_not_panic() {
    let b = MockBackend;
    let wo = simple_wo("dropped-rx");
    let (tx, rx) = mpsc::channel(16);
    drop(rx);
    // Backend should not panic even if receiver is dropped
    let result = b.run(Uuid::new_v4(), wo, tx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn scenario_streaming_with_dropped_receiver() {
    let b = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
        chunks: vec!["a".into(), "b".into(), "c".into()],
        chunk_delay_ms: 0,
    });
    let wo = simple_wo("dropped-stream-rx");
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let result = b.run(Uuid::new_v4(), wo, tx).await;
    // Should complete without panic (send errors are silently ignored)
    assert!(result.is_ok());
}

// ===========================================================================
// Module 21: Run ID propagation
// ===========================================================================

#[tokio::test]
async fn run_id_propagated_to_receipt() {
    let run_id = Uuid::new_v4();
    let wo = simple_wo("run-id-test");
    let (tx, _rx) = mpsc::channel(16);
    let receipt = MockBackend.run(run_id, wo, tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn scenario_run_id_propagated_to_receipt() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let run_id = Uuid::new_v4();
    let wo = simple_wo("scenario-run-id");
    let (tx, _rx) = mpsc::channel(16);
    let receipt = b.run(run_id, wo, tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn different_run_ids_yield_different_receipts() {
    let b = MockBackend;
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let (tx1, _) = mpsc::channel(16);
    let (tx2, _) = mpsc::channel(16);
    let r1 = b.run(id1, simple_wo("t1"), tx1).await.unwrap();
    let r2 = b.run(id2, simple_wo("t2"), tx2).await.unwrap();
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
    // Hashes should differ because run_id differs
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

// ===========================================================================
// Module 22: Nested transient scenarios
// ===========================================================================

#[tokio::test]
async fn nested_transient_then_permanent_error() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 1,
        then: Box::new(MockScenario::PermanentError {
            code: "PERM".into(),
            message: "permanent after transient".into(),
        }),
    });
    // First call: transient
    let err1 = run_simple(&b, "n1").await.unwrap_err();
    assert!(err1.to_string().contains("transient"));
    // Subsequent calls: permanent error
    let err2 = run_simple(&b, "n2").await.unwrap_err();
    assert!(err2.to_string().contains("PERM"));
    let err3 = run_simple(&b, "n3").await.unwrap_err();
    assert!(err3.to_string().contains("PERM"));
}

#[tokio::test]
async fn nested_transient_then_streaming() {
    let b = ScenarioMockBackend::new(MockScenario::TransientError {
        fail_count: 2,
        then: Box::new(MockScenario::StreamingSuccess {
            chunks: vec!["streamed".into()],
            chunk_delay_ms: 0,
        }),
    });
    let _ = run_simple(&b, "fail1").await; // transient
    let _ = run_simple(&b, "fail2").await; // transient
    let (_, events) = run_simple(&b, "ok").await.unwrap();
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["streamed"]);
}

// ===========================================================================
// Module 23: Receipt usage_raw field
// ===========================================================================

#[tokio::test]
async fn mock_receipt_usage_raw_is_mock_note() {
    let (receipt, _) = run_simple(&MockBackend, "usage-raw").await.unwrap();
    assert_eq!(receipt.usage_raw, serde_json::json!({"note": "mock"}));
}

#[tokio::test]
async fn scenario_receipt_usage_raw_is_scenario_mock_note() {
    let b = ScenarioMockBackend::new(MockScenario::Success {
        delay_ms: 0,
        text: "x".into(),
    });
    let (receipt, _) = run_simple(&b, "usage-scenario").await.unwrap();
    assert_eq!(
        receipt.usage_raw,
        serde_json::json!({"note": "scenario-mock"})
    );
}
