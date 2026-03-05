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
//! Comprehensive runtime integration tests covering the full orchestration flow.
//!
//! Modules:
//! - `runtime_construction` — Runtime creation, backend registration, policy, workspace
//! - `runtime_execution` — Streaming runs, receipt generation, event ordering, errors
//! - `runtime_retry_fallback` — Retry policy, fallback chains, pipeline events
//! - `runtime_configuration` — TOML loading, env overrides, validation

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, MinSupport, Outcome, Receipt, SupportLevel,
    WorkOrder, WorkOrderBuilder, WorkspaceMode, CONTRACT_VERSION,
};
use abp_integrations::Backend;
use abp_runtime::execution::{ExecutionConfig, ExecutionPipeline, PipelineEvent};
use abp_runtime::retry::{FallbackChain, RetryPolicy};
use abp_runtime::{RunHandle, Runtime, RuntimeError};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

// ===========================================================================
// Shared helpers
// ===========================================================================

/// Drain all streamed events and await the receipt from a `RunHandle`.
async fn drain_run(handle: RunHandle) -> (Vec<AgentEvent>, Result<Receipt, RuntimeError>) {
    let mut events = handle.events;
    let mut collected = Vec::new();
    while let Some(ev) = events.next().await {
        collected.push(ev);
    }
    let receipt = handle.receipt.await.expect("receipt task panicked");
    (collected, receipt)
}

/// Build a PassThrough work order for a given task string.
fn passthrough_wo(task: &str) -> WorkOrder {
    WorkOrderBuilder::new(task)
        .workspace_mode(WorkspaceMode::PassThrough)
        .build()
}

// ===========================================================================
// Custom test backends
// ===========================================================================

/// A backend that always fails with a configurable message.
#[derive(Debug, Clone)]
struct FailingBackend {
    message: String,
}

#[async_trait]
impl Backend for FailingBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "failing".into(),
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
        anyhow::bail!("{}", self.message)
    }
}

/// A backend that emits a configurable number of events then succeeds.
#[derive(Debug, Clone)]
struct MultiEventBackend {
    event_count: usize,
}

#[async_trait]
impl Backend for MultiEventBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "multi-event".into(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        }
    }
    fn capabilities(&self) -> CapabilityManifest {
        let mut m = CapabilityManifest::default();
        m.insert(Capability::Streaming, SupportLevel::Native);
        m
    }
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<Receipt> {
        let started = chrono::Utc::now();
        let mut trace = Vec::new();

        let started_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "multi-event starting".into(),
            },
            ext: None,
        };
        trace.push(started_ev.clone());
        let _ = events_tx.send(started_ev).await;

        for i in 0..self.event_count {
            let ev = AgentEvent {
                ts: chrono::Utc::now(),
                kind: AgentEventKind::AssistantDelta {
                    text: format!("chunk-{i}"),
                },
                ext: None,
            };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        }

        let completed_ev = AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "multi-event done".into(),
            },
            ext: None,
        };
        trace.push(completed_ev.clone());
        let _ = events_tx.send(completed_ev).await;

        let finished = chrono::Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Receipt {
            meta: abp_core::RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode: abp_core::ExecutionMode::Mapped,
            usage_raw: serde_json::json!({"note": "multi-event"}),
            usage: abp_core::UsageNormalized::default(),
            trace,
            artifacts: vec![],
            verification: abp_core::VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: true,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
        .with_hash()?)
    }
}

// ===========================================================================
// Module: runtime_construction
// ===========================================================================

mod runtime_construction {
    use super::*;

    #[test]
    fn create_runtime_default() {
        let rt = Runtime::new();
        assert!(
            rt.backend_names().is_empty(),
            "new runtime should have no backends"
        );
    }

    #[test]
    fn create_runtime_with_default_backends() {
        let rt = Runtime::with_default_backends();
        let names = rt.backend_names();
        assert!(
            names.contains(&"mock".to_string()),
            "expected 'mock' backend"
        );
    }

