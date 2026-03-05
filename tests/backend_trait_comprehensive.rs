#![allow(clippy::all)]
#![allow(dead_code)]
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
//! Comprehensive tests for the Backend trait and backend implementations.
//!
//! Categories:
//! 1. Backend trait methods and signatures
//! 2. MockBackend construction and behavior
//! 3. MockBackend event streaming
//! 4. MockBackend receipt generation
//! 5. Backend registration (BackendSelector)
//! 6. Backend selection by name
//! 7. Backend capabilities declaration
//! 8. Edge cases: unknown backends, concurrent streams

use std::collections::BTreeMap;
use std::sync::Arc;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, MinSupport, Outcome,
    SupportLevel, WorkOrder, WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_integrations::capability::CapabilityMatrix;
use abp_integrations::health::{HealthChecker, HealthStatus};
use abp_integrations::metrics::{BackendMetrics, MetricsRegistry};
use abp_integrations::selector::{BackendCandidate, BackendSelector, SelectionStrategy};
use abp_integrations::{
    ensure_capability_requirements, extract_execution_mode, validate_passthrough_compatibility,
    Backend, MockBackend,
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

// ===========================================================================
// Helpers
// ===========================================================================

fn base_work_order() -> WorkOrder {
    WorkOrderBuilder::new("test task").build()
}

fn work_order_with_task(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).build()
}

async fn run_mock(backend: &MockBackend, wo: WorkOrder) -> (abp_core::Receipt, Vec<AgentEvent>) {
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend.run(run_id, wo, tx).await.unwrap();
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
// Category 1: Backend trait methods and signatures (tests 1–15)
// ===========================================================================

#[test]
fn t001_backend_trait_is_object_safe() {
    // Verify we can create a trait object from MockBackend.
    let backend: Box<dyn Backend> = Box::new(MockBackend);
    let _identity = backend.identity();
}

#[test]
fn t002_backend_trait_identity_returns_backend_identity() {
    let backend = MockBackend;
    let id: BackendIdentity = backend.identity();
    assert!(!id.id.is_empty());
}

#[test]
fn t003_backend_trait_capabilities_returns_manifest() {
    let backend = MockBackend;
    let caps: CapabilityManifest = backend.capabilities();
    assert!(!caps.is_empty());
}

#[tokio::test]
async fn t004_backend_trait_run_returns_receipt() {
    let backend = MockBackend;
    let (receipt, _) = run_mock(&backend, base_work_order()).await;
    assert!(receipt.receipt_sha256.is_some());
}

#[test]
fn t005_backend_trait_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockBackend>();
}

#[test]
fn t006_backend_as_arc_dyn() {
    let backend: Arc<dyn Backend> = Arc::new(MockBackend);
    let id = backend.identity();
    assert_eq!(id.id, "mock");
}

#[tokio::test]
async fn t007_backend_run_with_specific_run_id() {
    let backend = MockBackend;
    let run_id = Uuid::new_v4();
    let (tx, _rx) = mpsc::channel(64);
    let receipt = backend.run(run_id, base_work_order(), tx).await.unwrap();
    assert_eq!(receipt.meta.run_id, run_id);
}

#[tokio::test]
async fn t008_backend_run_preserves_work_order_id() {
    let backend = MockBackend;
    let wo = base_work_order();
    let wo_id = wo.id;
    let (receipt, _) = run_mock(&backend, wo).await;
    assert_eq!(receipt.meta.work_order_id, wo_id);
}

#[test]
fn t009_backend_identity_fields_populated() {
    let id = MockBackend.identity();
    assert_eq!(id.id, "mock");
    assert!(id.backend_version.is_some());
    assert!(id.adapter_version.is_some());
}

#[test]
fn t010_backend_capabilities_contains_streaming() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::Streaming));
}

#[test]
fn t011_backend_dyn_dispatch_identity() {
    let b: &dyn Backend = &MockBackend;
    assert_eq!(b.identity().id, "mock");
}

#[test]
fn t012_backend_dyn_dispatch_capabilities() {
    let b: &dyn Backend = &MockBackend;
    assert!(!b.capabilities().is_empty());
}

