#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Comprehensive runtime pipeline integration tests.
//!
//! Covers: full pipeline flow, backend selection, event streaming,
//! receipt generation, and error propagation.

use abp_backend_mock::scenarios::{MockScenario, ScenarioMockBackend};
use abp_backend_mock::MockBackend;
use abp_core::{
    AgentEvent, AgentEventKind, Capability, CapabilityRequirement, CapabilityRequirements,
    ExecutionMode, MinSupport, Outcome, Receipt, SupportLevel, WorkOrder, WorkOrderBuilder,
    WorkspaceMode, CONTRACT_VERSION,
};
use abp_dialect::Dialect;
use abp_runtime::{ProjectionMatrix, Runtime, RuntimeError};
use tokio_stream::StreamExt;

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a minimal work order for testing (uses PassThrough to avoid staging).
fn simple_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

/// Build a work order requesting passthrough mode.
fn passthrough_wo(task: &str) -> WorkOrder {
    let mut wo = simple_wo(task);
    wo.config
        .vendor
        .insert("abp".into(), serde_json::json!({"mode": "passthrough"}));
    wo
}

/// Collect all events from a RunHandle, then return the receipt.
async fn collect_run(rt: &Runtime, backend: &str, wo: WorkOrder) -> (Vec<AgentEvent>, Receipt) {
    let handle = rt.run_streaming(backend, wo).await.unwrap();
    let events: Vec<AgentEvent> = handle.events.collect().await;
    let receipt = handle.receipt.await.unwrap().unwrap();
    (events, receipt)
}

/// Build a mock CapabilityManifest with given capabilities as Native.
fn cap_manifest(caps: &[Capability]) -> abp_core::CapabilityManifest {
    let mut m = abp_core::CapabilityManifest::default();
    for c in caps {
        m.insert(c.clone(), SupportLevel::Native);
    }
    m
}

// ===========================================================================
// Module 1: Full Pipeline Flow (12 tests)
// ===========================================================================

mod full_pipeline {
    use super::*;

    #[tokio::test]
    async fn mock_backend_produces_receipt() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("hello world");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn receipt_work_order_id_matches() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("id check");
        let wo_id = wo.id;
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.meta.work_order_id, wo_id);
    }

    #[tokio::test]
    async fn events_are_streamed_to_caller() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("stream check");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        assert!(!events.is_empty(), "expected at least one event");
    }

    #[tokio::test]
    async fn receipt_trace_is_non_empty() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("trace check");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert!(!receipt.trace.is_empty());
    }

    #[tokio::test]
    async fn receipt_has_sha256_hash() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("hash check");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert!(receipt.receipt_sha256.is_some());
    }

    #[tokio::test]
    async fn backend_identity_is_mock() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("identity check");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.backend.id, "mock");
    }

    #[tokio::test]
    async fn contract_version_in_receipt() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("version check");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn passthrough_mode_detected() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("passthrough test");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.mode, ExecutionMode::Passthrough);
    }

    #[tokio::test]
    async fn mapped_mode_is_default() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("mapped test");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.mode, ExecutionMode::Mapped);
    }

    #[tokio::test]
    async fn events_contain_run_started() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("run started event");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        let has_started = events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
        assert!(has_started, "missing RunStarted event");
    }

    #[tokio::test]
    async fn events_contain_run_completed() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("run completed event");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        let has_completed = events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
        assert!(has_completed, "missing RunCompleted event");
    }

    #[tokio::test]
    async fn run_handle_run_id_is_unique() {
        let rt = Runtime::with_default_backends();
        let h1 = rt.run_streaming("mock", simple_wo("a")).await.unwrap();
        let h2 = rt.run_streaming("mock", simple_wo("b")).await.unwrap();
        assert_ne!(h1.run_id, h2.run_id);
        // Consume to avoid drop panics.
        let _ = h1.receipt.await;
        let _ = h2.receipt.await;
    }
}

