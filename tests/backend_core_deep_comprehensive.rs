// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive deep tests for `abp-backend-core` and `abp-backend-mock`.
//!
//! 150+ tests covering: Backend trait implementation, MockBackend config/behavior,
//! event streaming, receipt generation, backend selection/registry, capability
//! declaration, error handling, identity/metadata, SidecarBackend basics, lifecycle,
//! config validation, timeout handling, concurrent execution, event ordering,
//! and usage reporting.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use abp_backend_core::{
    Backend, ensure_capability_requirements, extract_execution_mode,
    validate_passthrough_compatibility,
};
use abp_backend_mock::MockBackend;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ContextPacket, ContextSnippet, ExecutionMode,
    MinSupport, Outcome, PolicyProfile, Receipt, SupportLevel, WorkOrder, WorkOrderBuilder,
    receipt_hash,
};
use abp_host::SidecarSpec;
use abp_integrations::capability::CapabilityMatrix;
use abp_integrations::health::{HealthChecker, HealthStatus};
use abp_integrations::metrics::{BackendMetrics, MetricsRegistry};
use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

async fn run_collect(task: &str) -> (Receipt, Vec<AgentEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend.run(Uuid::new_v4(), wo(task), tx).await.unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
}

async fn run_receipt(task: &str) -> Receipt {
    let (tx, _rx) = mpsc::channel(64);
    MockBackend.run(Uuid::new_v4(), wo(task), tx).await.unwrap()
}

async fn run_with_id(run_id: Uuid, work_order: WorkOrder) -> (Receipt, Vec<AgentEvent>) {
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = MockBackend.run(run_id, work_order, tx).await.unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    (receipt, events)
}

fn mock_candidate(name: &str, caps: Vec<Capability>, priority: u32) -> BackendCandidate {
    BackendCandidate {
        name: name.to_string(),
        capabilities: caps,
        priority,
        enabled: true,
        metadata: BTreeMap::new(),
    }
}

// ===========================================================================
// Section 1 – Backend trait implementation validation (t001–t015)
// ===========================================================================

#[test]
fn t001_backend_trait_is_object_safe() {
    let _: Box<dyn Backend> = Box::new(MockBackend);
}

#[test]
fn t002_backend_trait_identity_via_dyn() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let id = backend.identity();
    assert_eq!(id.id, "mock");
}

#[test]
fn t003_backend_trait_capabilities_via_dyn() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let caps = backend.capabilities();
    assert!(!caps.is_empty());
}

#[tokio::test]
async fn t004_backend_trait_run_via_dyn() {
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _rx) = mpsc::channel(64);
    let receipt = backend
        .run(Uuid::new_v4(), wo("dyn test"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[test]
fn t005_backend_trait_arc_dyn() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    assert_eq!(backend.identity().id, "mock");
}

#[test]
fn t006_backend_trait_vec_of_dyn() {
    let backends: Vec<Box<dyn Backend>> = vec![Box::new(MockBackend), Box::new(MockBackend)];
    assert_eq!(backends.len(), 2);
    for b in &backends {
        assert_eq!(b.identity().id, "mock");
    }
}

#[tokio::test]
async fn t007_backend_trait_arc_run() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let (tx, _rx) = mpsc::channel(64);
    let receipt = backend
        .run(Uuid::new_v4(), wo("arc run"), tx)
        .await
        .unwrap();
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn t008_backend_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<MockBackend>();
}

#[test]
fn t009_backend_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<MockBackend>();
}

#[test]
fn t010_backend_send_sync_together() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

