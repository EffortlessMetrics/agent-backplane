#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive runtime orchestration tests covering the full execution pipeline.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, MinSupport, Outcome, PolicyProfile, Receipt,
    RuntimeConfig, SupportLevel, VerificationReport, WorkOrder, WorkOrderBuilder, WorkspaceMode,
    WorkspaceSpec,
};
use abp_error::{AbpError, ErrorCode};
use abp_integrations::{Backend, MockBackend};
use abp_receipt::{ReceiptBuilder, ReceiptChain};
use abp_runtime::multiplex::{EventMultiplexer, EventRouter, MultiplexError};
use abp_runtime::telemetry::RunMetrics;
use abp_runtime::{BackendRegistry, Runtime, RuntimeError};
use abp_stream::{
    EventFilter, EventRecorder, EventStats, EventTransform, StreamPipeline, StreamPipelineBuilder,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// tempfile used for staged workspace tests

// ---------------------------------------------------------------------------
// Custom backend helpers for error injection
// ---------------------------------------------------------------------------

/// Backend that always fails with an error.
#[derive(Debug, Clone)]
struct FailingBackend {
    message: String,
}

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".to_string(),
            backend_version: Some("0.1".to_string()),
            adapter_version: Some("0.1".to_string()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        anyhow::bail!("{}", self.message)
    }
}

/// Backend that emits N events before returning a receipt.
#[derive(Debug, Clone)]
struct EventCountBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for EventCountBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "event-count".to_string(),
            backend_version: Some("0.1".to_string()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("chunk-{i}"),
                },
                ext: None,
            };
            let _ = events_tx.send(ev).await;
        }
        Ok(ReceiptBuilder::new("event-count")
            .run_id(run_id)
            .work_order_id(work_order.id)
            .outcome(Outcome::Complete)
            .build())
    }
}

/// Backend that panics.
#[derive(Debug, Clone)]
struct PanickingBackend;

#[async_trait]
impl Backend for PanickingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "panicking".to_string(),
            backend_version: None,
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        _run_id: Uuid,
        _work_order: WorkOrder,
        _events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        panic!("backend panicked intentionally");
    }
}

/// Backend that sleeps for a given duration before finishing.
#[derive(Debug, Clone)]
struct SlowBackend {
    delay_ms: u64,
}

#[async_trait]
impl Backend for SlowBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "slow".to_string(),
            backend_version: Some("0.1".to_string()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "slow done".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;
        Ok(ReceiptBuilder::new("slow")
            .run_id(run_id)
            .work_order_id(work_order.id)
            .outcome(Outcome::Complete)
            .build())
    }
}

/// Backend that tracks how many times it has been invoked.
#[derive(Debug, Clone)]
struct CountingBackend {
    counter: Arc<AtomicU32>,
}

#[async_trait]
impl Backend for CountingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "counting".to_string(),
            backend_version: None,
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "counted".into(),
            },
            ext: None,
        };
        let _ = events_tx.send(ev).await;
        Ok(ReceiptBuilder::new("counting")
            .run_id(run_id)
            .work_order_id(work_order.id)
            .outcome(Outcome::Complete)
            .build())
    }
}

/// Backend that emits specific event kinds for pipeline testing.
#[derive(Debug, Clone)]
struct MixedEventBackend;

#[async_trait]
impl Backend for MixedEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "mixed".to_string(),
            backend_version: Some("0.1".to_string()),
            adapter_version: None,
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let events = vec![
            AgentEventKind::RunStarted {
                message: "start".into(),
            },
            AgentEventKind::AssistantDelta {
                text: "hello".into(),
            },
            AgentEventKind::ToolCall {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "file.txt"}),
            },
            AgentEventKind::ToolResult {
                tool_name: "read".into(),
                tool_use_id: Some("t1".into()),
                output: "content".into(),
                is_error: false,
            },
            AgentEventKind::Error {
                message: "minor warning".into(),
                error_code: None,
            },
            AgentEventKind::RunCompleted {
                message: "done".into(),
            },
        ];
        for kind in events {
            let ev = AgentEvent {
                ts: Utc::now(),
                kind,
                ext: None,
            };
            let _ = events_tx.send(ev).await;
        }
        Ok(ReceiptBuilder::new("mixed")
            .run_id(run_id)
            .work_order_id(work_order.id)
            .outcome(Outcome::Complete)
            .build())
    }
}

