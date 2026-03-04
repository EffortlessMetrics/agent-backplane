// SPDX-License-Identifier: MIT OR Apache-2.0
//! Processing pipeline for work order pre-processing.
//!
//! A `Pipeline` chains zero or more `PipelineStage` implementations that
//! inspect and optionally mutate a [`WorkOrder`] before it reaches a backend.
//! Stages run in insertion order; any failure short-circuits the remaining
//! stages.

use abp_core::{AgentEvent, Receipt, WorkOrder};
use abp_integrations::{Backend, ensure_capability_requirements};
use abp_policy::PolicyEngine;
use abp_receipt::ReceiptBuilder;
use abp_workspace::WorkspaceManager;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, mpsc};
use tracing::debug;

// ---------------------------------------------------------------------------
// Core trait
// ---------------------------------------------------------------------------

/// A single processing stage applied to a work order before execution.
#[async_trait]
pub trait PipelineStage: Send + Sync {
    /// Process (and optionally mutate) the work order.
    ///
    /// Return `Ok(())` to continue to the next stage, or `Err` to
    /// short-circuit the pipeline.
    async fn process(&self, order: &mut WorkOrder) -> Result<()>;

    /// Human-readable name used in tracing/audit output.
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Built-in stages
// ---------------------------------------------------------------------------

/// Validates required work order fields.
///
/// Rejects orders with an empty `task` or empty workspace `root`.
pub struct ValidationStage;

#[async_trait]
impl PipelineStage for ValidationStage {
    async fn process(&self, order: &mut WorkOrder) -> Result<()> {
        anyhow::ensure!(
            !order.task.trim().is_empty(),
            "work order task must not be empty"
        );
        anyhow::ensure!(
            !order.workspace.root.trim().is_empty(),
            "work order workspace root must not be empty"
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "validation"
    }
}

/// Checks the work order's policy for disallowed tools.
///
/// Compiles the [`PolicyEngine`] from the order's `PolicyProfile` and
/// rejects the order if any tool in `allowed_tools` is simultaneously
/// present in `disallowed_tools`.
pub struct PolicyStage;

#[async_trait]
impl PipelineStage for PolicyStage {
    async fn process(&self, order: &mut WorkOrder) -> Result<()> {
        let engine = PolicyEngine::new(&order.policy)?;

        // Check every tool in the allow-list against the deny-list.
        for tool in &order.policy.allowed_tools {
            let decision = engine.can_use_tool(tool);
            anyhow::ensure!(
                decision.allowed,
                "policy blocks tool `{}`: {}",
                tool,
                decision.reason.as_deref().unwrap_or("denied by policy")
            );
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "policy"
    }
}

/// Records work order processing for audit purposes.
///
/// Stores a log of processed work order ids that can be inspected after
/// pipeline execution.
pub struct AuditStage {
    log: Arc<Mutex<Vec<AuditEntry>>>,
}

/// A single audit log entry.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    /// Work order id.
    pub work_order_id: uuid::Uuid,
    /// Task description at the time of processing.
    pub task: String,
}

impl AuditStage {
    /// Create a new audit stage with an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return a snapshot of the audit log.
    pub async fn entries(&self) -> Vec<AuditEntry> {
        self.log.lock().await.clone()
    }
}

impl Default for AuditStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for AuditStage {
    async fn process(&self, order: &mut WorkOrder) -> Result<()> {
        debug!(target: "abp.pipeline", id=%order.id, task=%order.task, "audit");
        self.log.lock().await.push(AuditEntry {
            work_order_id: order.id,
            task: order.task.clone(),
        });
        Ok(())
    }