#[tokio::test]
async fn t011_backend_trait_run_preserves_run_id() {
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend.run(run_id, wo("id check"), tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn t012_backend_trait_run_preserves_work_order_id() {
    let work_order = wo("wo-id check");
    let wo_id = work_order.id;
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[test]
fn t013_backend_trait_identity_returns_backend_identity_struct() {
    let id: BackendIdentity = MockBackend.identity();
    assert!(!id.id.is_empty());
}

#[test]
fn t014_backend_trait_capabilities_returns_btreemap() {
    let caps: CapabilityManifest = MockBackend.capabilities();
    assert!(caps.len() >= 1);
}

#[test]
fn t015_backend_trait_multiple_identity_calls_consistent() {
    let id1 = MockBackend.identity();
    let id2 = MockBackend.identity();
    assert_eq!(id1.id, id2.id);
    assert_eq!(id1.backend_version, id2.backend_version);
    assert_eq!(id1.adapter_version, id2.adapter_version);
}

// ===========================================================================
// Section 2 – MockBackend configuration and behavior (t016–t030)
// ===========================================================================

#[test]
fn t016_mock_backend_is_unit_struct() {
    let _b = MockBackend;
}

#[test]
fn t017_mock_backend_is_clone() {
    let a = MockBackend;
    let _b = a.clone();
}

#[test]
fn t018_mock_backend_is_debug() {
    let dbg = format!("{:?}", MockBackend);
    assert!(dbg.contains("MockBackend"));
}

#[test]
fn t019_mock_backend_identity_id() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn t020_mock_backend_identity_backend_version() {
    assert_eq!(
        MockBackend.identity().backend_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t021_mock_backend_identity_adapter_version() {
    assert_eq!(
        MockBackend.identity().adapter_version.as_deref(),
        Some("0.1")
    );
}

#[test]
fn t022_mock_backend_six_capabilities() {
    assert_eq!(MockBackend.capabilities().len(), 6);
}

#[test]
fn t023_mock_backend_streaming_native() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t024_mock_backend_tool_read_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t025_mock_backend_tool_write_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t026_mock_backend_tool_edit_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t027_mock_backend_tool_bash_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t028_mock_backend_structured_output_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t029_mock_backend_capabilities_deterministic() {
    let c1 = MockBackend.capabilities();
    let c2 = MockBackend.capabilities();
    assert_eq!(c1.len(), c2.len());
    for key in c1.keys() {
        assert!(c2.contains_key(key));
    }
}

#[test]
fn t030_mock_backend_clone_same_identity() {
    let a = MockBackend;
    let b = a.clone();
    assert_eq!(a.identity().id, b.identity().id);
}

// ===========================================================================
// Section 3 – Backend event streaming (t031–t045)
// ===========================================================================

#[tokio::test]
async fn t031_mock_emits_exactly_four_events() {
    let (_, events) = run_collect("four events").await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn t032_first_event_is_run_started() {
    let (_, events) = run_collect("started").await;
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn t033_last_event_is_run_completed() {
    let (_, events) = run_collect("completed").await;
    assert!(matches!(
        events[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t034_middle_events_are_assistant_messages() {
    let (_, events) = run_collect("msgs").await;
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn t035_run_started_contains_task() {
    let (_, events) = run_collect("my special task").await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("my special task"));
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn t036_event_timestamps_are_non_null() {
    let (_, events) = run_collect("ts check").await;
    for ev in &events {
        assert!(ev.ts.timestamp() > 0);
    }
}

#[tokio::test]
async fn t037_event_timestamps_monotonic() {
    let (_, events) = run_collect("mono ts").await;
    for i in 1..events.len() {
        assert!(events[i].ts >= events[i - 1].ts);
    }
}

#[tokio::test]
async fn t038_event_ext_is_none() {
    let (_, events) = run_collect("ext check").await;
    for ev in &events {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn t039_trace_matches_streamed_events() {
    let (receipt, events) = run_collect("trace match").await;
    assert_eq!(receipt.trace.len(), events.len());
    for (r, e) in receipt.trace.iter().zip(events.iter()) {
        assert_eq!(r.ts, e.ts);
    }
}

#[tokio::test]
async fn t040_events_received_via_channel_receiver() {
    let (tx, mut rx) = mpsc::channel(64);
    MockBackend
        .run(Uuid::new_v4(), wo("chan"), tx)
        .await
        .unwrap();
    let mut count = 0;
    while let Ok(_ev) = rx.try_recv() {
        count += 1;
    }
    assert_eq!(count, 4);
}

#[tokio::test]
async fn t041_small_channel_still_works() {
    let (tx, mut rx) = mpsc::channel(4);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("tiny chan"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
    let mut count = 0;
    while let Ok(_) = rx.try_recv() {
        count += 1;
    }
    assert!(count > 0);
}

#[tokio::test]
async fn t042_dropped_receiver_does_not_panic() {
    let (tx, rx) = mpsc::channel(64);
    drop(rx);
    let receipt = MockBackend
        .run(Uuid::new_v4(), wo("dropped"), tx)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t043_event_ordering_run_started_first_completed_last() {
    let (_, events) = run_collect("order").await;
    assert!(matches!(
        events.first().unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t044_assistant_message_content_first() {
    let (_, events) = run_collect("content").await;
    if let AgentEventKind::AssistantMessage { text } = &events[1].kind {
        assert!(text.contains("mock backend"));
    } else {
        panic!("expected AssistantMessage");
    }
}

#[tokio::test]
async fn t045_assistant_message_content_second() {
    let (_, events) = run_collect("sidecar hint").await;
    if let AgentEventKind::AssistantMessage { text } = &events[2].kind {
        assert!(text.contains("sidecar"));
    } else {
        panic!("expected AssistantMessage");
    }
}

// ===========================================================================
// Section 4 – Backend receipt generation (t046–t060)
// ===========================================================================

#[tokio::test]
async fn t046_receipt_outcome_complete() {
    let r = run_receipt("receipt check").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t047_receipt_has_sha256_hash() {
    let r = run_receipt("hash").await;
    assert!(r.receipt_sha256.is_some());
}

#[tokio::test]
async fn t048_receipt_hash_is_64_hex_chars() {
    let r = run_receipt("hex").await;
    let hash = r.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn t049_receipt_contract_version() {
    let r = run_receipt("contract").await;
    assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn t050_receipt_backend_id_matches() {
    let r = run_receipt("backend id").await;
    assert_eq!(r.backend.id, "mock");
}

#[tokio::test]
async fn t051_receipt_backend_version_matches() {
    let r = run_receipt("ver").await;
    assert_eq!(r.backend.backend_version.as_deref(), Some("0.1"));
}

#[tokio::test]
async fn t052_receipt_mode_default_mapped() {
    let r = run_receipt("mode").await;
    assert_eq!(r.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t053_receipt_usage_raw_note_mock() {
    let r = run_receipt("usage raw").await;
    assert_eq!(r.usage_raw["note"], "mock");
}

#[tokio::test]
async fn t054_receipt_usage_input_tokens_zero() {
    let r = run_receipt("tokens").await;
    assert_eq!(r.usage.input_tokens, Some(0));
}

#[tokio::test]
async fn t055_receipt_usage_output_tokens_zero() {
    let r = run_receipt("tokens out").await;
    assert_eq!(r.usage.output_tokens, Some(0));
}

#[tokio::test]
async fn t056_receipt_usage_cost_zero() {
    let r = run_receipt("cost").await;
    assert_eq!(r.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn t057_receipt_artifacts_empty() {
    let r = run_receipt("artifacts").await;
    assert!(r.artifacts.is_empty());
}

#[tokio::test]
async fn t058_receipt_verification_harness_ok() {
    let r = run_receipt("harness").await;
    assert!(r.verification.harness_ok);
}

#[tokio::test]
async fn t059_receipt_verification_git_diff_none() {
    let r = run_receipt("diff").await;
    assert!(r.verification.git_diff.is_none());
}

#[tokio::test]
async fn t060_receipt_started_before_finished() {
    let r = run_receipt("timing").await;
    assert!(r.meta.started_at <= r.meta.finished_at);
}

// ===========================================================================
// Section 5 – Backend selection and registry (t061–t075)
// ===========================================================================

#[test]
fn t061_selector_first_match_selects_first() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("alpha", vec![Capability::Streaming], 1));
    sel.add_candidate(mock_candidate("beta", vec![Capability::Streaming], 2));
    let c = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(c.name, "alpha");
}

#[test]
fn t062_selector_best_fit_picks_most_caps() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(mock_candidate(
        "b",
        vec![Capability::Streaming, Capability::ToolRead],
        2,
    ));
    let c = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(c.name, "b");
}

#[test]
fn t063_selector_priority_picks_lowest_value() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(mock_candidate("hi", vec![Capability::Streaming], 100));
    sel.add_candidate(mock_candidate("lo", vec![Capability::Streaming], 1));
    let c = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(c.name, "lo");
}

#[test]
fn t064_selector_round_robin_rotates() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(mock_candidate("b", vec![Capability::Streaming], 2));
    let first = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let second = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    assert_ne!(first, second);
}

#[test]
fn t065_selector_none_when_no_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::ToolRead], 1));
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

#[test]
fn t066_selector_empty_requirements_selects_first() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![], 1));
    let c = sel.select(&[]).unwrap();
    assert_eq!(c.name, "a");
}

#[test]
fn t067_selector_disabled_candidate_skipped() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut c = mock_candidate("disabled", vec![Capability::Streaming], 1);
    c.enabled = false;
    sel.add_candidate(c);
    sel.add_candidate(mock_candidate("enabled", vec![Capability::Streaming], 2));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "enabled");
}

#[test]
fn t068_selector_candidate_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![], 1));
    sel.add_candidate(mock_candidate("b", vec![], 2));
    assert_eq!(sel.candidate_count(), 2);
}

#[test]
fn t069_selector_enabled_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![], 1));
    let mut dis = mock_candidate("b", vec![], 2);
    dis.enabled = false;
    sel.add_candidate(dis);
    assert_eq!(sel.enabled_count(), 1);
}

#[test]
fn t070_selector_select_all_returns_multiple() {
    let sel = {
        let mut s = BackendSelector::new(SelectionStrategy::FirstMatch);
        s.add_candidate(mock_candidate("a", vec![Capability::Streaming], 1));
        s.add_candidate(mock_candidate("b", vec![Capability::Streaming], 2));
        s
    };
    let all = sel.select_all(&[Capability::Streaming]);
    assert_eq!(all.len(), 2);
}

#[test]
fn t071_selector_with_result_provides_reason() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert!(!result.reason.is_empty());
    assert_eq!(result.selected, "a");
}

#[test]
fn t072_selector_with_result_alternatives() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 1));
    sel.add_candidate(mock_candidate("b", vec![Capability::Streaming], 2));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert!(result.alternatives.contains(&"b".to_string()));
}

#[test]
fn t073_selector_with_result_unmet_capabilities() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::ToolRead], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert!(!result.unmet_capabilities.is_empty());
}

#[test]
fn t074_selector_least_loaded_strategy() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLoaded);
    sel.add_candidate(mock_candidate("heavy", vec![Capability::Streaming], 100));
    sel.add_candidate(mock_candidate("light", vec![Capability::Streaming], 1));
    let c = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(c.name, "light");
}