fn simple_work_order(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

fn staged_work_order(task: &str, root: &str) -> WorkOrder {
    WorkOrderBuilder::new(task).root(root).build()
}

// ============================================================================
// Module: run_streaming with MockBackend
// ============================================================================
mod run_streaming_mock {
    use super::*;

    #[tokio::test]
    async fn basic_run_returns_handle() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        assert!(!handle.run_id.is_nil());
    }

    #[tokio::test]
    async fn receipt_is_complete() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("test receipt");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn receipt_backend_id_is_mock() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("backend check");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.backend.id, "mock");
    }

    #[tokio::test]
    async fn receipt_has_hash() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("hash check");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }

    #[tokio::test]
    async fn receipt_hash_verifies() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("hash verify");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert!(abp_receipt::verify_hash(&receipt));
    }

    #[tokio::test]
    async fn receipt_trace_is_nonempty() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("trace check");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert!(!receipt.trace.is_empty());
    }

    #[tokio::test]
    async fn receipt_work_order_id_matches() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("id check");
        let wo_id = wo.id;
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.meta.work_order_id, wo_id);
    }

    #[tokio::test]
    async fn receipt_contract_version_present() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("version check");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert!(!receipt.meta.contract_version.is_empty());
    }

    #[tokio::test]
    async fn receipt_duration_is_positive() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("duration check");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        // Duration can be 0 on fast machines but should not be negative.
        assert!(receipt.meta.duration_ms < 60_000);
    }

    #[tokio::test]
    async fn events_stream_delivers_events() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("events test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let events: Vec<_> = handle.events.collect().await;
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn run_id_is_unique_across_runs() {
        let rt = Runtime::with_default_backends();
        let h1 = rt
            .run_streaming("mock", simple_work_order("a"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", simple_work_order("b"))
            .await
            .unwrap();
        assert_ne!(h1.run_id, h2.run_id);
        let _ = h1.receipt.await;
        let _ = h2.receipt.await;
    }

    #[tokio::test]
    async fn mock_backend_emits_run_started_event() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("run started");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let events: Vec<_> = handle.events.collect().await;
        let has_start = events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunStarted { .. }));
        assert!(has_start, "expected RunStarted event");
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn mock_backend_emits_run_completed_event() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("run completed");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let events: Vec<_> = handle.events.collect().await;
        let has_complete = events
            .iter()
            .any(|e| matches!(&e.kind, AgentEventKind::RunCompleted { .. }));
        assert!(has_complete, "expected RunCompleted event");
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn mock_receipt_has_capabilities() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("caps");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert!(receipt.capabilities.contains_key(&Capability::Streaming));
    }

    #[tokio::test]
    async fn receipt_timestamps_are_ordered() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("timestamps");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    }
}

// ============================================================================
// Module: RunHandle tests
// ============================================================================
mod run_handle {
    use super::*;

    #[tokio::test]
    async fn receipt_future_resolves() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("resolve"))
            .await
            .unwrap();
        let result = handle.receipt.await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn events_can_be_collected_fully() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("collect"))
            .await
            .unwrap();
        let events: Vec<_> = handle.events.collect().await;
        assert!(events.len() >= 2, "expected at least 2 events");
    }

    #[tokio::test]
    async fn run_id_matches_receipt_run_id() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("id match"))
            .await
            .unwrap();
        let run_id = handle.run_id;
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.meta.run_id, run_id);
    }

    #[tokio::test]
    async fn events_are_ordered_chronologically() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("order"))
            .await
            .unwrap();
        let events: Vec<_> = handle.events.collect().await;
        for window in events.windows(2) {
            assert!(window[0].ts <= window[1].ts);
        }
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn receipt_future_with_failing_backend_returns_error() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "boom".into(),
            },
        );
        let handle = rt
            .run_streaming("fail", simple_work_order("fail"))
            .await
            .unwrap();
        let result = handle.receipt.await.unwrap();
        assert!(result.is_err());
    }
}

// ============================================================================
// Module: RuntimeError variants
// ============================================================================
mod runtime_error_variants {
    use super::*;

    #[test]
    fn unknown_backend_display() {
        let err = RuntimeError::UnknownBackend {
            name: "nope".into(),
        };
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn unknown_backend_error_code() {
        let err = RuntimeError::UnknownBackend { name: "foo".into() };
        assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
        assert_eq!(err.error_code().as_str(), "backend_not_found");
    }

    #[test]
    fn workspace_failed_display() {
        let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
        assert!(err.to_string().contains("workspace preparation failed"));
    }

    #[test]
    fn workspace_failed_error_code() {
        let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("err"));
        assert_eq!(err.error_code(), ErrorCode::WorkspaceInitFailed);
        assert_eq!(err.error_code().as_str(), "workspace_init_failed");
    }

    #[test]
    fn policy_failed_display() {
        let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
        assert!(err.to_string().contains("policy compilation failed"));
    }

    #[test]
    fn policy_failed_error_code() {
        let err = RuntimeError::PolicyFailed(anyhow::anyhow!("err"));
        assert_eq!(err.error_code(), ErrorCode::PolicyInvalid);
        assert_eq!(err.error_code().as_str(), "policy_invalid");
    }

    #[test]
    fn backend_failed_display() {
        let err = RuntimeError::BackendFailed(anyhow::anyhow!("crash"));
        assert!(err.to_string().contains("backend execution failed"));
    }

    #[test]
    fn backend_failed_error_code() {
        let err = RuntimeError::BackendFailed(anyhow::anyhow!("err"));
        assert_eq!(err.error_code(), ErrorCode::BackendCrashed);
        assert_eq!(err.error_code().as_str(), "backend_crashed");
    }

    #[test]
    fn capability_check_failed_display() {
        let err = RuntimeError::CapabilityCheckFailed("missing streaming".into());
        assert!(err.to_string().contains("missing streaming"));
    }

    #[test]
    fn capability_check_failed_error_code() {
        let err = RuntimeError::CapabilityCheckFailed("x".into());
        assert_eq!(err.error_code(), ErrorCode::CapabilityUnsupported);
        assert_eq!(err.error_code().as_str(), "capability_unsupported");
    }

    #[test]
    fn classified_error_preserves_code() {
        let abp_err = AbpError::new(ErrorCode::BackendTimeout, "timed out");
        let rt_err: RuntimeError = abp_err.into();
        assert_eq!(rt_err.error_code(), ErrorCode::BackendTimeout);
    }

    #[test]
    fn no_projection_match_error_code() {
        let err = RuntimeError::NoProjectionMatch {
            reason: "empty".into(),
        };
        assert_eq!(err.error_code(), ErrorCode::BackendNotFound);
        assert_eq!(err.error_code().as_str(), "backend_not_found");
    }

    #[test]
    fn into_abp_error_preserves_message() {
        let err = RuntimeError::UnknownBackend {
            name: "gone".into(),
        };
        let abp_err = err.into_abp_error();
        assert!(abp_err.message.contains("gone"));
    }