#[tokio::test]
async fn t013_backend_dyn_dispatch_run() {
    let b: Box<dyn Backend> = Box::new(MockBackend);
    let (tx, _rx) = mpsc::channel(64);
    let result = b.run(Uuid::new_v4(), base_work_order(), tx).await;
    assert!(result.is_ok());
}

#[test]
fn t014_backend_identity_version_strings() {
    let id = MockBackend.identity();
    assert_eq!(id.backend_version.as_deref(), Some("0.1"));
    assert_eq!(id.adapter_version.as_deref(), Some("0.1"));
}

#[test]
fn t015_backend_capabilities_support_levels() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

// ===========================================================================
// Category 2: MockBackend construction and behavior (tests 16–30)
// ===========================================================================

#[test]
fn t016_mock_backend_is_unit_struct() {
    let _b = MockBackend;
}

#[test]
fn t017_mock_backend_clone() {
    let a = MockBackend;
    let b = a.clone();
    assert_eq!(a.identity().id, b.identity().id);
}

#[test]
fn t018_mock_backend_debug() {
    let b = MockBackend;
    let debug = format!("{b:?}");
    assert!(debug.contains("MockBackend"));
}

#[test]
fn t019_mock_backend_identity_is_mock() {
    assert_eq!(MockBackend.identity().id, "mock");
}

#[test]
fn t020_mock_backend_has_tool_write() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::ToolWrite));
}

#[test]
fn t021_mock_backend_has_tool_edit() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::ToolEdit));
}

#[test]
fn t022_mock_backend_has_tool_bash() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::ToolBash));
}

#[test]
fn t023_mock_backend_has_structured_output() {
    let caps = MockBackend.capabilities();
    assert!(caps.contains_key(&Capability::StructuredOutputJsonSchema));
}

#[test]
fn t024_mock_backend_streaming_is_native() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn t025_mock_backend_tool_read_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolRead),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t026_mock_backend_tool_write_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolWrite),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t027_mock_backend_tool_edit_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolEdit),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t028_mock_backend_tool_bash_is_emulated() {
    let caps = MockBackend.capabilities();
    assert!(matches!(
        caps.get(&Capability::ToolBash),
        Some(SupportLevel::Emulated)
    ));
}

#[test]
fn t029_mock_backend_capability_count() {
    let caps = MockBackend.capabilities();
    assert_eq!(caps.len(), 6);
}

#[test]
fn t030_mock_backend_no_unsupported_cap() {
    let caps = MockBackend.capabilities();
    assert!(!caps.contains_key(&Capability::McpClient));
}

// ===========================================================================
// Category 3: MockBackend event streaming (tests 31–50)
// ===========================================================================

#[tokio::test]
async fn t031_mock_streams_run_started() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. })));
}

#[tokio::test]
async fn t032_mock_streams_run_completed() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. })));
}

#[tokio::test]
async fn t033_mock_streams_assistant_messages() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    let msg_count = events
        .iter()
        .filter(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }))
        .count();
    assert!(msg_count >= 2);
}

#[tokio::test]
async fn t034_mock_event_count_is_four() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn t035_mock_first_event_is_run_started() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[tokio::test]
async fn t036_mock_last_event_is_run_completed() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(matches!(
        &events.last().unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t037_mock_run_started_message_contains_task() {
    let wo = work_order_with_task("my special task");
    let (_, events) = run_mock(&MockBackend, wo).await;
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains("my special task"));
    } else {
        panic!("expected RunStarted");
    }
}

#[tokio::test]
async fn t038_mock_events_have_timestamps() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    for ev in &events {
        // Timestamps should be non-zero
        assert!(ev.ts.timestamp() > 0);
    }
}

#[tokio::test]
async fn t039_mock_event_timestamps_non_decreasing() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    for w in events.windows(2) {
        assert!(w[1].ts >= w[0].ts);
    }
}

#[tokio::test]
async fn t040_mock_events_ext_is_none() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    for ev in &events {
        assert!(ev.ext.is_none());
    }
}

#[tokio::test]
async fn t041_mock_trace_matches_streamed_events() {
    let backend = MockBackend;
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(64);
    let receipt = backend.run(run_id, base_work_order(), tx).await.unwrap();
    let mut streamed = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        streamed.push(ev);
    }
    assert_eq!(receipt.trace.len(), streamed.len());
}