    fn name(&self) -> &str {
        "audit"
    }
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// An ordered chain of [`PipelineStage`]s executed sequentially.
///
/// ```
/// use abp_runtime::pipeline::{Pipeline, ValidationStage, AuditStage};
///
/// let pipeline = Pipeline::new()
///     .stage(ValidationStage)
///     .stage(AuditStage::new());
/// ```
pub struct Pipeline {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    /// Create an empty pipeline with no stages.
    #[must_use]
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Append a stage to the pipeline (builder pattern).
    #[must_use]
    pub fn stage<S: PipelineStage + 'static>(mut self, stage: S) -> Self {
        self.stages.push(Box::new(stage));
        self
    }

    /// Execute all stages in order against the given work order.
    ///
    /// Returns `Ok(())` when every stage succeeds, or the first `Err`
    /// encountered (short-circuiting remaining stages).
    pub async fn execute(&self, order: &mut WorkOrder) -> Result<()> {
        for stage in &self.stages {
            debug!(target: "abp.pipeline", stage=%stage.name(), "executing");
            stage.process(order).await?;
        }
        Ok(())
    }

    /// Return the number of stages in the pipeline.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// Return `true` if the pipeline contains no stages.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }
}

// ---------------------------------------------------------------------------
// RuntimePipeline — full work-order orchestration
// ---------------------------------------------------------------------------

/// Result of a single stage in the [`RuntimePipeline`].
#[derive(Debug, Clone)]
pub struct StageOutcome {
    /// Name of the stage.
    pub name: String,
    /// Whether the stage succeeded.
    pub success: bool,
    /// Wall-clock time for the stage in milliseconds.
    pub duration_ms: u64,
    /// Error message on failure.
    pub error: Option<String>,
}

/// Orchestrates a complete work-order execution through discrete stages:
///
/// 1. **validate_policy** — compile the policy engine and check for conflicts
/// 2. **negotiate_capabilities** — verify the backend satisfies requirements
/// 3. **select_backend** — resolve the backend from the registry
/// 4. **prepare_workspace** — stage the workspace directory
/// 5. **run_backend** — execute the backend and collect events
/// 6. **collect_events** — drain remaining events from the channel
/// 7. **produce_receipt** — assemble and hash the final receipt
///
/// Each stage is exposed as a separate method for testability and composability.
pub struct RuntimePipeline {
    backend: Arc<dyn Backend>,
    backend_name: String,
}

impl RuntimePipeline {
    /// Create a pipeline for the given backend.
    #[must_use]
    pub fn new(backend_name: impl Into<String>, backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            backend_name: backend_name.into(),
        }
    }

    /// Return the backend name.
    #[must_use]
    pub fn backend_name(&self) -> &str {
        &self.backend_name
    }

    // -- Stage 1 --

    /// Validate the work order's policy profile.
    ///
    /// Compiles the [`PolicyEngine`] and checks that allowed tools are not
    /// simultaneously denied.
    pub fn validate_policy(&self, order: &WorkOrder) -> Result<()> {
        let engine = PolicyEngine::new(&order.policy)
            .context("compile policy")?;
        for tool in &order.policy.allowed_tools {
            let d = engine.can_use_tool(tool);
            anyhow::ensure!(
                d.allowed,
                "policy blocks tool `{}`: {}",
                tool,
                d.reason.as_deref().unwrap_or("denied")
            );
        }
        Ok(())
    }

    // -- Stage 2 --

    /// Verify the backend satisfies the work order's capability requirements.
    pub fn negotiate_capabilities(&self, order: &WorkOrder) -> Result<()> {
        let caps = self.backend.capabilities();
        if caps.is_empty() {
            // Sidecar backends declare capabilities after handshake; skip.
            return Ok(());
        }
        ensure_capability_requirements(&order.requirements, &caps)
            .context("capability negotiation")?;
        Ok(())
    }

    // -- Stage 3 --

    /// Select and return the backend. In this pipeline the backend is already
    /// bound at construction; this method exists so the stage is explicit.
    #[must_use]
    pub fn select_backend(&self) -> Arc<dyn Backend> {
        Arc::clone(&self.backend)
    }