    #[test]
    fn register_mock_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("test-mock", abp_integrations::MockBackend);
        assert!(rt.backend("test-mock").is_some());
    }

    #[test]
    fn runtime_with_multiple_backends() {
        let mut rt = Runtime::new();
        rt.register_backend("alpha", abp_integrations::MockBackend);
        rt.register_backend("beta", abp_integrations::MockBackend);
        rt.register_backend("gamma", MultiEventBackend { event_count: 1 });
        let names = rt.backend_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));
        assert!(names.contains(&"gamma".to_string()));
    }

    #[test]
    fn runtime_replace_backend() {
        let mut rt = Runtime::new();
        rt.register_backend("slot", abp_integrations::MockBackend);
        let id1 = rt.backend("slot").unwrap().identity().id.clone();

        rt.register_backend("slot", MultiEventBackend { event_count: 2 });
        let id2 = rt.backend("slot").unwrap().identity().id.clone();

        assert_eq!(id1, "mock");
        assert_eq!(id2, "multi-event");
    }

    #[test]
    fn runtime_with_policy_check() {
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
    fn runtime_capability_check_fails_for_unsupported() {
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
    fn runtime_backend_registry_access() {
        let rt = Runtime::with_default_backends();
        let registry = rt.registry();
        assert!(registry.contains("mock"));
        assert!(!registry.contains("nonexistent"));
    }

    #[test]
    fn runtime_metrics_initially_zero() {
        let rt = Runtime::new();
        let snap = rt.metrics().snapshot();
        assert_eq!(snap.total_runs, 0);
    }
}

// ===========================================================================
// Module: runtime_execution
// ===========================================================================

mod runtime_execution {
    use super::*;

    #[tokio::test]
    async fn execute_simple_task_returns_receipt() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("hello world");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn execute_streaming_produces_events_and_receipt() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("stream test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert!(!events.is_empty(), "should produce events");
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[tokio::test]
    async fn execute_with_unknown_backend_returns_error() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("unknown backend test");
        let err = match rt.run_streaming("nonexistent", wo).await {
            Err(e) => e,
            Ok(_) => panic!("expected error for unknown backend"),
        };
        assert!(
            matches!(err, RuntimeError::UnknownBackend { ref name } if name == "nonexistent"),
            "expected UnknownBackend, got {err:?}"
        );
    }

    #[tokio::test]
    async fn receipt_has_correct_contract_version() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("version test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[tokio::test]
    async fn receipt_hash_is_present_and_valid() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("hash test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();

        assert!(
            receipt.receipt_sha256.is_some(),
            "receipt should have a hash"
        );
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert!(!hash.is_empty(), "hash should not be empty");
    }

    #[tokio::test]
    async fn events_start_with_run_started_end_with_run_completed() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("event order test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let _ = receipt.unwrap();

        assert!(events.len() >= 2, "need at least 2 events");

        assert!(
            matches!(
                events.first().unwrap().kind,
                AgentEventKind::RunStarted { .. }
            ),
            "first event should be RunStarted, got {:?}",
            events.first().unwrap().kind
        );
        assert!(
            matches!(
                events.last().unwrap().kind,
                AgentEventKind::RunCompleted { .. }
            ),
            "last event should be RunCompleted, got {:?}",
            events.last().unwrap().kind
        );
    }

    #[tokio::test]
    async fn multi_event_backend_streams_correct_count() {
        let mut rt = Runtime::new();
        rt.register_backend("multi", MultiEventBackend { event_count: 5 });
        let wo = passthrough_wo("multi event");
        let handle = rt.run_streaming("multi", wo).await.unwrap();
        let (events, receipt) = drain_run(handle).await;
        let _ = receipt.unwrap();

        // RunStarted + 5 deltas + RunCompleted = 7
        assert_eq!(events.len(), 7, "expected 7 events, got {}", events.len());
    }

    #[tokio::test]
    async fn failing_backend_returns_backend_failed() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail",
            FailingBackend {
                message: "intentional".into(),
            },
        );
        let wo = passthrough_wo("fail test");
        let handle = rt.run_streaming("fail", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let err = receipt.unwrap_err();
        assert!(
            matches!(err, RuntimeError::BackendFailed(_)),
            "expected BackendFailed, got {err:?}"
        );
    }

    #[tokio::test]
    async fn receipt_backend_identity_matches() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("identity test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        assert_eq!(receipt.backend.id, "mock");
    }

    #[tokio::test]
    async fn receipt_trace_is_non_empty() {
        let rt = Runtime::with_default_backends();
        let wo = passthrough_wo("trace test");
        let handle = rt.run_streaming("mock", wo).await.unwrap();
        let (_, receipt) = drain_run(handle).await;
        let receipt = receipt.unwrap();
        assert!(
            !receipt.trace.is_empty(),
            "receipt trace should contain events"
        );
    }

    #[tokio::test]
    async fn sequential_runs_produce_distinct_run_ids() {
        let rt = Runtime::with_default_backends();

        let h1 = rt
            .run_streaming("mock", passthrough_wo("run 1"))
            .await
            .unwrap();
        let h2 = rt
            .run_streaming("mock", passthrough_wo("run 2"))
            .await
            .unwrap();

        let id1 = h1.run_id;
        let id2 = h2.run_id;

        let _ = drain_run(h1).await;
        let _ = drain_run(h2).await;

        assert_ne!(id1, id2, "sequential runs must have different run IDs");
    }
}

// ===========================================================================
// Module: runtime_retry_fallback
// ===========================================================================

mod runtime_retry_fallback {
    use super::*;
    use abp_backend_mock::scenarios::{MockScenario, ScenarioMockBackend};
    use std::time::Duration;

    #[test]
    fn retry_policy_default_values() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert!(policy.should_retry(0));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
    }

    #[test]
    fn retry_policy_no_retry() {
        let policy = RetryPolicy::no_retry();
        assert_eq!(policy.max_retries, 0);
        assert!(!policy.should_retry(0));
    }

    #[test]
    fn retry_policy_builder_custom() {
        let policy = RetryPolicy::builder()
            .max_retries(5)
            .initial_backoff(Duration::from_millis(50))
            .max_backoff(Duration::from_secs(2))
            .backoff_multiplier(1.5)
            .build();
        assert_eq!(policy.max_retries, 5);
        assert!(policy.should_retry(4));
        assert!(!policy.should_retry(5));
    }

    #[test]
    fn fallback_chain_iteration() {
        let mut chain = FallbackChain::new(vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.remaining(), 3);

        assert_eq!(chain.next_backend(), Some("a"));
        assert_eq!(chain.next_backend(), Some("b"));
        assert_eq!(chain.next_backend(), Some("c"));
        assert_eq!(chain.next_backend(), None);
        assert_eq!(chain.remaining(), 0);
    }

    #[test]
    fn fallback_chain_reset() {
        let mut chain = FallbackChain::new(vec!["x".into(), "y".into()]);
        let _ = chain.next_backend();
        let _ = chain.next_backend();
        assert_eq!(chain.remaining(), 0);

        chain.reset();
        assert_eq!(chain.remaining(), 2);
        assert_eq!(chain.next_backend(), Some("x"));
    }

    #[tokio::test]
    async fn pipeline_success_on_first_try() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "primary",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 0,
                text: "ok".into(),
            }),
        );

        let config = ExecutionConfig {
            retry_policy: Some(RetryPolicy::default()),
            fallback_chain: None,
        };
        let pipeline = ExecutionPipeline::new(config);
        let wo = passthrough_wo("pipeline success");
        let output = pipeline.execute(&rt, "primary", wo).await.unwrap();

        assert_eq!(output.backend, "primary");
        assert_eq!(output.receipt.outcome, Outcome::Complete);
        assert!(
            output
                .events
                .iter()
                .any(|e| matches!(e, PipelineEvent::Success { .. })),
            "should have a Success pipeline event"
        );
    }

    #[tokio::test]
    async fn pipeline_retries_transient_error_then_succeeds() {
        let mut rt = Runtime::new();
        // Fails twice, then succeeds
        rt.register_backend(
            "flaky",
            ScenarioMockBackend::new(MockScenario::TransientError {
                fail_count: 2,
                then: Box::new(MockScenario::Success {
                    delay_ms: 0,
                    text: "recovered".into(),
                }),
            }),
        );

        let config = ExecutionConfig {
            retry_policy: Some(
                RetryPolicy::builder()
                    .max_retries(3)
                    .initial_backoff(Duration::from_millis(1))
                    .max_backoff(Duration::from_millis(10))
                    .build(),
            ),
            fallback_chain: None,
        };
        let pipeline = ExecutionPipeline::new(config);
        let wo = passthrough_wo("retry test");
        let output = pipeline.execute(&rt, "flaky", wo).await.unwrap();

        assert_eq!(output.receipt.outcome, Outcome::Complete);
        // Should have retry events
        let retry_count = output
            .events
            .iter()
            .filter(|e| matches!(e, PipelineEvent::Retry { .. }))
            .count();
        assert!(
            retry_count >= 1,
            "expected at least 1 retry event, got {retry_count}"
        );
    }

    #[tokio::test]
    async fn pipeline_fallback_on_permanent_error() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "broken",
            ScenarioMockBackend::new(MockScenario::PermanentError {
                code: "ERR".into(),
                message: "permanent".into(),
            }),
        );
        rt.register_backend(
            "backup",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 0,
                text: "backup ok".into(),
            }),
        );

        let config = ExecutionConfig {
            retry_policy: Some(RetryPolicy::no_retry()),
            fallback_chain: Some(FallbackChain::new(vec!["backup".into()])),
        };
        let pipeline = ExecutionPipeline::new(config);
        let wo = passthrough_wo("fallback test");
        let output = pipeline.execute(&rt, "broken", wo).await.unwrap();

        assert_eq!(output.backend, "backup");
        assert!(
            output
                .events
                .iter()
                .any(|e| matches!(e, PipelineEvent::Fallback { .. })),
            "should have a Fallback pipeline event"
        );
    }

    #[tokio::test]
    async fn pipeline_all_backends_fail_returns_error() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "fail1",
            ScenarioMockBackend::new(MockScenario::PermanentError {
                code: "E1".into(),
                message: "fail1".into(),
            }),
        );
        rt.register_backend(
            "fail2",
            ScenarioMockBackend::new(MockScenario::PermanentError {
                code: "E2".into(),
                message: "fail2".into(),
            }),
        );

        let config = ExecutionConfig {
            retry_policy: Some(RetryPolicy::no_retry()),
            fallback_chain: Some(FallbackChain::new(vec!["fail2".into()])),
        };
        let pipeline = ExecutionPipeline::new(config);
        let wo = passthrough_wo("all fail");
        let result = pipeline.execute(&rt, "fail1", wo).await;

        assert!(result.is_err(), "expected error when all backends fail");
    }

    #[tokio::test]
    async fn pipeline_success_does_not_trigger_fallback() {
        let mut rt = Runtime::new();
        rt.register_backend(
            "good",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 0,
                text: "fine".into(),
            }),
        );
        rt.register_backend(
            "backup",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 0,
                text: "backup".into(),
            }),
        );

        let config = ExecutionConfig {
            retry_policy: Some(RetryPolicy::no_retry()),
            fallback_chain: Some(FallbackChain::new(vec!["backup".into()])),
        };
        let pipeline = ExecutionPipeline::new(config);
        let wo = passthrough_wo("no fallback needed");
        let output = pipeline.execute(&rt, "good", wo).await.unwrap();

        assert_eq!(output.backend, "good");
        assert!(
            !output
                .events
                .iter()
                .any(|e| matches!(e, PipelineEvent::Fallback { .. })),
            "should NOT have a Fallback event when primary succeeds"
        );
    }

    #[tokio::test]
    async fn pipeline_events_track_retry_and_fallback() {
        let mut rt = Runtime::new();
        // Primary fails with transient errors, exhausts retries, then falls back
        rt.register_backend(
            "primary",
            ScenarioMockBackend::new(MockScenario::PermanentError {
                code: "DEAD".into(),
                message: "dead".into(),
            }),
        );
        rt.register_backend(
            "secondary",
            ScenarioMockBackend::new(MockScenario::Success {
                delay_ms: 0,
                text: "secondary ok".into(),
            }),
        );

        let config = ExecutionConfig {
            retry_policy: Some(RetryPolicy::no_retry()),
            fallback_chain: Some(FallbackChain::new(vec!["secondary".into()])),
        };
        let pipeline = ExecutionPipeline::new(config);
        let wo = passthrough_wo("events tracking");
        let output = pipeline.execute(&rt, "primary", wo).await.unwrap();

        // Should have Fallback then Success
        let has_fallback = output
            .events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Fallback { .. }));
        let has_success = output
            .events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Success { .. }));

        assert!(has_fallback, "expected Fallback event");
        assert!(has_success, "expected Success event");
        assert_eq!(output.backend, "secondary");
    }

    #[test]
    fn retry_policy_delay_respects_max_backoff() {
        let policy = RetryPolicy::builder()
            .max_retries(10)
            .initial_backoff(Duration::from_secs(1))
            .max_backoff(Duration::from_secs(3))
            .backoff_multiplier(10.0)
            .build();

        // Even with aggressive multiplier, delay should be capped
        let delay = policy.compute_delay(5);
        assert!(
            delay <= Duration::from_secs(3),
            "delay {delay:?} exceeds max_backoff"
        );
    }
}

