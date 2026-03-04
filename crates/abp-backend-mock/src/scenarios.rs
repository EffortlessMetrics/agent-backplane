//! Configurable mock backend scenarios for testing various failure and success patterns.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use abp_backend_core::{Backend, ensure_capability_requirements, extract_execution_mode};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest, Outcome,
    Receipt, RunMetadata, UsageNormalized, VerificationReport, WorkOrder,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::MockBackend;

// ---------------------------------------------------------------------------
// MockScenario
// ---------------------------------------------------------------------------

/// Describes the behaviour a [`ScenarioMockBackend`] should exhibit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MockScenario {
    /// Succeed after an optional delay, returning `text` as the assistant message.
    Success {
        /// Milliseconds to sleep before completing.
        delay_ms: u64,
        /// Text returned as the assistant message.
        text: String,
    },
    /// Stream `chunks` one at a time with a delay between each.
    StreamingSuccess {
        /// Ordered text chunks to emit as `AssistantDelta` events.
        chunks: Vec<String>,
        /// Milliseconds to sleep between each chunk.
        chunk_delay_ms: u64,
    },
    /// Fail the first `fail_count` invocations, then behave according to `then`.
    TransientError {
        /// How many times to fail before succeeding.
        fail_count: usize,
        /// The scenario to follow once all transient failures are exhausted.
        then: Box<MockScenario>,
    },
    /// Always fail with the given error code and message.
    PermanentError {
        /// An application-level error code string (e.g. `"ABP-B001"`).
        code: String,
        /// Human-readable error description.
        message: String,
    },
    /// Simulate a timeout by sleeping for `after_ms` and then returning an error.
    Timeout {
        /// Milliseconds to sleep before returning a timeout error.
        after_ms: u64,
    },
    /// Simulate rate-limiting by failing with a retry hint.
    RateLimited {
        /// Suggested retry-after duration in milliseconds.
        retry_after_ms: u64,
    },
}

// ---------------------------------------------------------------------------
// RecordedCall
// ---------------------------------------------------------------------------

/// A snapshot of a single call made through a scenario or recorder backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedCall {
    /// The work order that was submitted.
    pub work_order: WorkOrder,
    /// Timestamp when the call started.
    pub timestamp: DateTime<Utc>,
    /// Wall-clock duration of the call in milliseconds.
    pub duration_ms: u64,
    /// `Ok(outcome)` if the run produced a receipt, `Err(message)` otherwise.
    pub result: std::result::Result<Outcome, String>,
}

// ---------------------------------------------------------------------------
// ScenarioMockBackend
// ---------------------------------------------------------------------------

/// A mock backend that follows a configurable [`MockScenario`].
///
/// It delegates identity and capabilities to the underlying [`MockBackend`]
/// and adds scenario-driven behaviour on top.
#[derive(Debug)]
pub struct ScenarioMockBackend {
    inner: MockBackend,
    scenario: MockScenario,
    call_count: AtomicUsize,
    last_error: Arc<Mutex<Option<String>>>,
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl Clone for ScenarioMockBackend {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            scenario: self.scenario.clone(),
            call_count: AtomicUsize::new(self.call_count.load(Ordering::SeqCst)),
            last_error: Arc::clone(&self.last_error),
            calls: Arc::clone(&self.calls),
        }
    }
}