    // -- Stage 4 --

    /// Prepare the workspace according to the work order spec.
    ///
    /// Returns the effective workspace root path.
    pub fn prepare_workspace(
        &self,
        order: &WorkOrder,
    ) -> Result<abp_workspace::PreparedWorkspace> {
        WorkspaceManager::prepare(&order.workspace).context("prepare workspace")
    }

    // -- Stage 5 --

    /// Run the backend, streaming events to the provided sender.
    ///
    /// Returns the raw receipt from the backend.
    pub async fn run_backend(
        &self,
        run_id: uuid::Uuid,
        order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let backend = self.select_backend();
        debug!(
            target: "abp.runtime.pipeline",
            backend=%self.backend_name, %run_id, "running backend"
        );
        backend.run(run_id, order, events_tx).await
    }

    // -- Stage 6 --

    /// Drain any remaining events from a receiver into a trace vec.
    pub async fn collect_events(
        &self,
        rx: &mut mpsc::Receiver<AgentEvent>,
    ) -> Vec<AgentEvent> {
        let mut trace = Vec::new();
        while let Some(ev) = rx.recv().await {
            trace.push(ev);
        }
        trace
    }

    // -- Stage 7 --

    /// Assemble and hash the final receipt.
    pub fn produce_receipt(
        &self,
        run_id: uuid::Uuid,
        order: &WorkOrder,
        outcome: abp_core::Outcome,
        trace: Vec<AgentEvent>,
    ) -> Result<Receipt> {
        let identity = self.backend.identity();
        let mut receipt = ReceiptBuilder::new(&identity.id)
            .backend_version(identity.backend_version.unwrap_or_default())
            .adapter_version(identity.adapter_version.unwrap_or_default())
            .capabilities(self.backend.capabilities())
            .run_id(run_id)
            .work_order_id(order.id)
            .outcome(outcome)
            .build();

        receipt.trace = trace;

        // Compute receipt hash.
        receipt.receipt_sha256 = Some(
            abp_receipt::compute_hash(&receipt).context("hash receipt")?,
        );

        Ok(receipt)
    }

    // -- Full orchestration --