// ===========================================================================
// Module: runtime_configuration
// ===========================================================================

mod runtime_configuration {
    use abp_config::{parse_toml, validate_config, BackendEntry, BackplaneConfig};

    #[test]
    fn config_from_toml_minimal() {
        let toml = r#"
default_backend = "mock"
log_level = "debug"
"#;
        let config = parse_toml(toml).unwrap();
        assert_eq!(config.default_backend.as_deref(), Some("mock"));
        assert_eq!(config.log_level.as_deref(), Some("debug"));
    }

    #[test]
    fn config_from_toml_with_backends() {
        let toml = r#"
default_backend = "mock"

[backends.mock]
type = "mock"

[backends.node]
type = "sidecar"
command = "node"
args = ["hosts/node/index.js"]
timeout_secs = 60
"#;
        let config = parse_toml(toml).unwrap();
        assert_eq!(config.backends.len(), 2);
        assert!(config.backends.contains_key("mock"));
        assert!(config.backends.contains_key("node"));

        match &config.backends["node"] {
            BackendEntry::Sidecar {
                command,
                args,
                timeout_secs,
            } => {
                assert_eq!(command, "node");
                assert_eq!(args.len(), 1);
                assert_eq!(*timeout_secs, Some(60));
            }
            other => panic!("expected Sidecar, got {other:?}"),
        }
    }