#[test]
fn t075_selector_empty_returns_none() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert!(sel.select(&[]).is_none());
}

// ===========================================================================
// Section 6 – Backend capability declaration (t076–t090)
// ===========================================================================

#[test]
fn t076_capability_matrix_new_empty() {
    let m = CapabilityMatrix::new();
    assert!(m.is_empty());
}

#[test]
fn t077_capability_matrix_register_and_supports() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    assert!(m.supports("mock", &Capability::Streaming));
}

#[test]
fn t078_capability_matrix_does_not_support_unregistered() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    assert!(!m.supports("mock", &Capability::ToolBash));
}

#[test]
fn t079_capability_matrix_backends_for() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register("b", vec![Capability::ToolRead]);
    let bs = m.backends_for(&Capability::Streaming);
    assert_eq!(bs, vec!["a"]);
}

#[test]
fn t080_capability_matrix_all_capabilities() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    let all = m.all_capabilities("mock").unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn t081_capability_matrix_common_capabilities_intersection() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    m.register("b", vec![Capability::Streaming, Capability::ToolWrite]);
    let common = m.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(!common.contains(&Capability::ToolRead));
}

#[test]
fn t082_capability_matrix_empty_common_when_empty() {
    let m = CapabilityMatrix::new();
    assert!(m.common_capabilities().is_empty());
}

#[test]
fn t083_capability_matrix_backend_count() {
    let mut m = CapabilityMatrix::new();
    m.register("a", vec![Capability::Streaming]);
    m.register("b", vec![Capability::ToolRead]);
    assert_eq!(m.backend_count(), 2);
}

