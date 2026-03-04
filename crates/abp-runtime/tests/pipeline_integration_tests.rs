// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the retry/fallback execution pipeline.

use abp_backend_core::Backend;
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    ExecutionLane, Outcome, PolicyProfile, Receipt, RunMetadata, UsageNormalized,
    VerificationReport, WorkOrder, WorkspaceMode, WorkspaceSpec,
};
use abp_runtime::Runtime;
use abp_runtime::execution::{ExecutionConfig, ExecutionPipeline, PipelineEvent};
use abp_runtime::retry::{FallbackChain, RetryPolicy};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn mock_work_order() -> WorkOrder {
    WorkOrder {
        id: Uuid::new_v4(),
        task: "pipeline test".into(),
        lane: ExecutionLane::PatchFirst,
        workspace: WorkspaceSpec {
            root: ".".into(),
            mode: WorkspaceMode::PassThrough,
            include: vec![],
            exclude: vec![],
        },
        context: abp_core::ContextPacket::default(),
        policy: PolicyProfile::default(),
        requirements: abp_core::CapabilityRequirements::default(),
        config: abp_core::RuntimeConfig::default(),
    }
}

fn mock_receipt(run_id: Uuid, work_order_id: Uuid, backend_id: &str) -> Receipt {
    let now = Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: now,
            finished_at: now,
            duration_ms: 0,
        },
        backend: BackendIdentity {
            id: backend_id.to_string(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
        },
        capabilities: CapabilityManifest::default(),
        mode: abp_core::ExecutionMode::Mapped,
        usage_raw: serde_json::json!({}),
        usage: UsageNormalized::default(),
        trace: vec![],
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
}

/// A backend that always succeeds on its `run` call.
#[derive(Debug, Clone)]
struct SuccessBackend {
    id: String,
    call_count: Arc<AtomicU32>,
}

impl SuccessBackend {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn calls(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Backend for SuccessBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.id.clone(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
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
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "ok".into(),
                },
                ext: None,
            })
            .await;
        Ok(mock_receipt(run_id, work_order.id, &self.id))
    }
}

/// A backend that always fails with a retryable error.
#[derive(Debug, Clone)]
struct TransientFailBackend {
    id: String,
    call_count: Arc<AtomicU32>,
}

