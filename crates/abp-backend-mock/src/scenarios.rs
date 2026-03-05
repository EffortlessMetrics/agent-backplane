//! Configurable mock backend scenarios for testing various failure and success patterns.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use abp_backend_core::{ensure_capability_requirements, extract_execution_mode, Backend};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport, WorkOrder, CONTRACT_VERSION,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, Mutex};
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
    /// A fully custom event sequence built via [`EventSequenceBuilder`].
    Custom {
        /// Ordered steps to execute.
        steps: Vec<EventStep>,
        /// Token usage to report in the receipt.
        usage: Option<UsageNormalized>,
        /// The outcome to set on the receipt.
        outcome: Outcome,
        /// If set, emit events up to this point then fail with this error.
        fail_after: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// EventStep
// ---------------------------------------------------------------------------

/// A single step in a custom event sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStep {
    /// The event to emit.
    pub kind: AgentEventKind,
    /// Milliseconds to sleep before emitting this event.
    pub delay_before_ms: u64,
}

// ---------------------------------------------------------------------------
// EventSequenceBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing custom [`MockScenario::Custom`] sequences.
///
/// Provides a fluent API for assembling arbitrary event sequences with
/// per-event latency, tool call/result simulation, error injection, and
/// configurable token usage.
///
/// # Example
/// ```
/// use abp_backend_mock::scenarios::EventSequenceBuilder;
///
/// let scenario = EventSequenceBuilder::new()
///     .message("Hello")
///     .delay_ms(50)
///     .tool_call("read_file", serde_json::json!({"path": "foo.txt"}))
///     .tool_result("read_file", serde_json::json!({"content": "bar"}))
///     .message("Done")
///     .usage_tokens(100, 50)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct EventSequenceBuilder {
    steps: Vec<EventStep>,
    next_delay_ms: u64,
    usage: Option<UsageNormalized>,
    outcome: Outcome,
    fail_after: Option<String>,
}