#[tokio::test]
async fn t042_mock_channel_capacity_1() {
    // Even with tiny channel, run should succeed (sends are awaited)
    let backend = MockBackend;
    let run_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(1);
    // Spawn receiver to drain
    let handle = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        events
    });
    let receipt = backend.run(run_id, base_work_order(), tx).await.unwrap();
    let events = handle.await.unwrap();
    assert_eq!(events.len(), 4);
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn t043_mock_assistant_message_content() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    let msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(msgs.iter().any(|m| m.contains("mock backend")));
}

#[tokio::test]
async fn t044_mock_run_completed_message() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    if let AgentEventKind::RunCompleted { message } = &events.last().unwrap().kind {
        assert!(message.contains("mock run complete"));
    } else {
        panic!("expected RunCompleted");
    }
}

#[tokio::test]
async fn t045_mock_events_order_structure() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    // RunStarted, AssistantMessage, AssistantMessage, RunCompleted
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        &events[1].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        &events[2].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(
        &events[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn t046_mock_second_assistant_msg_mentions_sidecar() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    if let AgentEventKind::AssistantMessage { text } = &events[2].kind {
        assert!(text.contains("sidecar"));
    } else {
        panic!("expected AssistantMessage");
    }
}

#[tokio::test]
async fn t047_mock_no_tool_call_events() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(!events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::ToolCall { .. })));
}

#[tokio::test]
async fn t048_mock_no_error_events() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(!events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Error { .. })));
}

#[tokio::test]
async fn t049_mock_no_warning_events() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(!events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::Warning { .. })));
}

#[tokio::test]
async fn t050_mock_no_file_changed_events() {
    let (_, events) = run_mock(&MockBackend, base_work_order()).await;
    assert!(!events
        .iter()
        .any(|e| matches!(&e.kind, AgentEventKind::FileChanged { .. })));
}

// ===========================================================================
// Category 4: MockBackend receipt generation (tests 51–70)
// ===========================================================================

#[tokio::test]
async fn t051_receipt_has_sha256() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.receipt_sha256.is_some());
}

#[tokio::test]
async fn t052_receipt_sha256_is_hex() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn t053_receipt_sha256_length() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    let hash = receipt.receipt_sha256.as_ref().unwrap();
    assert_eq!(hash.len(), 64); // SHA-256 = 64 hex chars
}

#[tokio::test]
async fn t054_receipt_contract_version() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[tokio::test]
async fn t055_receipt_backend_identity_matches() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.backend.id, "mock");
}

#[tokio::test]
async fn t056_receipt_outcome_complete() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t057_receipt_mode_default_mapped() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.mode, ExecutionMode::Mapped);
}

#[tokio::test]
async fn t058_receipt_duration_non_negative() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    // duration_ms is u64, so always >= 0
    let _ = receipt.meta.duration_ms;
}

#[tokio::test]
async fn t059_receipt_started_before_finished() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.meta.finished_at >= receipt.meta.started_at);
}

#[tokio::test]
async fn t060_receipt_usage_zero_tokens() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.usage.input_tokens, Some(0));
    assert_eq!(receipt.usage.output_tokens, Some(0));
}

#[tokio::test]
async fn t061_receipt_usage_zero_cost() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.usage.estimated_cost_usd, Some(0.0));
}

#[tokio::test]
async fn t062_receipt_trace_has_events() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.trace.len(), 4);
}

#[tokio::test]
async fn t063_receipt_artifacts_empty() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.artifacts.is_empty());
}

#[tokio::test]
async fn t064_receipt_verification_harness_ok() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.verification.harness_ok);
}

#[tokio::test]
async fn t065_receipt_verification_no_git_diff() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.verification.git_diff.is_none());
}

#[tokio::test]
async fn t066_receipt_verification_no_git_status() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert!(receipt.verification.git_status.is_none());
}

#[tokio::test]
async fn t067_receipt_capabilities_match_backend() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    // SupportLevel doesn't impl PartialEq, so compare via Debug
    assert_eq!(
        format!("{:?}", receipt.capabilities),
        format!("{:?}", MockBackend.capabilities())
    );
}

#[tokio::test]
async fn t068_receipt_usage_raw_is_mock_note() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.usage_raw, json!({"note": "mock"}));
}