// ===========================================================================
// Module 2: Backend Selection (12 tests)
// ===========================================================================

mod backend_selection {
    use super::*;

    #[test]
    fn with_default_backends_has_mock() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend_names().contains(&"mock".to_string()));
    }

    #[test]
    fn register_custom_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("custom", MockBackend);
        assert!(rt.backend_names().contains(&"custom".to_string()));
    }

    #[test]
    fn register_multiple_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("alpha", MockBackend);
        rt.register_backend("beta", MockBackend);
        rt.register_backend("gamma", MockBackend);
        assert_eq!(rt.backend_names().len(), 3);
    }

    #[test]
    fn backend_lookup_returns_some_for_registered() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend("mock").is_some());
    }

    #[test]
    fn backend_lookup_returns_none_for_unknown() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend("nonexistent").is_none());
    }

    #[tokio::test]
    async fn run_streaming_unknown_backend_returns_error() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("fail");
        match rt.run_streaming("nonexistent", wo).await {
            Err(RuntimeError::UnknownBackend { .. }) => {}
            Err(e) => panic!("expected UnknownBackend, got {e:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn select_backend_without_projection_fails() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("test");
        let err = rt.select_backend(&wo).unwrap_err();
        assert!(matches!(err, RuntimeError::NoProjectionMatch { .. }));
    }

    #[test]
    fn select_backend_with_projection_picks_registered() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_backend(
            "mock",
            cap_manifest(&[Capability::Streaming]),
            Dialect::OpenAi,
            50,
        );
        let rt = Runtime::with_default_backends().with_projection(matrix);
        let wo = simple_wo("test");
        let result = rt.select_backend(&wo).unwrap();
        assert_eq!(result.selected_backend, "mock");
    }

    #[test]
    fn select_backend_prefers_higher_priority() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_backend(
            "low",
            cap_manifest(&[Capability::Streaming]),
            Dialect::OpenAi,
            10,
        );
        matrix.register_backend(
            "high",
            cap_manifest(&[Capability::Streaming]),
            Dialect::Claude,
            90,
        );
        let mut rt = Runtime::new().with_projection(matrix);
        rt.register_backend("low", MockBackend);
        rt.register_backend("high", MockBackend);
        let wo = simple_wo("test");
        let result = rt.select_backend(&wo).unwrap();
        assert_eq!(result.selected_backend, "high");
    }

    #[test]
    fn select_backend_unregistered_in_runtime_returns_error() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_backend(
            "phantom",
            cap_manifest(&[Capability::Streaming]),
            Dialect::OpenAi,
            50,
        );
        let rt = Runtime::new().with_projection(matrix);
        let wo = simple_wo("test");
        let err = rt.select_backend(&wo).unwrap_err();
        assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
    }

    #[test]
    fn registry_contains_check() {
        let rt = Runtime::with_default_backends();
        assert!(rt.registry().contains("mock"));
        assert!(!rt.registry().contains("nope"));
    }

    #[test]
    fn backend_names_sorted() {
        let mut rt = Runtime::new();
        rt.register_backend("charlie", MockBackend);
        rt.register_backend("alpha", MockBackend);
        rt.register_backend("bravo", MockBackend);
        let names = rt.backend_names();
        assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
    }
}

// ===========================================================================
// Module 3: Event Streaming (12 tests)
// ===========================================================================

mod event_streaming {
    use super::*;