#[test]
fn t084_capability_matrix_evaluate_full_score() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    let report = m.evaluate("mock", &[Capability::Streaming, Capability::ToolRead]);
    assert!((report.score - 1.0).abs() < f64::EPSILON);
}

#[test]
fn t085_capability_matrix_evaluate_partial_score() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    let report = m.evaluate("mock", &[Capability::Streaming, Capability::ToolRead]);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
}

#[test]
fn t086_capability_matrix_evaluate_zero_score() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::ToolRead]);
    let report = m.evaluate("mock", &[Capability::Streaming]);
    assert!((report.score - 0.0).abs() < f64::EPSILON);
}

#[test]
fn t087_capability_matrix_best_backend() {
    let mut m = CapabilityMatrix::new();
    m.register("weak", vec![Capability::Streaming]);
    m.register("strong", vec![Capability::Streaming, Capability::ToolRead]);
    let best = m
        .best_backend(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(best, "strong");
}

#[test]
fn t088_capability_matrix_evaluate_empty_requirements() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    let report = m.evaluate("mock", &[]);
    assert!((report.score - 1.0).abs() < f64::EPSILON);
}

#[test]
fn t089_capability_matrix_register_merges() {
    let mut m = CapabilityMatrix::new();
    m.register("mock", vec![Capability::Streaming]);
    m.register("mock", vec![Capability::ToolRead]);
    let all = m.all_capabilities("mock").unwrap();
    assert!(all.contains(&Capability::Streaming));
    assert!(all.contains(&Capability::ToolRead));
}

#[test]
fn t090_capability_matrix_unknown_backend_evaluate() {
    let m = CapabilityMatrix::new();
    let report = m.evaluate("nonexistent", &[Capability::Streaming]);
    assert!(report.missing.contains(&Capability::Streaming));
    assert!((report.score - 0.0).abs() < f64::EPSILON);
}

// ===========================================================================
// Section 7 – Backend error handling (t091–t105)
// ===========================================================================

#[test]
fn t091_ensure_requirements_empty_passes() {
    let reqs = CapabilityRequirements::default();
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t092_ensure_requirements_native_streaming_passes() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t093_ensure_requirements_emulated_tool_read_passes() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t094_ensure_requirements_native_tool_read_fails() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn t095_ensure_requirements_missing_capability_fails() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn t096_ensure_requirements_error_message_contains_capability() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
    assert!(err.to_string().contains("McpClient"));
}

#[test]
fn t097_ensure_requirements_multiple_unsatisfied() {
    let reqs = CapabilityRequirements {
        required: vec![
            CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Emulated,
            },
            CapabilityRequirement {
                capability: Capability::McpServer,
                min_support: MinSupport::Emulated,
            },
        ],
    };
    let caps = MockBackend.capabilities();
    let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("McpClient"));
    assert!(msg.contains("McpServer"));
}

#[tokio::test]
async fn t098_mock_run_with_unsatisfied_requirements_fails() {
    let mut work_order = wo("unsatisfied");
    work_order.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(64);
    let result = MockBackend.run(Uuid::new_v4(), work_order, tx).await;
    assert!(result.is_err());
}

#[test]
fn t099_ensure_requirements_all_mock_caps_emulated_pass() {
    let caps = MockBackend.capabilities();
    for (cap, _) in &caps {
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: cap.clone(),
                min_support: MinSupport::Emulated,
            }],
        };
        assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
    }
}

#[test]
fn t100_validate_passthrough_compatibility_ok() {
    let work_order = wo("passthrough check");
    assert!(validate_passthrough_compatibility(&work_order).is_ok());
}

#[test]
fn t101_ensure_requirements_empty_manifest_all_fail() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let empty: CapabilityManifest = BTreeMap::new();
    assert!(ensure_capability_requirements(&reqs, &empty).is_err());
}

#[test]
fn t102_ensure_requirements_empty_reqs_with_empty_manifest_passes() {
    let reqs = CapabilityRequirements::default();
    let empty: CapabilityManifest = BTreeMap::new();
    assert!(ensure_capability_requirements(&reqs, &empty).is_ok());
}

#[tokio::test]
async fn t103_mock_run_error_contains_context() {
    let mut work_order = wo("ctx err");
    work_order.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::SessionResume,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(64);
    let err = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("capability") || msg.contains("requirement"));
}

#[test]
fn t104_ensure_requirements_native_satisfies_emulated_request() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t105_ensure_requirements_error_reports_actual_level() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    let err = ensure_capability_requirements(&reqs, &caps).unwrap_err();
    assert!(err.to_string().contains("Emulated"));
}

// ===========================================================================
// Section 8 – Backend identity and metadata (t106–t115)
// ===========================================================================

#[test]
fn t106_identity_serializes_to_json() {
    let id = MockBackend.identity();
    let json = serde_json::to_value(&id).unwrap();
    assert_eq!(json["id"], "mock");
}

#[test]
fn t107_identity_deserializes_from_json() {
    let json = json!({"id": "mock", "backend_version": "0.1", "adapter_version": "0.1"});
    let id: BackendIdentity = serde_json::from_value(json).unwrap();
    assert_eq!(id.id, "mock");
}

#[test]
fn t108_identity_roundtrip_json() {
    let id = MockBackend.identity();
    let json = serde_json::to_string(&id).unwrap();
    let id2: BackendIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(id.id, id2.id);
    assert_eq!(id.backend_version, id2.backend_version);
}