#[tokio::test]
async fn t069_two_receipts_different_hashes() {
    // Different run_ids → different receipts → different hashes
    let (r1, _) = run_mock(&MockBackend, base_work_order()).await;
    let (r2, _) = run_mock(&MockBackend, base_work_order()).await;
    // run_id differs, so hashes should differ
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
    assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
}

#[tokio::test]
async fn t070_receipt_backend_version_in_receipt() {
    let (receipt, _) = run_mock(&MockBackend, base_work_order()).await;
    assert_eq!(receipt.backend.backend_version, Some("0.1".to_string()));
    assert_eq!(receipt.backend.adapter_version, Some("0.1".to_string()));
}

// ===========================================================================
// Category 5: Backend registration (BackendSelector) (tests 71–85)
// ===========================================================================

#[test]
fn t071_selector_new_first_match() {
    let sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert_eq!(sel.candidate_count(), 0);
}

#[test]
fn t072_selector_add_candidate() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("mock", vec![Capability::Streaming], 0));
    assert_eq!(sel.candidate_count(), 1);
}

#[test]
fn t073_selector_multiple_candidates() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate("b", vec![Capability::ToolRead], 1));
    assert_eq!(sel.candidate_count(), 2);
}

#[test]
fn t074_selector_enabled_count() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![], 0));
    let mut disabled = mock_candidate("b", vec![], 1);
    disabled.enabled = false;
    sel.add_candidate(disabled);
    assert_eq!(sel.enabled_count(), 1);
    assert_eq!(sel.candidate_count(), 2);
}

#[test]
fn t075_selector_select_all_empty() {
    let sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let matches = sel.select_all(&[Capability::Streaming]);
    assert!(matches.is_empty());
}

#[test]
fn t076_selector_select_all_matches() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate(
        "b",
        vec![Capability::Streaming, Capability::ToolRead],
        1,
    ));
    let matches = sel.select_all(&[Capability::Streaming]);
    assert_eq!(matches.len(), 2);
}

#[test]
fn t077_selector_select_all_filters_incapable() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate("b", vec![Capability::ToolRead], 1));
    let matches = sel.select_all(&[Capability::Streaming]);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "a");
}

#[test]
fn t078_selector_select_all_disabled_excluded() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut c = mock_candidate("a", vec![Capability::Streaming], 0);
    c.enabled = false;
    sel.add_candidate(c);
    let matches = sel.select_all(&[Capability::Streaming]);
    assert!(matches.is_empty());
}

#[test]
fn t079_selector_select_none_when_no_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 0));
    assert!(sel.select(&[Capability::McpClient]).is_none());
}

#[test]
fn t080_selector_select_first_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("first", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate("second", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "first");
}

#[test]
fn t081_selector_best_fit_prefers_more_caps() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(mock_candidate("narrow", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate(
        "broad",
        vec![Capability::Streaming, Capability::ToolRead],
        1,
    ));
    let chosen = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(chosen.name, "broad");
}

#[test]
fn t082_selector_priority_picks_lowest() {
    let mut sel = BackendSelector::new(SelectionStrategy::Priority);
    sel.add_candidate(mock_candidate("high", vec![Capability::Streaming], 10));
    sel.add_candidate(mock_candidate("low", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "low");
}

#[test]
fn t083_selector_round_robin_rotates() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate("b", vec![Capability::Streaming], 0));
    let first = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let second = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    assert_ne!(first, second);
}

#[test]
fn t084_selector_round_robin_wraps() {
    let mut sel = BackendSelector::new(SelectionStrategy::RoundRobin);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate("b", vec![Capability::Streaming], 0));
    let n1 = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let _n2 = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    let n3 = sel.select(&[Capability::Streaming]).unwrap().name.clone();
    assert_eq!(n1, n3);
}

#[test]
fn t085_selector_least_loaded_picks_low_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLoaded);
    sel.add_candidate(mock_candidate("heavy", vec![Capability::Streaming], 100));
    sel.add_candidate(mock_candidate("light", vec![Capability::Streaming], 1));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "light");
}

// ===========================================================================
// Category 6: Backend selection by name / detailed result (tests 86–100)
// ===========================================================================