    /// Execute the full pipeline, returning stage outcomes and the final receipt.
    pub async fn execute(
        &self,
        mut order: WorkOrder,
    ) -> (Vec<StageOutcome>, Result<Receipt>) {
        let mut outcomes = Vec::new();
        let run_id = uuid::Uuid::new_v4();

        // Stage 1: validate policy
        let start = Instant::now();
        let res = self.validate_policy(&order);
        outcomes.push(StageOutcome {
            name: "validate_policy".into(),
            success: res.is_ok(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: res.as_ref().err().map(|e| e.to_string()),
        });
        if let Err(e) = res {
            return (outcomes, Err(e));
        }

        // Stage 2: negotiate capabilities
        let start = Instant::now();
        let res = self.negotiate_capabilities(&order);
        outcomes.push(StageOutcome {
            name: "negotiate_capabilities".into(),
            success: res.is_ok(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: res.as_ref().err().map(|e| e.to_string()),
        });
        if let Err(e) = res {
            return (outcomes, Err(e));
        }

        // Stage 3: select backend
        let start = Instant::now();
        let _backend = self.select_backend();
        outcomes.push(StageOutcome {
            name: "select_backend".into(),
            success: true,
            duration_ms: start.elapsed().as_millis() as u64,
            error: None,
        });

        // Stage 4: prepare workspace
        let start = Instant::now();
        let prepared = self.prepare_workspace(&order);
        let ws_ok = prepared.is_ok();
        outcomes.push(StageOutcome {
            name: "prepare_workspace".into(),
            success: ws_ok,
            duration_ms: start.elapsed().as_millis() as u64,
            error: prepared.as_ref().err().map(|e| e.to_string()),
        });
        let prepared = match prepared {
            Ok(p) => p,
            Err(e) => return (outcomes, Err(e)),
        };

        // Rewrite workspace root to the prepared path.
        order.workspace.root = prepared.path().to_string_lossy().to_string();

        // Stage 5: run backend
        let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);
        let start = Instant::now();
        let backend_result = self.run_backend(run_id, order.clone(), tx).await;
        outcomes.push(StageOutcome {
            name: "run_backend".into(),
            success: backend_result.is_ok(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: backend_result.as_ref().err().map(|e| e.to_string()),
        });

        // Stage 6: collect events
        let start = Instant::now();
        let trace = self.collect_events(&mut rx).await;
        outcomes.push(StageOutcome {
            name: "collect_events".into(),
            success: true,
            duration_ms: start.elapsed().as_millis() as u64,
            error: None,
        });

        // Stage 7: produce receipt
        let start = Instant::now();
        let outcome_val = if backend_result.is_ok() {
            abp_core::Outcome::Complete
        } else {
            abp_core::Outcome::Failed
        };

        let receipt_result = self.produce_receipt(run_id, &order, outcome_val, trace);
        outcomes.push(StageOutcome {
            name: "produce_receipt".into(),
            success: receipt_result.is_ok(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: receipt_result.as_ref().err().map(|e| e.to_string()),
        });

        (outcomes, receipt_result)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::{
        AgentEvent, AgentEventKind, CapabilityRequirements, ContextPacket, ExecutionLane,
        PolicyProfile, WorkOrder, WorkspaceMode, WorkspaceSpec,
    };

    fn sample_work_order() -> WorkOrder {
        WorkOrder {
            id: uuid::Uuid::new_v4(),
            task: "test task".into(),
            lane: ExecutionLane::PatchFirst,
            workspace: WorkspaceSpec {
                root: ".".into(),
                mode: WorkspaceMode::PassThrough,
                include: vec![],
                exclude: vec![],
            },
            context: ContextPacket::default(),
            policy: PolicyProfile::default(),
            requirements: CapabilityRequirements::default(),
            config: abp_core::RuntimeConfig::default(),
        }
    }

    fn make_event() -> AgentEvent {
        AgentEvent {
            ts: chrono::Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "hello".into(),
            },
            ext: None,
        }
    }

    // -- Pipeline (existing) tests --

    #[tokio::test]
    async fn empty_pipeline_succeeds() {
        let p = Pipeline::new();
        let mut wo = sample_work_order();
        p.execute(&mut wo).await.unwrap();
    }

    #[tokio::test]
    async fn validation_stage_rejects_empty_task() {
        let p = Pipeline::new().stage(ValidationStage);
        let mut wo = sample_work_order();
        wo.task = "".into();
        let err = p.execute(&mut wo).await.unwrap_err();
        assert!(err.to_string().contains("task"));
    }

    #[tokio::test]
    async fn validation_stage_rejects_empty_root() {
        let p = Pipeline::new().stage(ValidationStage);
        let mut wo = sample_work_order();
        wo.workspace.root = "".into();
        let err = p.execute(&mut wo).await.unwrap_err();
        assert!(err.to_string().contains("root"));
    }

    #[tokio::test]
    async fn audit_stage_records_entries() {
        let audit = AuditStage::new();
        let p = Pipeline::new().stage(AuditStage {
            log: Arc::clone(&audit.log),
        });
        let mut wo = sample_work_order();
        p.execute(&mut wo).await.unwrap();
        let entries = audit.entries().await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].work_order_id, wo.id);
    }

    #[tokio::test]
    async fn policy_stage_blocks_conflicting_tools() {
        let p = Pipeline::new().stage(PolicyStage);
        let mut wo = sample_work_order();
        wo.policy.allowed_tools = vec!["bash".into()];
        wo.policy.disallowed_tools = vec!["bash".into()];
        let err = p.execute(&mut wo).await.unwrap_err();
        assert!(err.to_string().contains("bash"));
    }

    #[test]
    fn pipeline_len_and_is_empty() {
        let p = Pipeline::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);

        let p = p.stage(ValidationStage);
        assert!(!p.is_empty());
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn pipeline_default_is_empty() {
        let p = Pipeline::default();
        assert!(p.is_empty());
    }

    // -- RuntimePipeline tests --

    #[test]
    fn runtime_pipeline_backend_name() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        assert_eq!(rp.backend_name(), "mock");
    }