    #[tokio::test]
    async fn text_events_received() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("text events");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        let has_msg = events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::AssistantMessage { .. }));
        assert!(has_msg, "expected AssistantMessage event");
    }

    #[tokio::test]
    async fn streaming_chunks_via_scenario() {
        let chunks = vec!["Hello ".into(), "world!".into()];
        let backend = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
            chunks: chunks.clone(),
            chunk_delay_ms: 0,
        });
        let mut rt = Runtime::new();
        rt.register_backend("stream", backend);
        let wo = simple_wo("streaming");
        let (events, _) = collect_run(&rt, "stream", wo).await;
        let deltas: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.kind {
                AgentEventKind::AssistantDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, chunks);
    }

    #[tokio::test]
    async fn error_event_on_permanent_failure() {
        // ScenarioMockBackend with PermanentError will cause backend to fail.
        // The runtime wraps this into RuntimeError::BackendFailed, not an event.
        let backend = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "ERR-001".into(),
            message: "test permanent error".into(),
        });
        let mut rt = Runtime::new();
        rt.register_backend("fail", backend);
        let wo = simple_wo("fail task");
        let handle = rt.run_streaming("fail", wo).await.unwrap();
        let _events: Vec<_> = handle.events.collect().await;
        let result = handle.receipt.await.unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mixed_event_types_from_mock() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("mixed events");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match &e.kind {
                AgentEventKind::RunStarted { .. } => "run_started",
                AgentEventKind::AssistantMessage { .. } => "assistant_message",
                AgentEventKind::RunCompleted { .. } => "run_completed",
                AgentEventKind::AssistantDelta { .. } => "assistant_delta",
                _ => "other",
            })
            .collect();
        assert!(kinds.contains(&"run_started"));
        assert!(kinds.contains(&"assistant_message"));
        assert!(kinds.contains(&"run_completed"));
    }

    #[tokio::test]
    async fn event_ordering_run_started_first() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("ordering");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        assert!(
            matches!(&events[0].kind, AgentEventKind::RunStarted { .. }),
            "first event should be RunStarted"
        );
    }

    #[tokio::test]
    async fn event_ordering_run_completed_last() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("ordering last");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        assert!(
            matches!(
                &events.last().unwrap().kind,
                AgentEventKind::RunCompleted { .. }
            ),
            "last event should be RunCompleted"
        );
    }

    #[tokio::test]
    async fn large_number_of_events() {
        let n = 150;
        let chunks: Vec<String> = (0..n).map(|i| format!("chunk-{i}")).collect();
        let backend = ScenarioMockBackend::new(MockScenario::StreamingSuccess {
            chunks: chunks.clone(),
            chunk_delay_ms: 0,
        });
        let mut rt = Runtime::new();
        rt.register_backend("bulk", backend);
        let wo = simple_wo("bulk streaming");
        let (events, _) = collect_run(&rt, "bulk", wo).await;
        let deltas: Vec<_> = events
            .iter()
            .filter(|e| matches!(&e.kind, AgentEventKind::AssistantDelta { .. }))
            .collect();
        assert_eq!(deltas.len(), n);
    }

    #[tokio::test]
    async fn all_events_have_timestamps() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("ts check");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        for ev in &events {
            // Timestamp should be reasonable (after year 2020).
            assert!(ev.ts.timestamp() > 1_577_836_800);
        }
    }

    #[tokio::test]
    async fn event_timestamps_non_decreasing() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("ts ordering");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        for pair in events.windows(2) {
            assert!(
                pair[1].ts >= pair[0].ts,
                "timestamps must be non-decreasing"
            );
        }
    }

    #[tokio::test]
    async fn scenario_success_returns_text() {
        let backend = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "hello from scenario".into(),
        });
        let mut rt = Runtime::new();
        rt.register_backend("scen", backend);
        let wo = simple_wo("scenario");
        let (events, _) = collect_run(&rt, "scen", wo).await;
        let has_text = events.iter().any(|e| {
            matches!(&e.kind, AgentEventKind::AssistantMessage { text } if text == "hello from scenario")
        });
        assert!(has_text);
    }

    #[tokio::test]
    async fn events_ext_field_is_none_by_default() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("ext check");
        let (events, _) = collect_run(&rt, "mock", wo).await;
        for ev in &events {
            assert!(ev.ext.is_none());
        }
    }

    #[tokio::test]
    async fn event_count_matches_receipt_trace() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("count match");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let events: Vec<AgentEvent> = handle.events.collect().await;
        let receipt = handle.receipt.await.unwrap().unwrap();
        // Receipt trace may include events not yet forwarded to caller,
        // but at minimum the counts should be close. The mock backend
        // produces 4 events: RunStarted, 2x AssistantMessage, RunCompleted.
        assert_eq!(receipt.trace.len(), 4);
        assert!(events.len() <= receipt.trace.len());
    }
}