impl ScenarioMockBackend {
    /// Create a new scenario-driven mock backend.
    pub fn new(scenario: MockScenario) -> Self {
        Self {
            inner: MockBackend,
            scenario,
            call_count: AtomicUsize::new(0),
            last_error: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Total number of `run` invocations so far.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    /// The last error message, if any.
    pub async fn last_error(&self) -> Option<String> {
        self.last_error.lock().await.clone()
    }

    /// All recorded calls.
    pub async fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().await.clone()
    }

    /// The most recent recorded call, if any.
    pub async fn last_call(&self) -> Option<RecordedCall> {
        self.calls.lock().await.last().cloned()
    }

    // -- internal helpers ---------------------------------------------------

    async fn record(
        &self,
        wo: &WorkOrder,
        start: Instant,
        result: &std::result::Result<Outcome, String>,
    ) {
        let duration_ms = start.elapsed().as_millis() as u64;
        self.calls.lock().await.push(RecordedCall {
            work_order: wo.clone(),
            timestamp: Utc::now(),
            duration_ms,
            result: result.clone(),
        });
    }

    async fn set_last_error(&self, msg: &str) {
        *self.last_error.lock().await = Some(msg.to_string());
    }
}

#[async_trait]
impl Backend for ScenarioMockBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "scenario-mock".to_string(),
            backend_version: Some("0.1".to_string()),
            adapter_version: Some("0.1".to_string()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        self.inner.capabilities()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let invocation = self.call_count.fetch_add(1, Ordering::SeqCst);
        let start = Instant::now();

        let res = self
            .execute_scenario(&self.scenario, invocation, run_id, &work_order, &events_tx)
            .await;

        let outcome = match &res {
            Ok(r) => Ok(r.outcome.clone()),
            Err(e) => Err(e.to_string()),
        };
        self.record(&work_order, start, &outcome).await;

        if let Err(ref e) = res {
            self.set_last_error(&e.to_string()).await;
        }

        res
    }
}

impl ScenarioMockBackend {
    fn execute_scenario<'a>(
        &'a self,
        scenario: &'a MockScenario,
        invocation: usize,
        run_id: Uuid,
        work_order: &'a WorkOrder,
        events_tx: &'a mpsc::Sender<AgentEvent>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Receipt>> + Send + 'a>> {
        Box::pin(async move {
            match scenario {
                MockScenario::Success { delay_ms, text } => {
                    self.run_success(run_id, work_order, events_tx, *delay_ms, text)
                        .await
                }
                MockScenario::StreamingSuccess {
                    chunks,
                    chunk_delay_ms,
                } => {
                    self.run_streaming(run_id, work_order, events_tx, chunks, *chunk_delay_ms)
                        .await
                }
                MockScenario::TransientError { fail_count, then } => {
                    if invocation < *fail_count {
                        anyhow::bail!(
                            "transient error (attempt {}/{})",
                            invocation + 1,
                            fail_count
                        );
                    }
                    self.execute_scenario(then, invocation, run_id, work_order, events_tx)
                        .await
                }
                MockScenario::PermanentError { code, message } => {
                    anyhow::bail!("[{}] {}", code, message);
                }
                MockScenario::Timeout { after_ms } => {
                    tokio::time::sleep(std::time::Duration::from_millis(*after_ms)).await;
                    anyhow::bail!("backend timeout after {}ms", after_ms);
                }
                MockScenario::RateLimited { retry_after_ms } => {
                    anyhow::bail!("rate limited: retry after {}ms", retry_after_ms);
                }
            }
        })
    }

    async fn run_success(
        &self,
        run_id: Uuid,
        work_order: &WorkOrder,
        events_tx: &mpsc::Sender<AgentEvent>,
        delay_ms: u64,
        text: &str,
    ) -> Result<Receipt> {
        ensure_capability_requirements(&work_order.requirements, &self.capabilities())
            .context("capability requirements not satisfied")?;

        let started = Utc::now();
        let mut trace = Vec::new();

        emit(
            &mut trace,
            events_tx,
            AgentEventKind::RunStarted {
                message: format!("scenario-mock starting: {}", work_order.task),
            },
        )
        .await;

        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }

        emit(
            &mut trace,
            events_tx,
            AgentEventKind::AssistantMessage {
                text: text.to_string(),
            },
        )
        .await;

        emit(
            &mut trace,
            events_tx,
            AgentEventKind::RunCompleted {
                message: "scenario-mock run complete".into(),
            },
        )
        .await;