impl TransientFailBackend {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn calls(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Backend for TransientFailBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.id.clone(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
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
        self.call_count.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("transient failure: connection reset")
    }
}

/// A backend that fails N times then succeeds.
#[derive(Debug, Clone)]
struct FailThenSucceedBackend {
    id: String,
    failures_remaining: Arc<AtomicU32>,
    call_count: Arc<AtomicU32>,
}

impl FailThenSucceedBackend {
    fn new(id: &str, failures: u32) -> Self {
        Self {
            id: id.to_string(),
            failures_remaining: Arc::new(AtomicU32::new(failures)),
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn calls(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Backend for FailThenSucceedBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: self.id.clone(),
            backend_version: Some("0.1".into()),
            adapter_version: Some("0.1".into()),
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
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let remaining = self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
        if remaining > 0 {
            anyhow::bail!(
                "transient failure #{}",
                self.call_count.load(Ordering::SeqCst)
            )
        }
        let _ = events_tx
            .send(AgentEvent {
                ts: Utc::now(),
                kind: AgentEventKind::RunCompleted {
                    message: "recovered".into(),
                },
                ext: None,
            })
            .await;
        Ok(mock_receipt(run_id, work_order.id, &self.id))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// 1. Success on first try — no retries or fallbacks needed.
#[tokio::test]
async fn success_on_first_try() {
    let backend = SuccessBackend::new("primary");
    let mut rt = Runtime::new();
    rt.register_backend("primary", backend.clone());

    let pipeline = ExecutionPipeline::new(ExecutionConfig::default());
    let result = pipeline
        .execute(&rt, "primary", mock_work_order())
        .await
        .unwrap();

    assert_eq!(result.backend, "primary");
    assert_eq!(backend.calls(), 1);
    // Should have exactly one Success event.
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Success { .. }))
    );
    assert!(
        result
            .events
            .iter()
            .all(|e| !matches!(e, PipelineEvent::Retry { .. }))
    );
}

// 2. Retry once then succeed.
#[tokio::test]
async fn retry_once_then_succeed() {
    let backend = FailThenSucceedBackend::new("primary", 1);
    let mut rt = Runtime::new();
    rt.register_backend("primary", backend.clone());

    let config = ExecutionConfig {
        retry_policy: Some(
            RetryPolicy::builder()
                .max_retries(3)
                .initial_backoff(Duration::from_millis(1))
                .max_backoff(Duration::from_millis(5))
                .build(),
        ),
        fallback_chain: None,
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline
        .execute(&rt, "primary", mock_work_order())
        .await
        .unwrap();

    assert_eq!(result.backend, "primary");
    assert_eq!(backend.calls(), 2);
    assert_eq!(
        result
            .events
            .iter()
            .filter(|e| matches!(e, PipelineEvent::Retry { .. }))
            .count(),
        1
    );
}

// 3. Exhaust all retries on a single backend.
#[tokio::test]
async fn exhaust_retries_single_backend() {
    let backend = TransientFailBackend::new("primary");
    let mut rt = Runtime::new();
    rt.register_backend("primary", backend.clone());

    let config = ExecutionConfig {
        retry_policy: Some(
            RetryPolicy::builder()
                .max_retries(2)
                .initial_backoff(Duration::from_millis(1))
                .max_backoff(Duration::from_millis(5))
                .build(),
        ),
        fallback_chain: None,
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline.execute(&rt, "primary", mock_work_order()).await;

    assert!(result.is_err());
    // 1 initial + 2 retries = 3 attempts.
    assert_eq!(backend.calls(), 3);
}

// 4. Fallback to second backend on permanent error.
#[tokio::test]
async fn fallback_on_permanent_error() {
    let mut rt = Runtime::new();
    // Primary always fails (transient, but no retries configured).
    rt.register_backend("primary", TransientFailBackend::new("primary"));
    let secondary = SuccessBackend::new("secondary");
    rt.register_backend("secondary", secondary.clone());

    let config = ExecutionConfig {
        retry_policy: None, // No retries — fail immediately to fallback.
        fallback_chain: Some(FallbackChain::new(vec!["secondary".into()])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline
        .execute(&rt, "primary", mock_work_order())
        .await
        .unwrap();

    assert_eq!(result.backend, "secondary");
    assert_eq!(secondary.calls(), 1);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Fallback { .. }))
    );
}

// 5. Fallback chain with multiple backends.
#[tokio::test]
async fn fallback_chain_multiple_backends() {
    let mut rt = Runtime::new();
    rt.register_backend("a", TransientFailBackend::new("a"));
    rt.register_backend("b", TransientFailBackend::new("b"));
    let c = SuccessBackend::new("c");
    rt.register_backend("c", c.clone());

    let config = ExecutionConfig {
        retry_policy: None,
        fallback_chain: Some(FallbackChain::new(vec!["b".into(), "c".into()])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline.execute(&rt, "a", mock_work_order()).await.unwrap();

    assert_eq!(result.backend, "c");
    let fallback_count = result
        .events
        .iter()
        .filter(|e| matches!(e, PipelineEvent::Fallback { .. }))
        .count();
    assert_eq!(fallback_count, 2); // a->b, b->c
}

// 6. All backends in chain fail.
#[tokio::test]
async fn all_backends_fail() {
    let mut rt = Runtime::new();
    rt.register_backend("a", TransientFailBackend::new("a"));
    rt.register_backend("b", TransientFailBackend::new("b"));

    let config = ExecutionConfig {
        retry_policy: None,
        fallback_chain: Some(FallbackChain::new(vec!["b".into()])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline.execute(&rt, "a", mock_work_order()).await;
    assert!(result.is_err());
}

// 7. Retry + fallback combined: retry exhausted on primary, fallback succeeds.
#[tokio::test]
async fn retry_then_fallback() {
    let primary = TransientFailBackend::new("primary");
    let secondary = SuccessBackend::new("secondary");
    let mut rt = Runtime::new();
    rt.register_backend("primary", primary.clone());
    rt.register_backend("secondary", secondary.clone());

    let config = ExecutionConfig {
        retry_policy: Some(
            RetryPolicy::builder()
                .max_retries(1)
                .initial_backoff(Duration::from_millis(1))
                .max_backoff(Duration::from_millis(5))
                .build(),
        ),
        fallback_chain: Some(FallbackChain::new(vec!["secondary".into()])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline
        .execute(&rt, "primary", mock_work_order())
        .await
        .unwrap();

    assert_eq!(result.backend, "secondary");
    // Primary: 1 initial + 1 retry = 2.
    assert_eq!(primary.calls(), 2);
    assert_eq!(secondary.calls(), 1);
    // Events should have retry(s) + fallback + success.
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Retry { .. }))
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Fallback { .. }))
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, PipelineEvent::Success { .. }))
    );
}

// 8. Unknown primary backend returns error immediately.
#[tokio::test]
async fn unknown_primary_backend() {
    let rt = Runtime::new();
    let pipeline = ExecutionPipeline::new(ExecutionConfig::default());
    let result = pipeline
        .execute(&rt, "nonexistent", mock_work_order())
        .await;
    assert!(result.is_err());
}

// 9. Empty fallback chain behaves like no fallback.
#[tokio::test]
async fn empty_fallback_chain() {
    let mut rt = Runtime::new();
    rt.register_backend("primary", TransientFailBackend::new("primary"));

    let config = ExecutionConfig {
        retry_policy: None,
        fallback_chain: Some(FallbackChain::new(vec![])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline.execute(&rt, "primary", mock_work_order()).await;
    assert!(result.is_err());
}

// 10. Default config (no retry, no fallback) — single attempt.
#[tokio::test]
async fn default_config_single_attempt() {
    let backend = SuccessBackend::new("only");
    let mut rt = Runtime::new();
    rt.register_backend("only", backend.clone());

    let pipeline = ExecutionPipeline::new(ExecutionConfig::default());
    let result = pipeline
        .execute(&rt, "only", mock_work_order())
        .await
        .unwrap();

    assert_eq!(backend.calls(), 1);
    assert_eq!(result.backend, "only");
}

// 11. Retry policy with zero retries is equivalent to single attempt.
#[tokio::test]
async fn zero_retries_single_attempt() {
    let backend = TransientFailBackend::new("sole");
    let mut rt = Runtime::new();
    rt.register_backend("sole", backend.clone());

    let config = ExecutionConfig {
        retry_policy: Some(RetryPolicy::no_retry()),
        fallback_chain: None,
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline.execute(&rt, "sole", mock_work_order()).await;

    assert!(result.is_err());
    assert_eq!(backend.calls(), 1);
}

// 12. Pipeline events record the correct backend names.
#[tokio::test]
async fn pipeline_events_have_correct_backend_names() {
    let mut rt = Runtime::new();
    rt.register_backend("alpha", TransientFailBackend::new("alpha"));
    let beta = SuccessBackend::new("beta");
    rt.register_backend("beta", beta.clone());

    let config = ExecutionConfig {
        retry_policy: None,
        fallback_chain: Some(FallbackChain::new(vec!["beta".into()])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline
        .execute(&rt, "alpha", mock_work_order())
        .await
        .unwrap();

    // Verify fallback event names.
    let fallback = result
        .events
        .iter()
        .find(|e| matches!(e, PipelineEvent::Fallback { .. }))
        .unwrap();
    match fallback {
        PipelineEvent::Fallback {
            from_backend,
            to_backend,
            ..
        } => {
            assert_eq!(from_backend, "alpha");
            assert_eq!(to_backend, "beta");
        }
        _ => unreachable!(),
    }

    // Verify success event.
    let success = result
        .events
        .iter()
        .find(|e| matches!(e, PipelineEvent::Success { .. }))
        .unwrap();
    match success {
        PipelineEvent::Success { backend, attempts } => {
            assert_eq!(backend, "beta");
            assert_eq!(*attempts, 1);
        }
        _ => unreachable!(),
    }
}

// 13. Retry events include correct attempt count.
#[tokio::test]
async fn retry_events_include_attempt_count() {
    let backend = FailThenSucceedBackend::new("retrier", 2);
    let mut rt = Runtime::new();
    rt.register_backend("retrier", backend.clone());

    let config = ExecutionConfig {
        retry_policy: Some(
            RetryPolicy::builder()
                .max_retries(5)
                .initial_backoff(Duration::from_millis(1))
                .max_backoff(Duration::from_millis(5))
                .build(),
        ),
        fallback_chain: None,
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline
        .execute(&rt, "retrier", mock_work_order())
        .await
        .unwrap();

    assert_eq!(backend.calls(), 3); // 2 failures + 1 success
    let retry_attempts: Vec<u32> = result
        .events
        .iter()
        .filter_map(|e| match e {
            PipelineEvent::Retry { attempt, .. } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(retry_attempts, vec![0, 1]);
}

// 14. Fallback skips duplicate of primary backend in chain.
#[tokio::test]
async fn fallback_chain_skips_primary_duplicate() {
    let primary = TransientFailBackend::new("primary");
    let backup = SuccessBackend::new("backup");
    let mut rt = Runtime::new();
    rt.register_backend("primary", primary.clone());
    rt.register_backend("backup", backup.clone());

    let config = ExecutionConfig {
        retry_policy: None,
        // Chain includes primary again — should be skipped.
        fallback_chain: Some(FallbackChain::new(vec!["primary".into(), "backup".into()])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline
        .execute(&rt, "primary", mock_work_order())
        .await
        .unwrap();

    assert_eq!(result.backend, "backup");
    // Primary called once (initial), not again from fallback chain.
    assert_eq!(primary.calls(), 1);
}

// 15. Success event records correct attempt count after retries.
#[tokio::test]
async fn success_event_records_attempt_count() {
    let backend = FailThenSucceedBackend::new("flaky", 3);
    let mut rt = Runtime::new();
    rt.register_backend("flaky", backend.clone());

    let config = ExecutionConfig {
        retry_policy: Some(
            RetryPolicy::builder()
                .max_retries(5)
                .initial_backoff(Duration::from_millis(1))
                .max_backoff(Duration::from_millis(5))
                .build(),
        ),
        fallback_chain: None,
    };
    let pipeline = ExecutionPipeline::new(config);
    let result = pipeline
        .execute(&rt, "flaky", mock_work_order())
        .await
        .unwrap();

    let success = result
        .events
        .iter()
        .find(|e| matches!(e, PipelineEvent::Success { .. }))
        .unwrap();
    match success {
        PipelineEvent::Success { attempts, .. } => assert_eq!(*attempts, 4),
        _ => unreachable!(),
    }
}

// 16. ExecutionConfig serialises to empty object when all fields are None.
#[test]
fn execution_config_empty_serialization() {
    let config = ExecutionConfig::default();
    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json, serde_json::json!({}));
}

// 17. PipelineOutput includes receipt from the correct backend.
#[tokio::test]
async fn pipeline_output_receipt_has_correct_backend_id() {
    let mut rt = Runtime::new();
    rt.register_backend("alpha", TransientFailBackend::new("alpha"));
    rt.register_backend("beta", SuccessBackend::new("beta"));

    let config = ExecutionConfig {
        retry_policy: None,
        fallback_chain: Some(FallbackChain::new(vec!["beta".into()])),
    };
    let pipeline = ExecutionPipeline::new(config);
    let output = pipeline
        .execute(&rt, "alpha", mock_work_order())
        .await
        .unwrap();

    assert_eq!(output.receipt.backend.id, "beta");
}

// 18. Config accessor returns the stored config.
#[test]
fn pipeline_config_accessor() {
    let config = ExecutionConfig {
        retry_policy: Some(RetryPolicy::default()),
        fallback_chain: None,
    };
    let pipeline = ExecutionPipeline::new(config.clone());
    assert!(pipeline.config().retry_policy.is_some());
    assert!(pipeline.config().fallback_chain.is_none());
}