#[test]
fn t086_select_with_result_success() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("mock", vec![Capability::Streaming], 0));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "mock");
    assert!(result.unmet_capabilities.is_empty());
}

#[test]
fn t087_select_with_result_no_match() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("mock", vec![Capability::Streaming], 0));
    let result = sel.select_with_result(&[Capability::McpClient]);
    assert!(result.selected.is_empty());
    assert!(!result.unmet_capabilities.is_empty());
}

#[test]
fn t088_select_with_result_alternatives() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![Capability::Streaming], 0));
    sel.add_candidate(mock_candidate("b", vec![Capability::Streaming], 1));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert_eq!(result.selected, "a");
    assert!(result.alternatives.contains(&"b".to_string()));
}

#[test]
fn t089_select_with_result_reason_contains_strategy() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("mock", vec![Capability::Streaming], 0));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert!(result.reason.contains("FirstMatch"));
}

#[test]
fn t090_select_with_result_empty_requirements() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("mock", vec![Capability::Streaming], 0));
    let result = sel.select_with_result(&[]);
    assert_eq!(result.selected, "mock");
}

#[test]
fn t091_select_with_result_multiple_unmet() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("mock", vec![], 0));
    let result = sel.select_with_result(&[Capability::Streaming, Capability::McpClient]);
    assert_eq!(result.unmet_capabilities.len(), 2);
}

#[test]
fn t092_selector_metadata_preserved() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut c = mock_candidate("meta", vec![Capability::Streaming], 0);
    c.metadata.insert("region".into(), "us-east".into());
    sel.add_candidate(c);
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.metadata.get("region").unwrap(), "us-east");
}

#[test]
fn t093_selector_empty_select_none() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    assert!(sel.select(&[]).is_none()); // no candidates at all
}

#[test]
fn t094_selector_all_disabled_returns_none() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let mut c = mock_candidate("a", vec![Capability::Streaming], 0);
    c.enabled = false;
    sel.add_candidate(c);
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

#[test]
fn t095_selector_multiple_capabilities_required() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate(
        "full",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
        0,
    ));
    sel.add_candidate(mock_candidate("partial", vec![Capability::Streaming], 1));
    let chosen = sel
        .select(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(chosen.name, "full");
}

#[test]
fn t096_select_with_result_unmet_reason_message() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("mock", vec![], 0));
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert!(result.reason.contains("unmet"));
}

#[test]
fn t097_selector_candidate_priority_field() {
    let c = mock_candidate("a", vec![], 42);
    assert_eq!(c.priority, 42);
}

#[test]
fn t098_selector_candidate_enabled_default() {
    let c = mock_candidate("a", vec![], 0);
    assert!(c.enabled);
}

#[test]
fn t099_selector_select_all_empty_requirements() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(mock_candidate("a", vec![], 0));
    // All candidates satisfy empty requirements
    let matches = sel.select_all(&[]);
    assert_eq!(matches.len(), 1);
}

#[test]
fn t100_selector_best_fit_single_candidate() {
    let mut sel = BackendSelector::new(SelectionStrategy::BestFit);
    sel.add_candidate(mock_candidate("only", vec![Capability::Streaming], 0));
    let chosen = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(chosen.name, "only");
}

// ===========================================================================
// Category 7: Backend capabilities declaration (tests 101–115)
// ===========================================================================

#[test]
fn t101_ensure_requirements_satisfied() {
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
fn t102_ensure_requirements_unsatisfied() {
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
fn t103_ensure_requirements_empty_ok() {
    let reqs = CapabilityRequirements { required: vec![] };
    let caps = MockBackend.capabilities();
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t104_ensure_requirements_native_needed_emulated_given() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::ToolRead,
            min_support: MinSupport::Native,
        }],
    };
    let caps = MockBackend.capabilities();
    // ToolRead is Emulated, Native required → should fail
    assert!(ensure_capability_requirements(&reqs, &caps).is_err());
}

#[test]
fn t105_ensure_requirements_emulated_needed_native_given() {
    let reqs = CapabilityRequirements {
        required: vec![CapabilityRequirement {
            capability: Capability::Streaming,
            min_support: MinSupport::Emulated,
        }],
    };
    let caps = MockBackend.capabilities();
    // Streaming is Native, Emulated required → should pass
    assert!(ensure_capability_requirements(&reqs, &caps).is_ok());
}

