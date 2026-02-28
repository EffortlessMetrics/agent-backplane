// SPDX-License-Identifier: MIT OR Apache-2.0
//! Processing pipeline for work order pre-processing.
//!
//! A [`Pipeline`] chains zero or more [`PipelineStage`] implementations that
//! inspect and optionally mutate a [`WorkOrder`] before it reaches a backend.
//! Stages run in insertion order; any failure short-circuits the remaining
//! stages.

use abp_core::WorkOrder;
use abp_policy::PolicyEngine;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
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
        anyhow::ensure!(!order.task.trim().is_empty(), "work order task must not be empty");
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
/// Compiles the [`PolicyEngine`] from the order's [`PolicyProfile`] and
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