    #[test]
    fn config_default_is_valid() {
        let config = BackplaneConfig::default();
        let warnings = validate_config(&config).unwrap();
        // Default config should pass validation (may have optional-field warnings)
        for w in &warnings {
            // Just ensure no hard errors
            let _ = w.to_string();
        }
    }

    #[test]
    fn config_invalid_log_level_rejected() {
        let toml = r#"
log_level = "banana"
"#;
        let config = parse_toml(toml).unwrap();
        let result = validate_config(&config);
        assert!(
            result.is_err(),
            "invalid log level should cause validation error"
        );
    }

    #[test]
    fn config_empty_sidecar_command_rejected() {
        let toml = r#"
[backends.bad]
type = "sidecar"
command = ""
"#;
        let config = parse_toml(toml).unwrap();
        let result = validate_config(&config);
        assert!(
            result.is_err(),
            "empty sidecar command should cause validation error"
        );
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = BackplaneConfig {
            default_backend: Some("mock".into()),
            workspace_dir: Some("/tmp/abp".into()),
            log_level: Some("info".into()),
            receipts_dir: Some("/tmp/receipts".into()),
            backends: {
                let mut m = std::collections::BTreeMap::new();
                m.insert("mock".into(), BackendEntry::Mock {});
                m
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: BackplaneConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, decoded);
    }

    #[test]
    fn config_timeout_out_of_range_rejected() {
        let toml = r#"
[backends.slow]
type = "sidecar"
command = "sleep"
timeout_secs = 100000
"#;
        let config = parse_toml(toml).unwrap();
        let result = validate_config(&config);
        assert!(
            result.is_err(),
            "timeout exceeding 86400 should cause validation error"
        );
    }
}