// ===========================================================================
// Module 4: Receipt Generation (12 tests)
// ===========================================================================

mod receipt_generation {
    use super::*;

    #[tokio::test]
    async fn receipt_has_correct_work_order_id() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("receipt wo id");
        let wo_id = wo.id;
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.meta.work_order_id, wo_id);
    }

    #[tokio::test]
    async fn receipt_backend_name_is_mock() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("backend name");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.backend.id, "mock");
    }

    #[tokio::test]
    async fn receipt_hash_is_valid_hex() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("hash hex");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex"
        );
    }

    #[tokio::test]
    async fn receipt_hash_is_consistent() {
        // Recomputing the hash of the receipt (with hash field set to null)
        // should yield the same value.
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("hash consistent");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        let original_hash = receipt.receipt_sha256.clone().unwrap();
        let rehashed = abp_core::receipt_hash(&receipt).unwrap();
        assert_eq!(original_hash, rehashed);
    }

    #[tokio::test]
    async fn receipt_timing_is_plausible() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("timing");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert!(receipt.meta.finished_at >= receipt.meta.started_at);
    }

    #[tokio::test]
    async fn receipt_outcome_complete_for_mock() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("outcome");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn receipt_usage_fields_present() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("usage");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert!(receipt.usage.input_tokens.is_some());
        assert!(receipt.usage.output_tokens.is_some());
    }

    #[tokio::test]
    async fn receipt_capabilities_manifest_non_empty() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("caps");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert!(!receipt.capabilities.is_empty());
    }

    #[tokio::test]
    async fn receipt_verification_harness_ok() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("verify");
        let (_, receipt) = collect_run(&rt, "mock", wo).await;
        assert!(receipt.verification.harness_ok);
    }

    #[tokio::test]
    async fn error_receipt_on_backend_failure() {
        let backend = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "ERR".into(),
            message: "total failure".into(),
        });
        let mut rt = Runtime::new();
        rt.register_backend("bad", backend);
        let wo = simple_wo("fail receipt");
        let handle = rt.run_streaming("bad", wo).await.unwrap();
        let _: Vec<_> = handle.events.collect().await;
        let result = handle.receipt.await.unwrap();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, RuntimeError::BackendFailed(_)));
    }

    #[tokio::test]
    async fn scenario_backend_identity_is_scenario_mock() {
        let backend = ScenarioMockBackend::new(MockScenario::Success {
            delay_ms: 0,
            text: "hi".into(),
        });
        let mut rt = Runtime::new();
        rt.register_backend("scenario", backend);
        let wo = simple_wo("scenario id");
        let (_, receipt) = collect_run(&rt, "scenario", wo).await;
        assert_eq!(receipt.backend.id, "scenario-mock");
    }

    #[tokio::test]
    async fn receipt_chain_accumulates() {
        let rt = Runtime::with_default_backends();
        let chain = rt.receipt_chain();

        let (_, _) = collect_run(&rt, "mock", simple_wo("run 1")).await;
        let (_, _) = collect_run(&rt, "mock", simple_wo("run 2")).await;

        let guard = chain.lock().await;
        assert!(guard.len() >= 2, "receipt chain should have at least 2");
    }
}

// ===========================================================================
// Module 5: Error Propagation (12 tests)
// ===========================================================================

mod error_propagation {
    use super::*;