    #[test]
    fn classified_roundtrip() {
        let original = AbpError::new(ErrorCode::ConfigInvalid, "bad").with_context("key", "value");
        let rt_err: RuntimeError = original.into();
        let back = rt_err.into_abp_error();
        assert_eq!(back.code, ErrorCode::ConfigInvalid);
        assert!(back.context.contains_key("key"));
    }

    #[test]
    fn is_retryable_backend_failed() {
        let err = RuntimeError::BackendFailed(anyhow::anyhow!("transient"));
        assert!(err.is_retryable());
    }

    #[test]
    fn is_retryable_workspace_failed() {
        let err = RuntimeError::WorkspaceFailed(anyhow::anyhow!("transient"));
        assert!(err.is_retryable());
    }

    #[test]
    fn is_not_retryable_unknown_backend() {
        let err = RuntimeError::UnknownBackend { name: "x".into() };
        assert!(!err.is_retryable());
    }

    #[test]
    fn is_not_retryable_policy_failed() {
        let err = RuntimeError::PolicyFailed(anyhow::anyhow!("bad"));
        assert!(!err.is_retryable());
    }

    #[test]
    fn is_not_retryable_capability_check() {
        let err = RuntimeError::CapabilityCheckFailed("missing".into());
        assert!(!err.is_retryable());
    }

    #[test]
    fn is_not_retryable_no_projection() {
        let err = RuntimeError::NoProjectionMatch {
            reason: "none".into(),
        };
        assert!(!err.is_retryable());
    }

    #[test]
    fn classified_retryable_follows_taxonomy() {
        let retryable = AbpError::new(ErrorCode::BackendTimeout, "timeout");
        let rt_err: RuntimeError = retryable.into();
        assert!(rt_err.is_retryable());

        let permanent = AbpError::new(ErrorCode::PolicyDenied, "denied");
        let rt_err: RuntimeError = permanent.into();
        assert!(!rt_err.is_retryable());
    }

    #[test]
    fn error_code_as_str_is_snake_case() {
        let codes = [
            (ErrorCode::BackendNotFound, "backend_not_found"),
            (ErrorCode::WorkspaceInitFailed, "workspace_init_failed"),
            (ErrorCode::PolicyInvalid, "policy_invalid"),
            (ErrorCode::BackendCrashed, "backend_crashed"),
            (ErrorCode::CapabilityUnsupported, "capability_unsupported"),
        ];
        for (code, expected) in codes {
            assert_eq!(code.as_str(), expected);
        }
    }
}

// ============================================================================
// Module: Error propagation
// ============================================================================
mod error_propagation {
    use super::*;

    #[tokio::test]
    async fn unknown_backend_returns_error_immediately() {
        let rt = Runtime::new();
        let result = rt
            .run_streaming("nonexistent", simple_work_order("fail"))
            .await;
        assert!(matches!(result, Err(RuntimeError::UnknownBackend { .. })));
    }

    #[tokio::test]
    async fn unknown_backend_error_message_includes_name() {
        let rt = Runtime::new();
        let result = rt
            .run_streaming("my_backend", simple_work_order("fail"))
            .await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert!(err.to_string().contains("my_backend"));
    }

    #[tokio::test]
    async fn failing_backend_propagates_through_receipt() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "explosion".into(),
            },
        );
        let handle = rt
            .run_streaming("fail", simple_work_order("fail"))
            .await
            .unwrap();
        let result = handle.receipt.await.unwrap();
        match result {
            Err(RuntimeError::BackendFailed(e)) => {
                let chain = format!("{e:#}");
                assert!(
                    chain.contains("explosion"),
                    "expected 'explosion' in error chain, got: {chain}"
                );
            }
            other => panic!("expected BackendFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn panicking_backend_returns_backend_failed() {
        let mut rt = Runtime::new();
        rt.register_backend("panic", PanickingBackend);
        let handle = rt
            .run_streaming("panic", simple_work_order("panic"))
            .await
            .unwrap();
        let result = handle.receipt.await.unwrap();
        assert!(
            matches!(result, Err(RuntimeError::BackendFailed(_))),
            "expected BackendFailed for panic, got {result:?}"
        );
    }

    #[tokio::test]
    async fn multiple_failing_runs_all_return_errors() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "nope".into(),
            },
        );
        for _ in 0..5 {
            let handle = rt
                .run_streaming("fail", simple_work_order("repeat"))
                .await
                .unwrap();
            let result = handle.receipt.await.unwrap();
            assert!(result.is_err());
        }
    }
}

// ============================================================================
// Module: Backend selection logic
// ============================================================================
mod backend_selection {
    use super::*;

    #[test]
    fn runtime_new_has_no_backends() {
        let rt = Runtime::new();
        assert!(rt.backend_names().is_empty());
    }