#[test]
fn t109_capabilities_are_btreemap_ordered() {
    let caps = MockBackend.capabilities();
    let keys: Vec<_> = caps.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[tokio::test]
async fn t110_receipt_backend_matches_identity() {
    let r = run_receipt("identity receipt").await;
    let id = MockBackend.identity();
    assert_eq!(r.backend.id, id.id);
    assert_eq!(r.backend.backend_version, id.backend_version);
    assert_eq!(r.backend.adapter_version, id.adapter_version);
}

#[tokio::test]
async fn t111_receipt_capabilities_match_declared() {
    let r = run_receipt("cap receipt").await;
    let caps = MockBackend.capabilities();
    assert_eq!(r.capabilities.len(), caps.len());
    for key in caps.keys() {
        assert!(r.capabilities.contains_key(key));
    }
}

#[test]
fn t112_support_level_native_satisfies_native() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Native));
}

#[test]
fn t113_support_level_native_satisfies_emulated() {
    assert!(SupportLevel::Native.satisfies(&MinSupport::Emulated));
}

#[test]
fn t114_support_level_emulated_satisfies_emulated() {
    assert!(SupportLevel::Emulated.satisfies(&MinSupport::Emulated));
}

#[test]
fn t115_support_level_emulated_does_not_satisfy_native() {
    assert!(!SupportLevel::Emulated.satisfies(&MinSupport::Native));
}

// ===========================================================================
// Section 9 – SidecarBackend basics without real process (t116–t122)
// ===========================================================================

#[test]
fn t116_sidecar_spec_new() {
    let spec = SidecarSpec::new("echo");
    assert_eq!(spec.command, "echo");
}

#[test]
fn t117_sidecar_spec_default_args_empty() {
    let spec = SidecarSpec::new("cmd");
    assert!(spec.args.is_empty());
}

#[test]
fn t118_sidecar_spec_default_env_empty() {
    let spec = SidecarSpec::new("cmd");
    assert!(spec.env.is_empty());
}

#[test]
fn t119_sidecar_spec_default_cwd_none() {
    let spec = SidecarSpec::new("cmd");
    assert!(spec.cwd.is_none());
}

#[test]
fn t120_sidecar_spec_is_debug() {
    let spec = SidecarSpec::new("test");
    let dbg = format!("{:?}", spec);
    assert!(dbg.contains("test"));
}

#[test]
fn t121_sidecar_spec_is_clone() {
    let spec = SidecarSpec::new("cmd");
    let copy = spec.clone();
    assert_eq!(copy.command, spec.command);
}

#[test]
fn t122_sidecar_spec_serializes_json() {
    let spec = SidecarSpec::new("node");
    let json = serde_json::to_value(&spec).unwrap();
    assert_eq!(json["command"], "node");
}

// ===========================================================================
// Section 10 – Backend lifecycle: init, run, shutdown (t123–t130)
// ===========================================================================