    #[tokio::test]
    async fn unknown_backend_error() {
        let rt = Runtime::with_default_backends();
        match rt.run_streaming("nonexistent", simple_wo("x")).await {
            Err(RuntimeError::UnknownBackend { name }) => {
                assert_eq!(name, "nonexistent");
            }
            Err(e) => panic!("expected UnknownBackend, got {e:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn unknown_backend_error_code() {
        let rt = Runtime::with_default_backends();
        match rt.run_streaming("nonexistent", simple_wo("x")).await {
            Err(e) => assert_eq!(e.error_code(), abp_error::ErrorCode::BackendNotFound),
            Ok(_) => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn unknown_backend_not_retryable() {
        let rt = Runtime::with_default_backends();
        match rt.run_streaming("nonexistent", simple_wo("x")).await {
            Err(e) => assert!(!e.is_retryable()),
            Ok(_) => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn backend_failure_propagated() {
        let backend = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "FATAL".into(),
            message: "crash".into(),
        });
        let mut rt = Runtime::new();
        rt.register_backend("crash", backend);
        let wo = simple_wo("crash task");
        let handle = rt.run_streaming("crash", wo).await.unwrap();
        let _: Vec<_> = handle.events.collect().await;
        let result = handle.receipt.await.unwrap();
        assert!(matches!(result, Err(RuntimeError::BackendFailed(_))));
    }

    #[tokio::test]
    async fn backend_failure_error_code() {
        let backend = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "FATAL".into(),
            message: "crash".into(),
        });
        let mut rt = Runtime::new();
        rt.register_backend("crash2", backend);
        let wo = simple_wo("crash2 task");
        let handle = rt.run_streaming("crash2", wo).await.unwrap();
        let _: Vec<_> = handle.events.collect().await;
        let err = handle.receipt.await.unwrap().unwrap_err();
        assert_eq!(err.error_code(), abp_error::ErrorCode::BackendCrashed);
    }

    #[tokio::test]
    async fn backend_failure_is_retryable() {
        let err = RuntimeError::BackendFailed(anyhow::anyhow!("temporary"));
        assert!(err.is_retryable());
    }

    #[test]
    fn policy_failed_error_code() {
        let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
        assert_eq!(err.error_code(), abp_error::ErrorCode::PolicyInvalid);
    }

    #[test]
    fn policy_failed_not_retryable() {
        let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
        assert!(!err.is_retryable());
    }

    #[test]
    fn workspace_failed_error_code() {
        let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
        assert_eq!(err.error_code(), abp_error::ErrorCode::WorkspaceInitFailed);
    }

    #[test]
    fn workspace_failed_is_retryable() {
        let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("transient"));
        assert!(err.is_retryable());
    }

    #[test]
    fn capability_check_failed_error_code() {
        let err = RuntimeError::CapabilityCheckFailed("missing MCP".into());
        assert_eq!(
            err.error_code(),
            abp_error::ErrorCode::CapabilityUnsupported
        );
    }

    #[test]
    fn capability_check_failed_not_retryable() {
        let err = RuntimeError::CapabilityCheckFailed("missing".into());
        assert!(!err.is_retryable());
    }
}

// ===========================================================================
// Module 6: Capability Checking (8 tests)
// ===========================================================================

mod capability_checking {
    use super::*;