    #[test]
    fn with_default_backends_has_mock() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend_names().contains(&"mock".to_string()));
    }

    #[test]
    fn register_backend_adds_name() {
        let mut rt = Runtime::new();
        rt.register_backend("test", MockBackend);
        assert!(rt.backend_names().contains(&"test".to_string()));
    }

    #[test]
    fn register_multiple_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("a", MockBackend);
        rt.register_backend("b", MockBackend);
        rt.register_backend("c", MockBackend);
        let names = rt.backend_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
        assert!(names.contains(&"c".to_string()));
    }

    #[test]
    fn backend_names_are_sorted() {
        let mut rt = Runtime::new();
        rt.register_backend("z", MockBackend);
        rt.register_backend("a", MockBackend);
        rt.register_backend("m", MockBackend);
        assert_eq!(rt.backend_names(), vec!["a", "m", "z"]);
    }

    #[test]
    fn replacing_backend_keeps_same_count() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend("mock", MockBackend);
        assert_eq!(rt.backend_names().len(), 1);
    }

    #[test]
    fn backend_lookup_returns_some_for_registered() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend("mock").is_some());
    }

    #[test]
    fn backend_lookup_returns_none_for_unregistered() {
        let rt = Runtime::with_default_backends();
        assert!(rt.backend("nonexistent").is_none());
    }

    #[tokio::test]
    async fn run_streaming_with_custom_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("custom", EventCountBackend { event_count: 3 });
        let handle = rt
            .run_streaming("custom", simple_work_order("custom run"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.backend.id, "event-count");
    }

    #[test]
    fn registry_access() {
        let rt = Runtime::with_default_backends();
        let registry = rt.registry();
        assert!(registry.contains("mock"));
    }

    #[test]
    fn registry_mut_allows_modification() {
        let mut rt = Runtime::with_default_backends();
        rt.registry_mut().register("extra", MockBackend);
        assert!(rt.backend("extra").is_some());
    }
}

// ============================================================================
// Module: Workspace preparation
// ============================================================================
mod workspace_preparation {
    use super::*;

    #[tokio::test]
    async fn passthrough_workspace_works() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("passthrough ws")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn receipt_has_verification_report() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("verification");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        // The verification report should exist even if diffs are None.
        let _report = &receipt.verification;
    }

    #[tokio::test]
    async fn workspace_root_is_rewritten() {
        let rt = Runtime::with_default_backends();
        let wo = simple_work_order("root rewrite");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        // The receipt should complete successfully (workspace prep succeeded).
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn staged_workspace_completes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("test.txt"), "hello").unwrap();
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("staged")
            .root(tmp.path().to_string_lossy().to_string())
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn workspace_preparation_failure_is_workspace_failed() {
        // Create a work order with an invalid workspace root.
        let mut wo = simple_work_order("bad workspace");
        wo.workspace.root = "/nonexistent/path/that/should/not/exist".to_string();
        wo.workspace.mode = WorkspaceMode::Staged;

        let rt = Runtime::with_default_backends();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let result = handle.receipt.await.unwrap();
        // Staged mode with nonexistent root should fail in workspace prep.
        assert!(
            result.is_err(),
            "expected workspace failure for nonexistent path"
        );
    }
}

// ============================================================================
// Module: Event multiplexing
// ============================================================================
mod event_multiplexing {
    use super::*;