#[tokio::test]
async fn t123_sequential_runs_all_complete() {
    for i in 0..5 {
        let r = run_receipt(&format!("seq-{i}")).await;
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn t124_sequential_runs_unique_hashes() {
    let mut hashes = HashSet::new();
    for i in 0..5 {
        let r = run_receipt(&format!("hash-{i}")).await;
        hashes.insert(r.receipt_sha256.unwrap());
    }
    assert_eq!(hashes.len(), 5);
}

#[tokio::test]
async fn t125_mock_backend_stateless_between_runs() {
    let (r1, _) = run_collect("run-a").await;
    let (r2, _) = run_collect("run-b").await;
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn t126_mock_backend_works_after_error_run() {
    let mut bad = wo("bad");
    bad.requirements = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::McpClient,
            min_support: MinSupport::Native,
        }],
    };
    let (tx, _rx) = mpsc::channel(64);
    let _ = MockBackend.run(Uuid::new_v4(), bad, tx).await;

    let r = run_receipt("good after bad").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t127_backend_clone_independent_runs() {
    let a = MockBackend;
    let b = a.clone();
    let r1 = {
        let (tx, _rx) = mpsc::channel(64);
        a.run(Uuid::new_v4(), wo("clone a"), tx).await.unwrap()
    };
    let r2 = {
        let (tx, _rx) = mpsc::channel(64);
        b.run(Uuid::new_v4(), wo("clone b"), tx).await.unwrap()
    };
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn t128_ten_sequential_runs() {
    for i in 0..10 {
        let r = run_receipt(&format!("ten-{i}")).await;
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn t129_run_with_nil_uuid() {
    let (receipt, events) = run_with_id(Uuid::nil(), wo("nil id")).await;
    assert_eq!(receipt.meta.run_id, Uuid::nil());
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn t130_run_with_max_uuid() {
    let (receipt, _) = run_with_id(Uuid::max(), wo("max id")).await;
    assert_eq!(receipt.meta.run_id, Uuid::max());
}

// ===========================================================================
// Section 11 – Backend configuration validation (t131–t138)
// ===========================================================================

#[test]
fn t131_extract_execution_mode_default_mapped() {
    let work_order = wo("mode test");
    assert_eq!(extract_execution_mode(&work_order), ExecutionMode::Mapped);
}

#[test]
fn t132_extract_execution_mode_passthrough_nested() {
    let mut work_order = wo("passthrough");
    work_order
        .config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    assert_eq!(
        extract_execution_mode(&work_order),
        ExecutionMode::Passthrough
    );
}

#[test]
fn t133_extract_execution_mode_mapped_nested() {
    let mut work_order = wo("mapped");
    work_order
        .config
        .vendor
        .insert("abp".into(), json!({"mode": "mapped"}));
    assert_eq!(extract_execution_mode(&work_order), ExecutionMode::Mapped);
}

#[test]
fn t134_extract_execution_mode_flat_key() {
    let mut work_order = wo("flat");
    work_order
        .config
        .vendor
        .insert("abp.mode".into(), json!("passthrough"));
    assert_eq!(
        extract_execution_mode(&work_order),
        ExecutionMode::Passthrough
    );
}

#[test]
fn t135_extract_execution_mode_invalid_defaults_mapped() {
    let mut work_order = wo("invalid");
    work_order
        .config
        .vendor
        .insert("abp".into(), json!({"mode": "invalid_mode"}));
    assert_eq!(extract_execution_mode(&work_order), ExecutionMode::Mapped);
}

#[tokio::test]
async fn t136_receipt_mode_passthrough_when_configured() {
    let mut work_order = wo("passthrough receipt");
    work_order
        .config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    let (tx, _rx) = mpsc::channel(64);
    let receipt = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(receipt.mode, ExecutionMode::Passthrough);
}

#[test]
fn t137_validate_passthrough_with_policy() {
    let mut work_order = wo("policy passthrough");
    work_order.policy.allowed_tools = vec!["read".into()];
    assert!(validate_passthrough_compatibility(&work_order).is_ok());
}

#[test]
fn t138_extract_execution_mode_no_vendor_key() {
    let work_order = wo("no vendor");
    assert_eq!(extract_execution_mode(&work_order), ExecutionMode::Mapped);
}

// ===========================================================================
// Section 12 – Backend timeout handling (t139–t143)
// ===========================================================================

#[tokio::test]
async fn t139_mock_run_completes_within_timeout() {
    let result = tokio::time::timeout(Duration::from_secs(5), run_receipt("timeout")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t140_timeout_wrapper_short_succeeds() {
    let result =
        tokio::time::timeout(Duration::from_millis(500), run_receipt("fast timeout")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t141_multiple_runs_within_timeout() {
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        for i in 0..10 {
            run_receipt(&format!("timed-{i}")).await;
        }
    })
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn t142_abort_handle_drops_gracefully() {
    let handle = tokio::spawn(async {
        run_receipt("abort test").await;
    });
    let _ = handle.await;
}

#[tokio::test]
async fn t143_select_with_timeout_produces_receipt() {
    let receipt = tokio::time::timeout(Duration::from_secs(2), run_receipt("select-timeout"))
        .await
        .unwrap();
    assert_eq!(receipt.outcome, Outcome::Complete);
}

// ===========================================================================
// Section 13 – Backend concurrent execution (t144–t155)
// ===========================================================================

#[tokio::test]
async fn t144_two_concurrent_runs() {
    let (r1, r2) = tokio::join!(run_receipt("conc-1"), run_receipt("conc-2"));
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
}

#[tokio::test]
async fn t145_ten_concurrent_runs() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..10 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo(&format!("par-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    let mut ids = HashSet::new();
    for h in handles {
        let r = h.await.unwrap();
        ids.insert(r.meta.run_id);
    }
    assert_eq!(ids.len(), 10);
}

#[tokio::test]
async fn t146_concurrent_runs_all_complete() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo(&format!("complete-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    for h in handles {
        let r = h.await.unwrap();
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn t147_concurrent_runs_unique_hashes() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo(&format!("uhash-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    let mut hashes = HashSet::new();
    for h in handles {
        let r = h.await.unwrap();
        hashes.insert(r.receipt_sha256.unwrap());
    }
    assert_eq!(hashes.len(), 5);
}

#[tokio::test]
async fn t148_concurrent_event_counts() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo(&format!("ev-{i}")), tx)
                .await
                .unwrap();
            let mut count = 0;
            while let Ok(_) = rx.try_recv() {
                count += 1;
            }
            count
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), 4);
    }
}

#[tokio::test]
async fn t149_arc_backend_concurrent_identity() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for _ in 0..10 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move { b.identity().id }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), "mock");
    }
}

#[tokio::test]
async fn t150_concurrent_capabilities_consistent() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for _ in 0..10 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move { b.capabilities().len() }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap(), 6);
    }
}

#[tokio::test]
async fn t151_concurrent_with_different_tasks() {
    let tasks = vec!["alpha", "beta", "gamma", "delta", "epsilon"];
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for task in &tasks {
        let b = Arc::clone(&backend);
        let t = task.to_string();
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo(&t), tx).await.unwrap()
        }));
    }
    for h in handles {
        let r = h.await.unwrap();
        assert_eq!(r.outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn t152_concurrent_with_shared_run_id_distinct_receipts() {
    let run_id = Uuid::new_v4();
    let (r1, r2) = tokio::join!(
        run_with_id(run_id, wo("shared-a")),
        run_with_id(run_id, wo("shared-b"))
    );
    assert_eq!(r1.0.meta.run_id, r2.0.meta.run_id);
}

#[tokio::test]
async fn t153_twenty_concurrent_runs() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..20 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo(&format!("twenty-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    let mut count = 0;
    for h in handles {
        let r = h.await.unwrap();
        assert_eq!(r.outcome, Outcome::Complete);
        count += 1;
    }
    assert_eq!(count, 20);
}

#[tokio::test]
async fn t154_concurrent_cloned_backends() {
    let mut handles = Vec::new();
    for i in 0..5 {
        let b = MockBackend;
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), wo(&format!("cloned-{i}")), tx)
                .await
                .unwrap()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap().outcome, Outcome::Complete);
    }
}