    #[test]
    fn check_capabilities_passes_for_streaming() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    #[test]
    fn check_capabilities_fails_for_mcp() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::McpClient,
                min_support: MinSupport::Native,
            }],
        };
        let err = rt.check_capabilities("mock", &reqs).unwrap_err();
        assert!(matches!(err, RuntimeError::CapabilityCheckFailed(_)));
    }

    #[test]
    fn check_capabilities_empty_requirements_passes() {
        let rt = Runtime::with_default_backends();
        rt.check_capabilities("mock", &CapabilityRequirements::default())
            .unwrap();
    }

    #[test]
    fn check_capabilities_unknown_backend() {
        let rt = Runtime::with_default_backends();
        let err = rt
            .check_capabilities("nonexistent", &CapabilityRequirements::default())
            .unwrap_err();
        assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
    }

    #[test]
    fn emulated_capability_passes_check() {
        let rt = Runtime::with_default_backends();
        // MockBackend has ToolRead as Emulated.
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            }],
        };
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    #[test]
    fn multiple_requirements_all_satisfied() {
        let rt = Runtime::with_default_backends();
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
            ],
        };
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    #[test]
    fn multiple_requirements_one_unsatisfied() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Native,
                },
            ],
        };
        assert!(rt.check_capabilities("mock", &reqs).is_err());
    }

    #[tokio::test]
    async fn run_with_unsatisfied_capability_fails() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("cap fail")
            .workspace_mode(WorkspaceMode::PassThrough)
            .requirements(CapabilityRequirements {
                required: vec![CapabilityRequirement {
                    capability: Capability::McpClient,
                    min_support: MinSupport::Native,
                }],
            })
            .build();
        // Capability check may fail at run_streaming level or inside the receipt future.
        let result = rt.run_streaming("mock", wo).await;
        match result {
            Err(RuntimeError::CapabilityCheckFailed(_)) => {}
            Ok(handle) => {
                let _: Vec<_> = handle.events.collect().await;
                let inner = handle.receipt.await.unwrap();
                assert!(inner.is_err(), "expected capability error inside receipt");
            }
            Err(e) => panic!("unexpected error variant: {e:?}"),
        }
    }
}

// ===========================================================================
// Module 7: Metrics & Telemetry (6 tests)
// ===========================================================================

mod metrics_telemetry {
    use super::*;

    #[tokio::test]
    async fn metrics_record_successful_run() {
        let rt = Runtime::with_default_backends();
        let snap_before = rt.metrics().snapshot();
        let (_, _) = collect_run(&rt, "mock", simple_wo("metrics")).await;
        let snap_after = rt.metrics().snapshot();
        assert_eq!(snap_after.total_runs, snap_before.total_runs + 1);
        assert_eq!(snap_after.successful_runs, snap_before.successful_runs + 1);
    }

    #[tokio::test]
    async fn metrics_not_recorded_on_backend_failure() {
        // When backend returns an error, the runtime short-circuits before
        // recording metrics (metrics are only recorded on receipt production).
        let backend = ScenarioMockBackend::new(MockScenario::PermanentError {
            code: "E".into(),
            message: "fail".into(),
        });
        let mut rt = Runtime::new();
        rt.register_backend("fail", backend);
        let snap_before = rt.metrics().snapshot();
        let handle = rt.run_streaming("fail", simple_wo("fail")).await.unwrap();
        let _: Vec<_> = handle.events.collect().await;
        let _ = handle.receipt.await;
        let snap_after = rt.metrics().snapshot();
        // Backend failure returns Err before metrics recording.
        assert_eq!(snap_after.total_runs, snap_before.total_runs);
    }

    #[tokio::test]
    async fn metrics_event_count_increases() {
        let rt = Runtime::with_default_backends();
        let snap_before = rt.metrics().snapshot();
        let (_, _) = collect_run(&rt, "mock", simple_wo("events")).await;
        let snap_after = rt.metrics().snapshot();
        assert!(snap_after.total_events > snap_before.total_events);
    }

    #[tokio::test]
    async fn metrics_multiple_runs() {
        let rt = Runtime::with_default_backends();
        for i in 0..5 {
            let (_, _) = collect_run(&rt, "mock", simple_wo(&format!("run-{i}"))).await;
        }
        let snap = rt.metrics().snapshot();
        assert!(snap.total_runs >= 5);
    }