    #[test]
    fn multiplexer_no_subscribers() {
        let mux = EventMultiplexer::new(16);
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: None,
        };
        let result = mux.broadcast(ev);
        assert!(matches!(result, Err(MultiplexError::NoSubscribers)));
    }

    #[test]
    fn multiplexer_with_subscriber_delivers() {
        let mux = EventMultiplexer::new(16);
        let mut sub = mux.subscribe();
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "msg".into() },
            ext: None,
        };
        let count = mux.broadcast(ev).unwrap();
        assert_eq!(count, 1);
        let received = sub.try_recv();
        assert!(received.is_some());
    }

    #[test]
    fn multiplexer_multiple_subscribers() {
        let mux = EventMultiplexer::new(16);
        let mut sub1 = mux.subscribe();
        let mut sub2 = mux.subscribe();
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        };
        let count = mux.broadcast(ev).unwrap();
        assert_eq!(count, 2);
        assert!(sub1.try_recv().is_some());
        assert!(sub2.try_recv().is_some());
    }

    #[test]
    fn subscriber_count_tracks() {
        let mux = EventMultiplexer::new(16);
        assert_eq!(mux.subscriber_count(), 0);
        let _s1 = mux.subscribe();
        assert_eq!(mux.subscriber_count(), 1);
        let _s2 = mux.subscribe();
        assert_eq!(mux.subscriber_count(), 2);
    }

    #[tokio::test]
    async fn subscriber_async_recv() {
        let mux = EventMultiplexer::new(16);
        let mut sub = mux.subscribe();
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "end".into(),
            },
            ext: None,
        };
        mux.broadcast(ev).unwrap();
        let received = sub.recv().await.unwrap();
        assert!(matches!(received.kind, AgentEventKind::RunCompleted { .. }));
    }

    #[test]
    fn event_router_dispatches_by_kind() {
        let mut router = EventRouter::new();
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        router.add_route(
            "run_started",
            Box::new(move |_ev| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        };
        router.route(&ev);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn event_router_ignores_unmatched_kinds() {
        let mut router = EventRouter::new();
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        router.add_route(
            "error",
            Box::new(move |_ev| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let ev = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        };
        router.route(&ev);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn event_router_empty_has_zero_routes() {
        let router = EventRouter::new();
        assert_eq!(router.route_count(), 0);
    }

    #[test]
    fn event_router_counts_routes() {
        let mut router = EventRouter::new();
        router.add_route("run_started", Box::new(|_| {}));
        router.add_route("error", Box::new(|_| {}));
        assert_eq!(router.route_count(), 2);
    }

    #[tokio::test]
    async fn multiplexer_closed_after_drop() {
        let mux = EventMultiplexer::new(16);
        let mut sub = mux.subscribe();
        drop(mux);
        let result = sub.recv().await;
        assert!(matches!(result, Err(MultiplexError::Closed)));
    }
}

// ============================================================================
// Module: Receipt generation and hashing
// ============================================================================
mod receipt_generation {
    use super::*;

    #[tokio::test]
    async fn receipt_has_sha256() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("hash"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert!(!hash.is_empty());
    }

    #[tokio::test]
    async fn receipt_hash_is_hex_string() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("hex"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn two_runs_produce_different_hashes() {
        let rt = Runtime::with_default_backends();
        let h1 = rt
            .run_streaming("mock", simple_work_order("a"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", simple_work_order("b"))
            .await
            .unwrap();
        let r1 = h1.receipt.await.unwrap().unwrap();
        let r2 = h2.receipt.await.unwrap().unwrap();
        // Different run IDs should yield different hashes.
        assert_ne!(r1.receipt_sha256, r2.receipt_sha256);
    }

    #[tokio::test]
    async fn receipt_chain_accumulates() {
        let rt = Runtime::with_default_backends();
        let chain = rt.receipt_chain();

        let h1 = rt
            .run_streaming("mock", simple_work_order("chain-1"))
            .await
            .unwrap();
        let _ = h1.receipt.await.unwrap().unwrap();

        let h2 = rt
            .run_streaming("mock", simple_work_order("chain-2"))
            .await
            .unwrap();
        let _ = h2.receipt.await.unwrap().unwrap();

        let locked = chain.lock().await;
        assert!(locked.len() >= 2);
    }

    #[tokio::test]
    async fn receipt_verification_fields() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("verify"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        // Harness OK should be true for mock.
        assert!(receipt.verification.harness_ok);
    }

    #[tokio::test]
    async fn receipt_usage_raw_has_data() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("usage"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert!(!receipt.usage_raw.is_null());
    }

    #[tokio::test]
    async fn receipt_serializes_to_json() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("json"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(json.contains("mock"));
    }

    #[tokio::test]
    async fn receipt_deserializes_roundtrip() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("roundtrip"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        let json = serde_json::to_string(&receipt).unwrap();
        let deserialized: Receipt = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.backend.id, receipt.backend.id);
        assert_eq!(deserialized.receipt_sha256, receipt.receipt_sha256);
    }
}

// ============================================================================
// Module: Telemetry and metrics
// ============================================================================
mod telemetry_metrics {
    use super::*;

    #[test]
    fn metrics_start_at_zero() {
        let m = RunMetrics::new();
        let s = m.snapshot();
        assert_eq!(s.total_runs, 0);
        assert_eq!(s.successful_runs, 0);
        assert_eq!(s.failed_runs, 0);
        assert_eq!(s.total_events, 0);
    }

    #[test]
    fn metrics_record_successful_run() {
        let m = RunMetrics::new();
        m.record_run(100, true, 5);
        let s = m.snapshot();
        assert_eq!(s.total_runs, 1);
        assert_eq!(s.successful_runs, 1);
        assert_eq!(s.failed_runs, 0);
        assert_eq!(s.total_events, 5);
    }

    #[test]
    fn metrics_record_failed_run() {
        let m = RunMetrics::new();
        m.record_run(50, false, 2);
        let s = m.snapshot();
        assert_eq!(s.total_runs, 1);
        assert_eq!(s.failed_runs, 1);
        assert_eq!(s.successful_runs, 0);
    }

    #[test]
    fn metrics_accumulate_events() {
        let m = RunMetrics::new();
        m.record_run(10, true, 3);
        m.record_run(20, true, 7);
        let s = m.snapshot();
        assert_eq!(s.total_events, 10);
    }

    #[test]
    fn metrics_average_duration() {
        let m = RunMetrics::new();
        m.record_run(100, true, 1);
        m.record_run(200, true, 1);
        let s = m.snapshot();
        assert_eq!(s.average_run_duration_ms, 150);
    }

    #[tokio::test]
    async fn runtime_metrics_after_run() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("metrics"))
            .await
            .unwrap();
        let _ = handle.receipt.await.unwrap().unwrap();
        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 1);
        assert_eq!(snap.successful_runs, 1);
    }

    #[tokio::test]
    async fn runtime_metrics_after_multiple_runs() {
        let rt = Runtime::with_default_backends();
        for i in 0..3 {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("run-{i}")))
                .await
                .unwrap();
            let _ = handle.receipt.await.unwrap().unwrap();
        }
        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 3);
        assert_eq!(snap.successful_runs, 3);
    }

    #[tokio::test]
    async fn runtime_metrics_count_events() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("events"))
            .await
            .unwrap();
        let _ = handle.receipt.await.unwrap().unwrap();
        let snap = rt.metrics().snapshot();
        assert!(snap.total_events > 0);
    }
}

// ============================================================================
// Module: Stream pipeline integration
// ============================================================================
mod stream_pipeline {
    use super::*;

    #[tokio::test]
    async fn empty_pipeline_passes_all_events() {
        let pipeline = StreamPipeline::new();
        let rt = Runtime::with_default_backends().with_stream_pipeline(pipeline);
        let handle = rt
            .run_streaming("mock", simple_work_order("pipeline"))
            .await
            .unwrap();
        let events: Vec<_> = handle.events.collect().await;
        assert!(!events.is_empty());
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn filter_pipeline_excludes_errors() {
        let pipeline = StreamPipelineBuilder::new()
            .filter(EventFilter::exclude_errors())
            .build();
        let mut rt = Runtime::new().with_stream_pipeline(pipeline);
        rt.register_backend("mixed", MixedEventBackend);

        let handle = rt
            .run_streaming("mixed", simple_work_order("filter"))
            .await
            .unwrap();
        let events: Vec<_> = handle.events.collect().await;
        let has_error = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::Error { .. }));
        assert!(!has_error, "error events should be filtered out");
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn pipeline_with_recorder_on_runtime() {
        let rt = Runtime::with_default_backends().with_stream_pipeline(StreamPipeline::new());
        assert!(rt.stream_pipeline().is_some());
    }

    #[tokio::test]
    async fn runtime_without_pipeline_returns_none() {
        let rt = Runtime::with_default_backends();
        assert!(rt.stream_pipeline().is_none());
    }
}