    #[test]
    fn runtime_pipeline_validate_policy_passes() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let wo = sample_work_order();
        rp.validate_policy(&wo).unwrap();
    }

    #[test]
    fn runtime_pipeline_validate_policy_rejects_conflict() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let mut wo = sample_work_order();
        wo.policy.allowed_tools = vec!["bash".into()];
        wo.policy.disallowed_tools = vec!["bash".into()];
        let err = rp.validate_policy(&wo).unwrap_err();
        assert!(err.to_string().contains("bash"));
    }

    #[test]
    fn runtime_pipeline_negotiate_capabilities_passes() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let wo = sample_work_order();
        rp.negotiate_capabilities(&wo).unwrap();
    }

    #[test]
    fn runtime_pipeline_select_backend_returns_arc() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let b = rp.select_backend();
        assert_eq!(b.identity().id, "mock");
    }

    #[tokio::test]
    async fn runtime_pipeline_collect_events_empty() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let (_tx, mut rx) = mpsc::channel::<AgentEvent>(16);
        drop(_tx);
        let events = rp.collect_events(&mut rx).await;
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn runtime_pipeline_collect_events_drains() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let (tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        // Send two events then close.
        let ev = make_event();
        tx.send(ev.clone()).await.unwrap();
        tx.send(ev).await.unwrap();
        drop(tx);

        let events = rp.collect_events(&mut rx).await;
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn runtime_pipeline_produce_receipt_has_hash() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let wo = sample_work_order();
        let receipt = rp
            .produce_receipt(uuid::Uuid::new_v4(), &wo, abp_core::Outcome::Complete, vec![])
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }

    #[test]
    fn runtime_pipeline_produce_receipt_includes_trace() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let wo = sample_work_order();
        let events = vec![make_event()];
        let receipt = rp
            .produce_receipt(uuid::Uuid::new_v4(), &wo, abp_core::Outcome::Complete, events)
            .unwrap();
        assert_eq!(receipt.trace.len(), 1);
    }

    #[test]
    fn stage_outcome_fields() {
        let o = StageOutcome {
            name: "test".into(),
            success: true,
            duration_ms: 42,
            error: None,
        };
        assert!(o.success);
        assert_eq!(o.duration_ms, 42);
        assert!(o.error.is_none());
    }

    #[tokio::test]
    async fn runtime_pipeline_execute_returns_all_stages() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let wo = sample_work_order();

        let (outcomes, receipt_result) = rp.execute(wo).await;
        // Should have 7 stages.
        assert_eq!(outcomes.len(), 7);
        assert!(receipt_result.is_ok());

        let names: Vec<&str> = outcomes.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "validate_policy",
                "negotiate_capabilities",
                "select_backend",
                "prepare_workspace",
                "run_backend",
                "collect_events",
                "produce_receipt",
            ]
        );
    }

    #[tokio::test]
    async fn runtime_pipeline_execute_short_circuits_on_policy_failure() {
        let backend = Arc::new(abp_integrations::MockBackend);
        let rp = RuntimePipeline::new("mock", backend);
        let mut wo = sample_work_order();
        wo.policy.allowed_tools = vec!["bash".into()];
        wo.policy.disallowed_tools = vec!["bash".into()];

        let (outcomes, receipt_result) = rp.execute(wo).await;
        // Should stop after validate_policy.
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].success);
        assert!(receipt_result.is_err());
    }
}