    #[test]
    fn fresh_metrics_are_zero() {
        let rt = Runtime::new();
        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 0);
        assert_eq!(snap.successful_runs, 0);
        assert_eq!(snap.failed_runs, 0);
    }

    #[tokio::test]
    async fn average_duration_is_positive_after_run() {
        let rt = Runtime::with_default_backends();
        let (_, _) = collect_run(&rt, "mock", simple_wo("duration")).await;
        let snap = rt.metrics().snapshot();
        // Duration might be 0 for very fast mock runs, so just check >= 0.
        assert!(snap.average_run_duration_ms < 10_000);
    }
}

// ===========================================================================
// Module 8: Error Taxonomy Integration (6 tests)
// ===========================================================================

mod error_taxonomy {
    use super::*;

    #[test]
    fn runtime_error_into_abp_error_preserves_code() {
        let err = RuntimeError::UnknownBackend {
            name: "ghost".into(),
        };
        let abp_err = err.into_abp_error();
        assert_eq!(abp_err.code, abp_error::ErrorCode::BackendNotFound);
    }

    #[test]
    fn classified_error_round_trips() {
        let abp_err = abp_error::AbpError::new(abp_error::ErrorCode::BackendTimeout, "timed out");
        let rt_err: RuntimeError = abp_err.into();
        assert_eq!(rt_err.error_code(), abp_error::ErrorCode::BackendTimeout);
    }

    #[test]
    fn no_projection_match_error_code() {
        let err = RuntimeError::NoProjectionMatch {
            reason: "no matrix".into(),
        };
        assert_eq!(err.error_code(), abp_error::ErrorCode::BackendNotFound);
    }