// ============================================================================
// Module: Concurrent runtime executions
// ============================================================================
mod concurrent_execution {
    use super::*;

    #[tokio::test]
    async fn two_concurrent_runs() {
        let rt = Runtime::with_default_backends();
        let h1 = rt
            .run_streaming("mock", simple_work_order("a"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", simple_work_order("b"))
            .await
            .unwrap();

        let (r1, r2) = tokio::join!(h1.receipt, h2.receipt);
        assert!(r1.unwrap().is_ok());
        assert!(r2.unwrap().is_ok());
    }

    #[tokio::test]
    async fn five_concurrent_runs() {
        let rt = Runtime::with_default_backends();
        let mut handles = Vec::new();
        for i in 0..5 {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("concurrent-{i}")))
                .await
                .unwrap();
            handles.push(handle.receipt);
        }
        for h in handles {
            let result = h.await.unwrap();
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn concurrent_runs_all_get_unique_ids() {
        let rt = Runtime::with_default_backends();
        let mut run_ids = Vec::new();
        let mut receipt_handles = Vec::new();
        for i in 0..5 {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("id-{i}")))
                .await
                .unwrap();
            run_ids.push(handle.run_id);
            receipt_handles.push(handle.receipt);
        }
        // All run IDs should be unique.
        let unique: std::collections::HashSet<_> = run_ids.iter().collect();
        assert_eq!(unique.len(), 5);
        for h in receipt_handles {
            let _ = h.await;
        }
    }

    #[tokio::test]
    async fn concurrent_different_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend("count3", EventCountBackend { event_count: 3 });
        rt.register_backend("slow", SlowBackend { delay_ms: 10 });

        let h1 = rt
            .run_streaming("mock", simple_work_order("a"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("count3", simple_work_order("b"))
            .await
            .unwrap();
        let h3 = rt
            .run_streaming("slow", simple_work_order("c"))
            .await
            .unwrap();

        let (r1, r2, r3) = tokio::join!(h1.receipt, h2.receipt, h3.receipt);
        assert!(r1.unwrap().is_ok());
        assert!(r2.unwrap().is_ok());
        assert!(r3.unwrap().is_ok());
    }

    #[tokio::test]
    async fn concurrent_runs_with_counting_backend() {
        let counter = Arc::new(AtomicU32::new(0));
        let mut rt = Runtime::new();
        rt.register_backend(
            "counter",
            CountingBackend {
                counter: counter.clone(),
            },
        );

        let mut handles = Vec::new();
        for i in 0..10 {
            let handle = rt
                .run_streaming("counter", simple_work_order(&format!("c-{i}")))
                .await
                .unwrap();
            handles.push(handle.receipt);
        }
        for h in handles {
            let _ = h.await;
        }
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn metrics_correct_after_concurrent_runs() {
        let rt = Runtime::with_default_backends();
        let mut handles = Vec::new();
        for i in 0..5 {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("m-{i}")))
                .await
                .unwrap();
            handles.push(handle.receipt);
        }
        for h in handles {
            let _ = h.await.unwrap().unwrap();
        }
        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 5);
        assert_eq!(snap.successful_runs, 5);
    }
}

// ============================================================================
// Module: Backend registry
// ============================================================================
mod backend_registry_tests {
    use super::*;

    #[test]
    fn default_registry_is_empty() {
        let reg = BackendRegistry::default();
        assert!(reg.list().is_empty());
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        assert!(reg.get("mock").is_some());
    }

    #[test]
    fn registry_contains() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        assert!(reg.contains("mock"));
        assert!(!reg.contains("other"));
    }

    #[test]
    fn registry_remove() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let removed = reg.remove("mock");
        assert!(removed.is_some());
        assert!(!reg.contains("mock"));
    }

    #[test]
    fn registry_remove_nonexistent() {
        let mut reg = BackendRegistry::default();
        let removed = reg.remove("nope");
        assert!(removed.is_none());
    }

    #[test]
    fn registry_get_arc() {
        let mut reg = BackendRegistry::default();
        reg.register("mock", MockBackend);
        let arc = reg.get_arc("mock");
        assert!(arc.is_some());
    }

    #[test]
    fn registry_list_is_sorted() {
        let mut reg = BackendRegistry::default();
        reg.register("z-backend", MockBackend);
        reg.register("a-backend", MockBackend);
        let list = reg.list();
        assert_eq!(list, vec!["a-backend", "z-backend"]);
    }
}

// ============================================================================
// Module: Capability checks
// ============================================================================
mod capability_checks {
    use super::*;

    #[test]
    fn check_capabilities_unknown_backend() {
        let rt = Runtime::new();
        let reqs = CapabilityRequirements::default();
        let err = rt.check_capabilities("nonexistent", &reqs).unwrap_err();
        assert!(matches!(err, RuntimeError::UnknownBackend { .. }));
    }

    #[test]
    fn check_capabilities_empty_reqs_passes() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements::default();
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    #[test]
    fn check_capabilities_streaming_native() {
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
    fn check_capabilities_unsupported_fails() {
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
    fn check_capabilities_emulated_satisfied() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::ToolRead,
                min_support: MinSupport::Emulated,
            }],
        };
        rt.check_capabilities("mock", &reqs).unwrap();
    }

    #[test]
    fn check_capabilities_multiple_requirements() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![
                CapabilityRequirement {
                    capability: Capability::Streaming,
                    min_support: MinSupport::Native,
                },
                CapabilityRequirement {
                    capability: Capability::ToolEdit,
                    min_support: MinSupport::Emulated,
                },
            ],
        };
        rt.check_capabilities("mock", &reqs).unwrap();
    }
}