#[test]
fn t106_extract_execution_mode_default() {
    let wo = base_work_order();
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn t107_extract_execution_mode_passthrough() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "passthrough"}));
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[test]
fn t108_extract_execution_mode_mapped_explicit() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp".into(), json!({"mode": "mapped"}));
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Mapped);
}

#[test]
fn t109_extract_execution_mode_flat_key() {
    let mut wo = base_work_order();
    wo.config
        .vendor
        .insert("abp.mode".into(), json!("passthrough"));
    let mode = extract_execution_mode(&wo);
    assert_eq!(mode, ExecutionMode::Passthrough);
}

#[test]
fn t110_validate_passthrough_compatibility_ok() {
    let wo = base_work_order();
    assert!(validate_passthrough_compatibility(&wo).is_ok());
}

#[test]
fn t111_capability_matrix_new_empty() {
    let matrix = CapabilityMatrix::new();
    assert!(matrix.is_empty());
    assert_eq!(matrix.backend_count(), 0);
}

#[test]
fn t112_capability_matrix_register() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    assert_eq!(matrix.backend_count(), 1);
    assert!(matrix.supports("mock", &Capability::Streaming));
}

#[test]
fn t113_capability_matrix_backends_for() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("b", vec![Capability::Streaming, Capability::ToolRead]);
    matrix.register("c", vec![Capability::ToolRead]);
    let backends = matrix.backends_for(&Capability::Streaming);
    assert_eq!(backends.len(), 2);
}

#[test]
fn t114_capability_matrix_common_capabilities() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming, Capability::ToolRead]);
    matrix.register("b", vec![Capability::Streaming]);
    let common = matrix.common_capabilities();
    assert!(common.contains(&Capability::Streaming));
    assert!(!common.contains(&Capability::ToolRead));
}

#[test]
fn t115_capability_matrix_evaluate() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("mock", vec![Capability::Streaming, Capability::ToolRead]);
    let report = matrix.evaluate("mock", &[Capability::Streaming, Capability::McpClient]);
    assert_eq!(report.supported.len(), 1);
    assert_eq!(report.missing.len(), 1);
    assert!((report.score - 0.5).abs() < f64::EPSILON);
}

// ===========================================================================
// Category 8: Edge cases (tests 116–135)
// ===========================================================================

#[tokio::test]
async fn t116_concurrent_mock_runs() {
    let backend = Arc::new(MockBackend);
    let mut handles = Vec::new();
    for _ in 0..10 {
        let b = Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            let (tx, _rx) = mpsc::channel(64);
            b.run(Uuid::new_v4(), base_work_order(), tx).await.unwrap()
        }));
    }
    let mut hashes = std::collections::HashSet::new();
    for h in handles {
        let receipt = h.await.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
        hashes.insert(receipt.receipt_sha256.clone().unwrap());
    }
    // All should have unique hashes
    assert_eq!(hashes.len(), 10);
}

#[tokio::test]
async fn t117_dropped_receiver_run_succeeds() {
    let backend = MockBackend;
    let run_id = Uuid::new_v4();
    let (tx, rx) = mpsc::channel(64);
    drop(rx); // drop receiver immediately
              // Run should still succeed (sends fail silently)
    let result = backend.run(run_id, base_work_order(), tx).await;
    assert!(result.is_ok());
}

#[test]
fn t118_capability_matrix_unknown_backend() {
    let matrix = CapabilityMatrix::new();
    assert!(!matrix.supports("unknown", &Capability::Streaming));
}

#[test]
fn t119_capability_matrix_best_backend() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("partial", vec![Capability::Streaming]);
    matrix.register(
        "full",
        vec![
            Capability::Streaming,
            Capability::ToolRead,
            Capability::ToolWrite,
        ],
    );
    let best = matrix
        .best_backend(&[Capability::Streaming, Capability::ToolRead])
        .unwrap();
    assert_eq!(best, "full");
}

#[test]
fn t120_capability_matrix_all_capabilities() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    let caps = matrix.all_capabilities("a").unwrap();
    assert!(caps.contains(&Capability::Streaming));
    assert!(matrix.all_capabilities("unknown").is_none());
}