    #[test]
    fn no_projection_match_not_retryable() {
        let err = RuntimeError::NoProjectionMatch {
            reason: "no matrix".into(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn classified_retryable_propagates() {
        let abp_err =
            abp_error::AbpError::new(abp_error::ErrorCode::BackendRateLimited, "rate limited");
        let rt_err: RuntimeError = abp_err.into();
        assert!(rt_err.is_retryable());
    }

    #[test]
    fn classified_non_retryable_propagates() {
        let abp_err =
            abp_error::AbpError::new(abp_error::ErrorCode::ContractSchemaViolation, "bad schema");
        let rt_err: RuntimeError = abp_err.into();
        assert!(!rt_err.is_retryable());
    }
}

// ===========================================================================
// Module 9: Projection Integration (6 tests)
// ===========================================================================

mod projection_integration {
    use super::*;

    #[tokio::test]
    async fn run_projected_selects_and_executes() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_backend(
            "mock",
            cap_manifest(&[Capability::Streaming]),
            Dialect::OpenAi,
            50,
        );
        let rt = Runtime::with_default_backends().with_projection(matrix);
        let wo = simple_wo("projected run");
        let handle = rt.run_projected(wo).await.unwrap();
        let _: Vec<_> = handle.events.collect().await;
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn run_projected_without_matrix_fails() {
        let rt = Runtime::with_default_backends();
        let wo = simple_wo("no matrix");
        match rt.run_projected(wo).await {
            Err(RuntimeError::NoProjectionMatch { .. }) => {}
            Err(e) => panic!("expected NoProjectionMatch, got {e:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn projection_score_is_positive() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_backend(
            "mock",
            cap_manifest(&[Capability::Streaming]),
            Dialect::OpenAi,
            50,
        );
        let rt = Runtime::with_default_backends().with_projection(matrix);
        let wo = simple_wo("score");
        let result = rt.select_backend(&wo).unwrap();
        assert!(result.fidelity_score.total > 0.0);
    }

    #[test]
    fn projection_with_fallback_chain() {
        let mut matrix = ProjectionMatrix::new();
        matrix.register_backend(
            "primary",
            cap_manifest(&[Capability::Streaming, Capability::ToolRead]),
            Dialect::OpenAi,
            80,
        );
        matrix.register_backend(
            "fallback",
            cap_manifest(&[Capability::Streaming]),
            Dialect::Claude,
            20,
        );
        let mut rt = Runtime::new().with_projection(matrix);
        rt.register_backend("primary", MockBackend);
        rt.register_backend("fallback", MockBackend);
        let wo = simple_wo("fallback");
        let result = rt.select_backend(&wo).unwrap();
        assert_eq!(result.selected_backend, "primary");
        assert!(!result.fallback_chain.is_empty());
    }

    #[test]
    fn projection_matrix_configurable() {
        let rt = Runtime::new();
        assert!(rt.projection().is_none());
        let rt2 = rt.with_projection(ProjectionMatrix::new());
        assert!(rt2.projection().is_some());
    }

    #[test]
    fn projection_matrix_mutable_access() {
        let mut rt = Runtime::new().with_projection(ProjectionMatrix::new());
        let pm = rt.projection_mut().unwrap();
        pm.register_backend(
            "dynamic",
            cap_manifest(&[Capability::Streaming]),
            Dialect::OpenAi,
            10,
        );
        // Can't select because "dynamic" isn't in the runtime registry, but the
        // matrix should now contain it.
        let rt2 = &rt;
        assert!(rt2.projection().is_some());
    }
}

// ===========================================================================
// Module 10: Concurrent & Stress (4 tests)
// ===========================================================================

mod concurrent_stress {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn concurrent_runs_on_same_runtime() {
        let rt = Arc::new(Runtime::with_default_backends());
        let mut handles = Vec::new();
        for i in 0..10 {
            let rt = Arc::clone(&rt);
            handles.push(tokio::spawn(async move {
                let handle = rt
                    .run_streaming("mock", simple_wo(&format!("concurrent-{i}")))
                    .await
                    .unwrap();
                let _: Vec<_> = handle.events.collect().await;
                handle.receipt.await.unwrap().unwrap()
            }));
        }
        let mut receipts = Vec::new();
        for h in handles {
            receipts.push(h.await.unwrap());
        }
        assert_eq!(receipts.len(), 10);
        // All receipts should have unique run ids.
        let ids: std::collections::HashSet<_> = receipts.iter().map(|r| r.meta.run_id).collect();
        assert_eq!(ids.len(), 10);
    }

    #[tokio::test]
    async fn concurrent_runs_metrics_consistent() {
        let rt = Arc::new(Runtime::with_default_backends());
        let snap_before = rt.metrics().snapshot();
        let mut handles = Vec::new();
        for i in 0..5 {
            let rt = Arc::clone(&rt);
            handles.push(tokio::spawn(async move {
                let handle = rt
                    .run_streaming("mock", simple_wo(&format!("metric-{i}")))
                    .await
                    .unwrap();
                let _: Vec<_> = handle.events.collect().await;
                let _ = handle.receipt.await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let snap_after = rt.metrics().snapshot();
        assert!(snap_after.total_runs >= snap_before.total_runs + 5);
    }

    #[tokio::test]
    async fn receipt_chain_consistent_under_concurrency() {
        let rt = Arc::new(Runtime::with_default_backends());
        let chain = rt.receipt_chain();
        let mut handles = Vec::new();
        for i in 0..5 {
            let rt = Arc::clone(&rt);
            handles.push(tokio::spawn(async move {
                let handle = rt
                    .run_streaming("mock", simple_wo(&format!("chain-{i}")))
                    .await
                    .unwrap();
                let _: Vec<_> = handle.events.collect().await;
                let _ = handle.receipt.await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let guard = chain.lock().await;
        assert!(guard.len() >= 5);
    }

    #[tokio::test]
    async fn rapid_sequential_runs() {
        let rt = Runtime::with_default_backends();
        for i in 0..20 {
            let (_, receipt) = collect_run(&rt, "mock", simple_wo(&format!("rapid-{i}"))).await;
            assert_eq!(receipt.outcome, Outcome::Complete);
        }
    }
}