        build_receipt(
            run_id,
            work_order,
            &self.identity(),
            &self.capabilities(),
            started,
            trace,
        )
    }

    async fn run_streaming(
        &self,
        run_id: Uuid,
        work_order: &WorkOrder,
        events_tx: &mpsc::Sender<AgentEvent>,
        chunks: &[String],
        chunk_delay_ms: u64,
    ) -> Result<Receipt> {
        ensure_capability_requirements(&work_order.requirements, &self.capabilities())
            .context("capability requirements not satisfied")?;

        let started = Utc::now();
        let mut trace = Vec::new();

        emit(
            &mut trace,
            events_tx,
            AgentEventKind::RunStarted {
                message: format!("scenario-mock streaming: {}", work_order.task),
            },
        )
        .await;

        for chunk in chunks {
            if chunk_delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(chunk_delay_ms)).await;
            }
            emit(
                &mut trace,
                events_tx,
                AgentEventKind::AssistantDelta {
                    text: chunk.clone(),
                },
            )
            .await;
        }

        emit(
            &mut trace,
            events_tx,
            AgentEventKind::RunCompleted {
                message: "scenario-mock streaming complete".into(),
            },
        )
        .await;

        build_receipt(
            run_id,
            work_order,
            &self.identity(),
            &self.capabilities(),
            started,
            trace,
        )
    }
}

// ---------------------------------------------------------------------------
// MockBackendRecorder
// ---------------------------------------------------------------------------

/// A recording wrapper around any [`Backend`] that captures every call.
#[derive(Debug)]
pub struct MockBackendRecorder<B: Backend> {
    inner: B,
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl<B: Backend + Clone> Clone for MockBackendRecorder<B> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            calls: Arc::clone(&self.calls),
        }
    }
}

impl<B: Backend> MockBackendRecorder<B> {
    /// Wrap an existing backend with call recording.
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return all recorded calls.
    pub async fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().await.clone()
    }

    /// Total number of recorded calls.
    pub async fn call_count(&self) -> usize {
        self.calls.lock().await.len()
    }

    /// The most recent recorded call, if any.
    pub async fn last_call(&self) -> Option<RecordedCall> {
        self.calls.lock().await.last().cloned()
    }
}

#[async_trait]
impl<B: Backend + Send + Sync> Backend for MockBackendRecorder<B> {
    fn identity(&self) -> BackendIdentity {
        self.inner.identity()
    }

    fn capabilities(&self) -> CapabilityManifest {
        self.inner.capabilities()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let start = Instant::now();
        let wo_snapshot = work_order.clone();

        let res = self.inner.run(run_id, work_order, events_tx).await;

        let duration_ms = start.elapsed().as_millis() as u64;
        let result = match &res {
            Ok(r) => Ok(r.outcome.clone()),
            Err(e) => Err(e.to_string()),
        };

        self.calls.lock().await.push(RecordedCall {
            work_order: wo_snapshot,
            timestamp: Utc::now(),
            duration_ms,
            result,
        });

        res
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn emit(trace: &mut Vec<AgentEvent>, tx: &mpsc::Sender<AgentEvent>, kind: AgentEventKind) {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    };
    trace.push(ev.clone());
    let _ = tx.send(ev).await;
}

fn build_receipt(
    run_id: Uuid,
    work_order: &WorkOrder,
    identity: &BackendIdentity,
    capabilities: &CapabilityManifest,
    started: DateTime<Utc>,
    trace: Vec<AgentEvent>,
) -> Result<Receipt> {
    let finished = Utc::now();
    let duration_ms = (finished - started)
        .to_std()
        .unwrap_or_default()
        .as_millis() as u64;
    let mode = extract_execution_mode(work_order);

    let receipt = Receipt {
        meta: RunMetadata {
            run_id,
            work_order_id: work_order.id,
            contract_version: CONTRACT_VERSION.to_string(),
            started_at: started,
            finished_at: finished,
            duration_ms,
        },
        backend: identity.clone(),
        capabilities: capabilities.clone(),
        mode,
        usage_raw: json!({"note": "scenario-mock"}),
        usage: UsageNormalized {
            input_tokens: Some(0),
            output_tokens: Some(0),
            estimated_cost_usd: Some(0.0),
            ..Default::default()
        },
        trace,
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: true,
        },
        outcome: Outcome::Complete,
        receipt_sha256: None,
    }
    .with_hash()?;

    Ok(receipt)
}