#[test]
fn t121_metrics_initial_state() {
    let m = BackendMetrics::new();
    assert_eq!(m.total_runs(), 0);
    assert_eq!(m.success_rate(), 0.0);
    assert_eq!(m.average_duration_ms(), 0.0);
}

#[test]
fn t122_metrics_record_run() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    assert_eq!(m.total_runs(), 1);
    assert!((m.success_rate() - 1.0).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 100.0).abs() < f64::EPSILON);
}

#[test]
fn t123_metrics_mixed_success_failure() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    m.record_run(false, 2, 200);
    assert_eq!(m.total_runs(), 2);
    assert!((m.success_rate() - 0.5).abs() < f64::EPSILON);
    assert!((m.average_duration_ms() - 150.0).abs() < f64::EPSILON);
}

#[test]
fn t124_metrics_reset() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    m.reset();
    assert_eq!(m.total_runs(), 0);
    assert_eq!(m.success_rate(), 0.0);
}

#[test]
fn t125_metrics_snapshot() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 500);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 1);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 0);
    assert_eq!(snap.total_events, 10);
    assert_eq!(snap.total_duration_ms, 500);
}

#[test]
fn t126_metrics_registry_get_or_create() {
    let reg = MetricsRegistry::new();
    let m1 = reg.get_or_create("mock");
    let m2 = reg.get_or_create("mock");
    m1.record_run(true, 1, 10);
    assert_eq!(m2.total_runs(), 1); // Same Arc
}

#[test]
fn t127_metrics_registry_snapshot_all() {
    let reg = MetricsRegistry::new();
    reg.get_or_create("a").record_run(true, 1, 10);
    reg.get_or_create("b").record_run(false, 2, 20);
    let snaps = reg.snapshot_all();
    assert_eq!(snaps.len(), 2);
    assert_eq!(snaps["a"].successful_runs, 1);
    assert_eq!(snaps["b"].failed_runs, 1);
}

#[test]
fn t128_health_checker_empty_healthy() {
    let checker = HealthChecker::new();
    assert!(checker.is_healthy());
    assert_eq!(checker.check_count(), 0);
}

#[test]
fn t129_health_checker_add_healthy() {
    let mut checker = HealthChecker::new();
    checker.add_check("mock", HealthStatus::Healthy);
    assert!(checker.is_healthy());
    assert_eq!(checker.check_count(), 1);
}

#[test]
fn t130_health_checker_degraded() {
    let mut checker = HealthChecker::new();
    checker.add_check("mock", HealthStatus::Healthy);
    checker.add_check(
        "slow",
        HealthStatus::Degraded {
            reason: "high latency".into(),
        },
    );
    assert!(!checker.is_healthy());
    let overall = checker.overall_status();
    assert!(matches!(overall, HealthStatus::Degraded { .. }));
}

#[test]
fn t131_health_checker_unhealthy_worst() {
    let mut checker = HealthChecker::new();
    checker.add_check(
        "down",
        HealthStatus::Unhealthy {
            reason: "unreachable".into(),
        },
    );
    checker.add_check(
        "slow",
        HealthStatus::Degraded {
            reason: "high latency".into(),
        },
    );
    let overall = checker.overall_status();
    assert!(matches!(overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn t132_health_checker_clear() {
    let mut checker = HealthChecker::new();
    checker.add_check("mock", HealthStatus::Healthy);
    checker.clear();
    assert_eq!(checker.check_count(), 0);
}

#[test]
fn t133_health_checker_unhealthy_checks() {
    let mut checker = HealthChecker::new();
    checker.add_check("ok", HealthStatus::Healthy);
    checker.add_check(
        "bad",
        HealthStatus::Unhealthy {
            reason: "dead".into(),
        },
    );
    let unhealthy = checker.unhealthy_checks();
    assert_eq!(unhealthy.len(), 1);
    assert_eq!(unhealthy[0].name, "bad");
}

#[test]
fn t134_metrics_average_events_per_run() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 100);
    m.record_run(true, 20, 200);
    assert!((m.average_events_per_run() - 15.0).abs() < f64::EPSILON);
}

#[test]
fn t135_capability_matrix_register_merges() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("a", vec![Capability::Streaming]);
    matrix.register("a", vec![Capability::ToolRead]);
    assert!(matrix.supports("a", &Capability::Streaming));
    assert!(matrix.supports("a", &Capability::ToolRead));
    assert_eq!(matrix.backend_count(), 1);
}