// ============================================================================
// Module: Runtime builder / configuration
// ============================================================================
mod runtime_config {
    use super::*;

    #[test]
    fn runtime_default_has_no_backends() {
        let rt = Runtime::default();
        assert!(rt.backend_names().is_empty());
    }

    #[test]
    fn runtime_default_has_no_projection() {
        let rt = Runtime::default();
        assert!(rt.projection().is_none());
    }

    #[test]
    fn runtime_default_has_no_emulation() {
        let rt = Runtime::default();
        assert!(rt.emulation_config().is_none());
    }

    #[test]
    fn runtime_with_emulation() {
        let config = abp_emulation::EmulationConfig::new();
        let rt = Runtime::new().with_emulation(config);
        assert!(rt.emulation_config().is_some());
    }

    #[test]
    fn runtime_with_stream_pipeline() {
        let pipeline = StreamPipeline::new();
        let rt = Runtime::new().with_stream_pipeline(pipeline);
        assert!(rt.stream_pipeline().is_some());
    }

    #[test]
    fn runtime_metrics_initial_snapshot() {
        let rt = Runtime::new();
        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 0);
    }

    #[test]
    fn runtime_receipt_chain_starts_empty() {
        let rt = Runtime::new();
        let chain = rt.receipt_chain();
        let rt_handle = tokio::runtime::Runtime::new().unwrap();
        let locked = rt_handle.block_on(chain.lock());
        assert_eq!(locked.len(), 0);
    }
}

// ============================================================================
// Module: Event count and custom backend
// ============================================================================
mod custom_backend_events {
    use super::*;