#[tokio::test]
async fn t155_concurrent_different_work_order_configs() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for i in 0..3 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let work_order = WorkOrderBuilder::new(format!("config-{i}"))
                .model(format!("model-{i}"))
                .build();
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), work_order, tx).await.unwrap()
        }));
    }
    for h in handles {
        assert_eq!(h.await.unwrap().outcome, Outcome::Complete);
    }
}

// ===========================================================================
// Section 14 – Backend event ordering guarantees (t156–t165)
// ===========================================================================

#[tokio::test]
async fn t156_events_always_start_with_run_started() {
    for i in 0..5 {
        let (_, events) = run_collect(&format!("order-{i}")).await;
        assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    }
}

#[tokio::test]
async fn t157_events_always_end_with_run_completed() {
    for i in 0..5 {
        let (_, events) = run_collect(&format!("end-{i}")).await;
        assert!(matches!(
            events.last().unwrap().kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }
}

#[tokio::test]
async fn t158_trace_event_count_matches_channel_count() {
    let (receipt, events) = run_collect("count match").await;
    assert_eq!(receipt.trace.len(), events.len());
}

#[tokio::test]
async fn t159_trace_event_kinds_match_channel_events() {
    let (receipt, events) = run_collect("kind match").await;
    for (t, e) in receipt.trace.iter().zip(events.iter()) {
        assert_eq!(
            std::mem::discriminant(&t.kind),
            std::mem::discriminant(&e.kind)
        );
    }
}

#[tokio::test]
async fn t160_timestamps_across_multiple_runs_increase() {
    let (r1, _) = run_collect("first-run").await;
    let (r2, _) = run_collect("second-run").await;
    assert!(r2.meta.started_at >= r1.meta.started_at);
}

#[tokio::test]
async fn t161_run_completed_message_deterministic() {
    let (_, events) = run_collect("det msg").await;
    if let AgentEventKind::RunCompleted { message } = &events[3].kind {
        assert_eq!(message, "mock run complete");
    } else {
        panic!("expected RunCompleted");
    }
}

#[tokio::test]
async fn t162_event_kinds_sequence_is_deterministic() {
    for _ in 0..3 {
        let (_, events) = run_collect("det sequence").await;
        assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
        assert!(matches!(
            events[1].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(matches!(
            events[2].kind,
            AgentEventKind::AssistantMessage { .. }
        ));
        assert!(matches!(
            events[3].kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }
}

#[tokio::test]
async fn t163_trace_timestamps_all_between_started_and_finished() {
    let r = run_receipt("bounds check").await;
    for ev in &r.trace {
        assert!(ev.ts >= r.meta.started_at);
        assert!(ev.ts <= r.meta.finished_at);
    }
}

#[tokio::test]
async fn t164_trace_and_events_have_same_length() {
    let (receipt, events) = run_collect("len check").await;
    assert_eq!(receipt.trace.len(), events.len());
    assert_eq!(receipt.trace.len(), 4);
}

#[tokio::test]
async fn t165_trace_ext_fields_all_none() {
    let r = run_receipt("ext none").await;
    for ev in &r.trace {
        assert!(ev.ext.is_none());
    }
}

// ===========================================================================
// Section 15 – Backend usage reporting (t166–t180)
// ===========================================================================

#[test]
fn t166_metrics_new_zero() {
    let m = BackendMetrics::new();
    assert_eq!(m.total_runs(), 0);
}

#[test]
fn t167_metrics_record_run_increments() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    assert_eq!(m.total_runs(), 1);
}

#[test]
fn t168_metrics_success_rate_all_success() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    m.record_run(true, 4, 200);
    assert!((m.success_rate() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn t169_metrics_success_rate_half() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    m.record_run(false, 0, 50);
    assert!((m.success_rate() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn t170_metrics_average_duration() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    m.record_run(true, 4, 300);
    assert!((m.average_duration_ms() - 200.0).abs() < f64::EPSILON);
}

#[test]
fn t171_metrics_average_events_per_run() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    m.record_run(true, 6, 200);
    assert!((m.average_events_per_run() - 5.0).abs() < f64::EPSILON);
}

#[test]
fn t172_metrics_zero_runs_rates() {
    let m = BackendMetrics::new();
    assert!((m.success_rate() - 0.0).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 0.0).abs() < f64::EPSILON);
    assert!((m.average_events_per_run() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn t173_metrics_reset() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    m.reset();
    assert_eq!(m.total_runs(), 0);
}

#[test]
fn t174_metrics_snapshot() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 4);
    assert_eq!(snap.total_duration_ms, 100);
}

#[test]
fn t175_metrics_snapshot_serializes() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    let snap = m.snapshot();
    let json = serde_json::to_value(&snap).unwrap();
    assert_eq!(json["total_runs"], 1);
}

#[test]
fn t176_registry_get_or_create() {
    let reg = MetricsRegistry::new();
    let m1 = reg.get_or_create("mock");
    m1.record_run(true, 4, 100);
    let m2 = reg.get_or_create("mock");
    assert_eq!(m2.total_runs(), 1);
}

#[test]
fn t177_registry_separate_backends() {
    let reg = MetricsRegistry::new();
    reg.get_or_create("a").record_run(true, 1, 10);
    reg.get_or_create("b").record_run(false, 0, 5);
    let snaps = reg.snapshot_all();
    assert_eq!(snaps["a"].successful_runs, 1);
    assert_eq!(snaps["b"].failed_runs, 1);
}

#[test]
fn t178_registry_snapshot_all_empty() {
    let reg = MetricsRegistry::new();
    assert!(reg.snapshot_all().is_empty());
}

#[test]
fn t179_health_checker_empty_is_healthy() {
    let hc = HealthChecker::new();
    assert!(hc.is_healthy());
}

#[test]
fn t180_health_checker_degraded_overall() {
    let mut hc = HealthChecker::new();
    hc.add_check("mock", HealthStatus::Healthy);
    hc.add_check(
        "slow",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
    );
    assert!(matches!(hc.overall_status(), HealthStatus::Degraded { .. }));
}

// ===========================================================================
// Section 16 – Additional deep coverage (t181–t190)
// ===========================================================================

#[tokio::test]
async fn t181_receipt_hash_deterministic_for_same_receipt() {
    let r = run_receipt("deterministic hash").await;
    let h1 = receipt_hash(&r).unwrap();
    let h2 = receipt_hash(&r).unwrap();
    assert_eq!(h1, h2);
}

#[tokio::test]
async fn t182_receipt_json_roundtrip() {
    let r = run_receipt("roundtrip").await;
    let json = serde_json::to_string(&r).unwrap();
    let r2: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(r.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn t183_work_order_builder_model() {
    let work_order = WorkOrderBuilder::new("model test").model("gpt-4").build();
    assert_eq!(work_order.config.model.as_deref(), Some("gpt-4"));
    let (tx, _rx) = mpsc::channel(64);
    let r = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t184_work_order_builder_max_budget() {
    let work_order = WorkOrderBuilder::new("budget test")
        .max_budget_usd(10.0)
        .build();
    assert_eq!(work_order.config.max_budget_usd, Some(10.0));
}

#[tokio::test]
async fn t185_work_order_builder_max_turns() {
    let work_order = WorkOrderBuilder::new("turns test").max_turns(5).build();
    assert_eq!(work_order.config.max_turns, Some(5));
}

#[tokio::test]
async fn t186_work_order_with_context_packet() {
    let ctx = ContextPacket {
        files: vec!["main.rs".into()],
        snippets: vec![ContextSnippet {
            name: "test".into(),
            content: "hello".into(),
        }],
    };
    let work_order = WorkOrderBuilder::new("ctx test").context(ctx).build();
    assert_eq!(work_order.context.files.len(), 1);
    let (tx, _rx) = mpsc::channel(64);
    let r = MockBackend
        .run(Uuid::new_v4(), work_order, tx)
        .await
        .unwrap();
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t187_work_order_with_policy() {
    let policy = PolicyProfile {
        allowed_tools: vec!["read".into(), "write".into()],
        disallowed_tools: vec!["bash".into()],
        ..Default::default()
    };
    let work_order = WorkOrderBuilder::new("policy test").policy(policy).build();
    assert_eq!(work_order.policy.allowed_tools.len(), 2);
}

#[tokio::test]
async fn t188_unicode_task_description() {
    let r = run_receipt("日本語テスト 🚀 émoji").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t189_empty_task_description() {
    let r = run_receipt("").await;
    assert_eq!(r.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t190_long_task_description() {
    let long = "x".repeat(10_000);
    let r = run_receipt(&long).await;
    assert_eq!(r.outcome, Outcome::Complete);
}

// ===========================================================================
// Section 17 – Health and metrics integration (t191–t200)
// ===========================================================================

#[test]
fn t191_health_checker_unhealthy_overrides() {
    let mut hc = HealthChecker::new();
    hc.add_check("a", HealthStatus::Healthy);
    hc.add_check(
        "b",
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
    );
    assert!(matches!(
        hc.overall_status(),
        HealthStatus::Unhealthy { .. }
    ));
}

#[test]
fn t192_health_checker_clear() {
    let mut hc = HealthChecker::new();
    hc.add_check("a", HealthStatus::Healthy);
    hc.clear();
    assert_eq!(hc.check_count(), 0);
}

#[test]
fn t193_health_checker_unhealthy_checks_list() {
    let mut hc = HealthChecker::new();
    hc.add_check("good", HealthStatus::Healthy);
    hc.add_check(
        "bad",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
    );
    let unhealthy = hc.unhealthy_checks();
    assert_eq!(unhealthy.len(), 1);
    assert_eq!(unhealthy[0].name, "bad");
}

#[test]
fn t194_health_checker_unknown_status() {
    let mut hc = HealthChecker::new();
    hc.add_check("mystery", HealthStatus::Unknown);
    assert!(matches!(hc.overall_status(), HealthStatus::Unknown));
}

#[test]
fn t195_metrics_debug_format() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    let dbg = format!("{:?}", m);
    assert!(dbg.contains("BackendMetrics"));
}

#[test]
fn t196_registry_debug_format() {
    let reg = MetricsRegistry::new();
    reg.get_or_create("mock");
    let dbg = format!("{:?}", reg);
    assert!(dbg.contains("MetricsRegistry"));
}

#[test]
fn t197_metrics_default_trait() {
    let m = BackendMetrics::default();
    assert_eq!(m.total_runs(), 0);
}

#[test]
fn t198_registry_default_trait() {
    let reg = MetricsRegistry::default();
    assert!(reg.snapshot_all().is_empty());
}

#[tokio::test]
async fn t199_receipt_duration_ms_non_negative() {
    let r = run_receipt("duration").await;
    assert!(r.meta.duration_ms < 10_000);
}

#[tokio::test]
async fn t200_receipt_trace_four_events() {
    let r = run_receipt("trace four").await;
    assert_eq!(r.trace.len(), 4);
}