#[test]
fn t136_capability_matrix_common_empty() {
    let matrix = CapabilityMatrix::new();
    assert!(matrix.common_capabilities().is_empty());
}

#[test]
fn t137_capability_matrix_evaluate_no_requirements() {
    let mut matrix = CapabilityMatrix::new();
    matrix.register("mock", vec![Capability::Streaming]);
    let report = matrix.evaluate("mock", &[]);
    assert!((report.score - 1.0).abs() < f64::EPSILON);
}

#[test]
fn t138_capability_matrix_evaluate_unknown() {
    let matrix = CapabilityMatrix::new();
    let report = matrix.evaluate("unknown", &[Capability::Streaming]);
    assert_eq!(report.score, 0.0);
    assert_eq!(report.missing.len(), 1);
}

#[tokio::test]
async fn t139_sequential_mock_runs_independent() {
    let backend = MockBackend;
    let (r1, e1) = run_mock(&backend, work_order_with_task("first")).await;
    let (r2, e2) = run_mock(&backend, work_order_with_task("second")).await;
    assert_eq!(e1.len(), e2.len());
    assert_ne!(r1.meta.run_id, r2.meta.run_id);
    assert_ne!(r1.meta.work_order_id, r2.meta.work_order_id);
}

#[tokio::test]
async fn t140_mock_with_requirements_succeeds() {
    let backend = MockBackend;
    let wo = WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        })
        .build();
    let (receipt, _) = run_mock(&backend, wo).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
}

#[tokio::test]
async fn t141_mock_with_unsatisfied_requirements_fails() {
    let backend = MockBackend;
    let run_id = Uuid::new_v4();
    let wo = WorkOrderBuilder::new("test")
        .requirements(CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        })
        .build();
    let (tx, _rx) = mpsc::channel(64);
    let result = backend.run(run_id, wo, tx).await;
    assert!(result.is_err());
}

#[test]
fn t142_metrics_debug_format() {
    let m = BackendMetrics::new();
    let debug = format!("{m:?}");
    assert!(debug.contains("BackendMetrics"));
}

#[test]
fn t143_metrics_registry_debug_format() {
    let reg = MetricsRegistry::new();
    reg.get_or_create("mock");
    let debug = format!("{reg:?}");
    assert!(debug.contains("MetricsRegistry"));
}

#[test]
fn t144_health_status_serde_roundtrip() {
    let status = HealthStatus::Degraded {
        reason: "slow".into(),
    };
    let json = serde_json::to_string(&status).unwrap();
    let back: HealthStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(status, back);
}

#[test]
fn t145_selection_strategy_serde_roundtrip() {
    let s = SelectionStrategy::RoundRobin;
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("round_robin"));
}

#[test]
fn t146_backend_candidate_serde_roundtrip() {
    let c = mock_candidate("test", vec![Capability::Streaming], 5);
    let json = serde_json::to_string(&c).unwrap();
    let back: BackendCandidate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "test");
    assert_eq!(back.priority, 5);
}

#[test]
fn t147_metrics_snapshot_serde() {
    let m = BackendMetrics::new();
    m.record_run(true, 4, 100);
    let snap = m.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let _: abp_integrations::metrics::MetricsSnapshot = serde_json::from_str(&json).unwrap();
}

#[test]
fn t148_selector_select_with_result_empty_selector() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    let result = sel.select_with_result(&[Capability::Streaming]);
    assert!(result.selected.is_empty());
}

#[test]
fn t149_capability_matrix_best_backend_empty() {
    let matrix = CapabilityMatrix::new();
    assert!(matrix.best_backend(&[Capability::Streaming]).is_none());
}

#[tokio::test]
async fn t150_mock_large_task_string() {
    let long_task = "a".repeat(10_000);
    let wo = WorkOrderBuilder::new(&long_task).build();
    let (receipt, events) = run_mock(&MockBackend, wo).await;
    assert_eq!(receipt.outcome, Outcome::Complete);
    if let AgentEventKind::RunStarted { message } = &events[0].kind {
        assert!(message.contains(&long_task));
    }
}