    #[tokio::test]
    async fn event_count_backend_emits_correct_number() {
        let mut rt = Runtime::new();
        rt.register_backend("ec", EventCountBackend { event_count: 5 });
        let handle = rt
            .run_streaming("ec", simple_work_order("count"))
            .await
            .unwrap();
        let events: Vec<_> = handle.events.collect().await;
        assert!(events.len() >= 5);
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn zero_event_backend_still_produces_receipt() {
        let mut rt = Runtime::new();
        rt.register_backend("ec", EventCountBackend { event_count: 0 });
        let handle = rt
            .run_streaming("ec", simple_work_order("zero"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn many_events_backend_completes() {
        let mut rt = Runtime::new();
        rt.register_backend("ec", EventCountBackend { event_count: 100 });
        let handle = rt
            .run_streaming("ec", simple_work_order("many"))
            .await
            .unwrap();
        let events: Vec<_> = handle.events.collect().await;
        assert!(events.len() >= 100);
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn mixed_event_backend_all_kinds_received() {
        let mut rt = Runtime::new();
        rt.register_backend("mixed", MixedEventBackend);
        let handle = rt
            .run_streaming("mixed", simple_work_order("kinds"))
            .await
            .unwrap();
        let events: Vec<_> = handle.events.collect().await;
        let has_start = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::RunStarted { .. }));
        let has_delta = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
        let has_tool_call = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }));
        let has_tool_result = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::ToolResult { .. }));
        let has_error = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::Error { .. }));
        let has_completed = events
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));

        assert!(has_start, "missing RunStarted");
        assert!(has_delta, "missing AssistantDelta");
        assert!(has_tool_call, "missing ToolCall");
        assert!(has_tool_result, "missing ToolResult");
        assert!(has_error, "missing Error");
        assert!(has_completed, "missing RunCompleted");
        let _ = handle.receipt.await;
    }

    #[tokio::test]
    async fn slow_backend_completes_eventually() {
        let mut rt = Runtime::new();
        rt.register_backend("slow", SlowBackend { delay_ms: 50 });
        let handle = rt
            .run_streaming("slow", simple_work_order("slow"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ============================================================================
// Module: Work order builder variations
// ============================================================================
mod work_order_variations {
    use super::*;

    #[tokio::test]
    async fn empty_task_string() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn long_task_string() {
        let rt = Runtime::with_default_backends();
        let task = "a".repeat(10_000);
        let wo = WorkOrderBuilder::new(&task)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn unicode_task_string() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("编写代码 🚀 données")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn with_policy_profile() {
        let rt = Runtime::with_default_backends();
        let policy = PolicyProfile {
            allowed_tools: vec!["read".into()],
            disallowed_tools: vec!["write".into()],
            ..Default::default()
        };
        let wo = WorkOrderBuilder::new("policy test")
            .policy(policy)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn with_runtime_config_model() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("model test")
            .model("gpt-4")
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn with_max_budget() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("budget test")
            .max_budget_usd(1.0)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn with_max_turns() {
        let rt = Runtime::with_default_backends();
        let wo = WorkOrderBuilder::new("turns test")
            .max_turns(10)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn with_capability_requirements() {
        let rt = Runtime::with_default_backends();
        let reqs = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let wo = WorkOrderBuilder::new("caps test")
            .requirements(reqs)
            .workspace_mode(WorkspaceMode::PassThrough)
            .build();
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }
}

// ============================================================================
// Module: Error code taxonomy integration
// ============================================================================
mod error_taxonomy {
    use super::*;

    #[test]
    fn all_runtime_error_variants_have_codes() {
        let errors: Vec<RuntimeError> = vec![
            RuntimeError::UnknownBackend { name: "x".into() },
            RuntimeError::WorkspaceFailed(anyhow::anyhow!("err")),
            RuntimeError::PolicyFailed(anyhow::anyhow!("err")),
            RuntimeError::BackendFailed(anyhow::anyhow!("err")),
            RuntimeError::CapabilityCheckFailed("err".into()),
            RuntimeError::NoProjectionMatch {
                reason: "none".into(),
            },
        ];
        for err in &errors {
            let code = err.error_code();
            let s = code.as_str();
            assert!(!s.is_empty());
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "as_str should be snake_case, got: {s}"
            );
        }
    }

    #[test]
    fn error_codes_are_distinct() {
        let codes = vec![
            RuntimeError::UnknownBackend { name: "x".into() }.error_code(),
            RuntimeError::WorkspaceFailed(anyhow::anyhow!("")).error_code(),
            RuntimeError::PolicyFailed(anyhow::anyhow!("")).error_code(),
            RuntimeError::BackendFailed(anyhow::anyhow!("")).error_code(),
            RuntimeError::CapabilityCheckFailed("".into()).error_code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().map(|c| c.as_str()).collect();
        assert_eq!(unique.len(), codes.len(), "some error codes collide");
    }

    #[test]
    fn into_abp_error_for_all_variants() {
        let errors: Vec<RuntimeError> = vec![
            RuntimeError::UnknownBackend { name: "x".into() },
            RuntimeError::WorkspaceFailed(anyhow::anyhow!("err")),
            RuntimeError::PolicyFailed(anyhow::anyhow!("err")),
            RuntimeError::BackendFailed(anyhow::anyhow!("err")),
            RuntimeError::CapabilityCheckFailed("err".into()),
            RuntimeError::NoProjectionMatch {
                reason: "none".into(),
            },
        ];
        for err in errors {
            let code = err.error_code();
            let abp_err = err.into_abp_error();
            assert_eq!(abp_err.code, code);
        }
    }
}

// ============================================================================
// Module: Receipt chain integration
// ============================================================================
mod receipt_chain_integration {
    use super::*;

    #[tokio::test]
    async fn chain_grows_with_each_run() {
        let rt = Runtime::with_default_backends();
        let chain = rt.receipt_chain();

        for i in 0..3 {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("chain-{i}")))
                .await
                .unwrap();
            let _ = handle.receipt.await.unwrap().unwrap();
        }

        let locked = chain.lock().await;
        assert_eq!(locked.len(), 3);
    }

    #[tokio::test]
    async fn chain_receipts_have_unique_ids() {
        let rt = Runtime::with_default_backends();
        let chain = rt.receipt_chain();

        for i in 0..3 {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("uniq-{i}")))
                .await
                .unwrap();
            let _ = handle.receipt.await.unwrap().unwrap();
        }

        let locked = chain.lock().await;
        let ids: Vec<_> = locked.iter().map(|r| r.meta.run_id).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[tokio::test]
    async fn chain_receipts_all_have_hashes() {
        let rt = Runtime::with_default_backends();
        let chain = rt.receipt_chain();

        for i in 0..3 {
            let handle = rt
                .run_streaming("mock", simple_work_order(&format!("hash-{i}")))
                .await
                .unwrap();
            let _ = handle.receipt.await.unwrap().unwrap();
        }

        let locked = chain.lock().await;
        for receipt in locked.iter() {
            assert!(receipt.receipt_sha256.is_some());
        }
    }
}

// ============================================================================
// Module: Edge cases and stress tests
// ============================================================================
mod edge_cases {
    use super::*;

    #[tokio::test]
    async fn runtime_can_be_used_after_backend_error() {
        let mut rt = Runtime::new();
        rt.register_backend("mock", MockBackend);
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "oops".into(),
            },
        );

        // Fail first.
        let handle = rt
            .run_streaming("fail", simple_work_order("fail"))
            .await
            .unwrap();
        let _ = handle.receipt.await;

        // Then succeed.
        let handle = rt
            .run_streaming("mock", simple_work_order("ok"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn runtime_can_be_shared_via_arc() {
        let rt = Arc::new(Runtime::with_default_backends());
        let rt2 = rt.clone();
        let h1 = tokio::spawn(async move {
            let handle = rt2
                .run_streaming("mock", simple_work_order("t1"))
                .await
                .unwrap();
            handle.receipt.await.unwrap().unwrap()
        });
        let h2 = {
            let rt3 = rt.clone();
            tokio::spawn(async move {
                let handle = rt3
                    .run_streaming("mock", simple_work_order("t2"))
                    .await
                    .unwrap();
                handle.receipt.await.unwrap().unwrap()
            })
        };
        let (r1, r2) = tokio::join!(h1, h2);
        assert_eq!(r1.unwrap().outcome, Outcome::Complete);
        assert_eq!(r2.unwrap().outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn dropping_events_stream_does_not_block_receipt() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("drop events"))
            .await
            .unwrap();
        // Intentionally drop events without consuming.
        drop(handle.events);
        let receipt = handle.receipt.await.unwrap().unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn receipt_sha256_is_deterministic_for_same_receipt() {
        let rt = Runtime::with_default_backends();
        let handle = rt
            .run_streaming("mock", simple_work_order("det"))
            .await
            .unwrap();
        let receipt = handle.receipt.await.unwrap().unwrap();
        // Recompute hash.
        let recomputed = abp_receipt::compute_hash(&receipt).unwrap();
        // The stored hash was computed before storage.
        let stored = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(stored, &recomputed);
    }

    #[tokio::test]
    async fn receipt_trace_events_match_stream_events() {
        let mut rt = Runtime::new();
        rt.register_backend("ec", EventCountBackend { event_count: 3 });
        let handle = rt
            .run_streaming("ec", simple_work_order("trace match"))
            .await
            .unwrap();
        let stream_events: Vec<_> = handle.events.collect().await;
        let receipt = handle.receipt.await.unwrap().unwrap();
        // The trace should contain at least as many events as the stream delivered.
        assert!(
            receipt.trace.len() >= stream_events.len(),
            "trace={} < stream={}",
            receipt.trace.len(),
            stream_events.len()
        );
    }
}