impl Default for EventSequenceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSequenceBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            next_delay_ms: 0,
            usage: None,
            outcome: Outcome::Complete,
            fail_after: None,
        }
    }

    /// Set delay (ms) to apply before the *next* added event.
    pub fn delay_ms(mut self, ms: u64) -> Self {
        self.next_delay_ms = ms;
        self
    }

    /// Emit an `AssistantMessage` event.
    pub fn message(mut self, text: impl Into<String>) -> Self {
        self.push(AgentEventKind::AssistantMessage { text: text.into() });
        self
    }

    /// Emit an `AssistantDelta` (streaming chunk) event.
    pub fn delta(mut self, text: impl Into<String>) -> Self {
        self.push(AgentEventKind::AssistantDelta { text: text.into() });
        self
    }

    /// Emit a `ToolCall` event.
    pub fn tool_call(self, name: impl Into<String>, input: serde_json::Value) -> Self {
        self.tool_call_full(name, None, None, input)
    }

    /// Emit a `ToolCall` event with explicit IDs.
    pub fn tool_call_full(
        mut self,
        name: impl Into<String>,
        tool_use_id: Option<String>,
        parent_tool_use_id: Option<String>,
        input: serde_json::Value,
    ) -> Self {
        self.push(AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id,
            parent_tool_use_id,
            input,
        });
        self
    }

    /// Emit a successful `ToolResult` event.
    pub fn tool_result(mut self, name: impl Into<String>, output: serde_json::Value) -> Self {
        self.push(AgentEventKind::ToolResult {
            tool_name: name.into(),
            tool_use_id: None,
            output,
            is_error: false,
        });
        self
    }

    /// Emit a failed `ToolResult` event.
    pub fn tool_error(mut self, name: impl Into<String>, output: serde_json::Value) -> Self {
        self.push(AgentEventKind::ToolResult {
            tool_name: name.into(),
            tool_use_id: None,
            output,
            is_error: true,
        });
        self
    }

    /// Emit a `FileChanged` event.
    pub fn file_changed(mut self, path: impl Into<String>, summary: impl Into<String>) -> Self {
        self.push(AgentEventKind::FileChanged {
            path: path.into(),
            summary: summary.into(),
        });
        self
    }

    /// Emit a `CommandExecuted` event.
    pub fn command_executed(
        mut self,
        command: impl Into<String>,
        exit_code: i32,
        output_preview: Option<String>,
    ) -> Self {
        self.push(AgentEventKind::CommandExecuted {
            command: command.into(),
            exit_code: Some(exit_code),
            output_preview,
        });
        self
    }

    /// Emit a `Warning` event.
    pub fn warning(mut self, message: impl Into<String>) -> Self {
        self.push(AgentEventKind::Warning {
            message: message.into(),
        });
        self
    }

    /// Emit an `Error` event (does **not** cause the run to fail; use
    /// [`fail_after`](Self::fail_after) for that).
    pub fn error_event(mut self, message: impl Into<String>) -> Self {
        self.push(AgentEventKind::Error {
            message: message.into(),
            error_code: None,
        });
        self
    }

    /// Configure token usage reported in the receipt.
    pub fn usage(mut self, usage: UsageNormalized) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Shorthand: set input and output token counts.
    pub fn usage_tokens(mut self, input: u64, output: u64) -> Self {
        self.usage = Some(UsageNormalized {
            input_tokens: Some(input),
            output_tokens: Some(output),
            ..Default::default()
        });
        self
    }

    /// Set the receipt outcome (defaults to `Complete`).
    pub fn outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// After emitting all events, fail with this error message instead of
    /// returning a receipt. Simulates mid-stream crashes.
    pub fn fail_after(mut self, message: impl Into<String>) -> Self {
        self.fail_after = Some(message.into());
        self
    }

    /// Build the final [`MockScenario::Custom`].
    pub fn build(self) -> MockScenario {
        MockScenario::Custom {
            steps: self.steps,
            usage: self.usage,
            outcome: self.outcome,
            fail_after: self.fail_after,
        }
    }

    fn push(&mut self, kind: AgentEventKind) {
        self.steps.push(EventStep {
            kind,
            delay_before_ms: self.next_delay_ms,
        });
        self.next_delay_ms = 0;
    }
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
                MockScenario::Custom {
                    steps,
                    usage,
                    outcome,
                    fail_after,
                } => {
                    self.run_custom(
                        run_id,
                        work_order,
                        events_tx,
                        steps,
                        usage.as_ref(),
                        outcome,
                        fail_after.as_deref(),
                    )
                    .await
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
            None,
            None,
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
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_custom(
        &self,
        run_id: Uuid,
        work_order: &WorkOrder,
        events_tx: &mpsc::Sender<AgentEvent>,
        steps: &[EventStep],
        usage: Option<&UsageNormalized>,
        outcome: &Outcome,
        fail_after: Option<&str>,
    ) -> Result<Receipt> {
        ensure_capability_requirements(&work_order.requirements, &self.capabilities())
            .context("capability requirements not satisfied")?;

        let started = Utc::now();
        let mut trace = Vec::new();

        emit(
            &mut trace,
            events_tx,
            AgentEventKind::RunStarted {
                message: format!("scenario-mock custom: {}", work_order.task),
            },
        )
        .await;

        for step in steps {
            if step.delay_before_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(step.delay_before_ms)).await;
            }
            emit(&mut trace, events_tx, step.kind.clone()).await;
        }

        if let Some(err_msg) = fail_after {
            anyhow::bail!("{}", err_msg);
        }

        emit(
            &mut trace,
            events_tx,
            AgentEventKind::RunCompleted {
                message: "scenario-mock custom complete".into(),
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
            usage,
            Some(outcome),
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

    /// Assert that exactly `n` calls were recorded. Panics with a descriptive
    /// message on mismatch.
    pub async fn assert_call_count(&self, expected: usize) {
        let actual = self.calls.lock().await.len();
        assert_eq!(
            actual, expected,
            "expected {expected} recorded calls, got {actual}"
        );
    }

    /// Assert that all recorded calls succeeded (returned `Ok`).
    pub async fn assert_all_succeeded(&self) {
        for (i, call) in self.calls.lock().await.iter().enumerate() {
            assert!(
                call.result.is_ok(),
                "call {i} failed: {:?}",
                call.result.as_ref().unwrap_err()
            );
        }
    }

    /// Assert that all recorded calls failed (returned `Err`).
    pub async fn assert_all_failed(&self) {
        for (i, call) in self.calls.lock().await.iter().enumerate() {
            assert!(call.result.is_err(), "call {i} unexpectedly succeeded");
        }
    }

    /// Return recorded calls filtered by task substring.
    pub async fn calls_matching(&self, task_substring: &str) -> Vec<RecordedCall> {
        self.calls
            .lock()
            .await
            .iter()
            .filter(|c| c.work_order.task.contains(task_substring))
            .cloned()
            .collect()
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

#[allow(clippy::too_many_arguments)]
fn build_receipt(
    run_id: Uuid,
    work_order: &WorkOrder,
    identity: &BackendIdentity,
    capabilities: &CapabilityManifest,
    started: DateTime<Utc>,
    trace: Vec<AgentEvent>,
    usage: Option<&UsageNormalized>,
    outcome: Option<&Outcome>,
) -> Result<Receipt> {
    let finished = Utc::now();
    let duration_ms = (finished - started)
        .to_std()
        .unwrap_or_default()
        .as_millis() as u64;
    let mode = extract_execution_mode(work_order);

    let default_usage = UsageNormalized {
        input_tokens: Some(0),
        output_tokens: Some(0),
        estimated_cost_usd: Some(0.0),
        ..Default::default()
    };

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
        usage: usage.cloned().unwrap_or(default_usage),
        trace,
        artifacts: vec![],
        verification: VerificationReport {
            git_diff: None,
            git_status: None,
            harness_ok: true,
        },
        outcome: outcome.cloned().unwrap_or(Outcome::Complete),
        receipt_sha256: None,
    }
    .with_hash()?;

    Ok(receipt)
}
