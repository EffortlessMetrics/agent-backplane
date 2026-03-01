// SPDX-License-Identifier: MIT OR Apache-2.0
//! Additional built-in pipeline stages, builder, and execution helpers.
//!
//! This module provides:
//! - `RateLimitStage` — per-minute throughput limiter
//! - `DeduplicationStage` — duplicate work order rejection
//! - `LoggingStage` — entry/exit tracing
//! - `MetricsStage` — execution statistics
//! - `PipelineBuilder` / `StagePipeline` — ergonomic pipeline assembly and
//!   per-stage result reporting

use crate::pipeline::PipelineStage;
use abp_core::WorkOrder;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info};

// ---------------------------------------------------------------------------
// StageStats
// ---------------------------------------------------------------------------

/// Accumulated execution statistics for a [`MetricsStage`].
#[derive(Debug, Clone, Default)]
pub struct StageStats {
    /// Total number of invocations.
    pub invocations: u64,
    /// Number of successful invocations.
    pub successes: u64,
    /// Number of failed invocations.
    pub failures: u64,
    /// Sum of all invocation durations in milliseconds.
    pub total_duration_ms: u64,
}

// ---------------------------------------------------------------------------
// StageResult
// ---------------------------------------------------------------------------

/// Outcome of executing a single stage inside a [`StagePipeline`].
#[derive(Debug, Clone)]
pub struct StageResult {
    /// Name of the stage (from [`PipelineStage::name`]).
    pub stage_name: String,
    /// Whether the stage completed without error.
    pub passed: bool,
    /// Wall-clock time spent in this stage (milliseconds).
    pub duration_ms: u64,
    /// Optional human-readable message (error text on failure).
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// RateLimitStage
// ---------------------------------------------------------------------------

/// Limits how many work orders may be processed per minute.
///
/// Each call to [`process`](PipelineStage::process) records a timestamp; if
/// the number of timestamps within the last 60 seconds exceeds
/// `max_per_minute`, the stage returns an error.
pub struct RateLimitStage {
    max_per_minute: u32,
    timestamps: Arc<Mutex<Vec<Instant>>>,
}

impl RateLimitStage {
    /// Create a new rate limiter allowing `max_per_minute` runs per 60-second window.
    #[must_use]
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            max_per_minute,
            timestamps: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl PipelineStage for RateLimitStage {
    async fn process(&self, _order: &mut WorkOrder) -> Result<()> {
        let now = Instant::now();
        let window = Duration::from_secs(60);
        let mut ts = self.timestamps.lock().await;
        // Evict entries older than 60 s.
        ts.retain(|t| now.duration_since(*t) < window);
        if ts.len() as u32 >= self.max_per_minute {
            anyhow::bail!(
                "rate limit exceeded: {} runs in the last 60 s (max {})",
                ts.len(),
                self.max_per_minute
            );
        }
        ts.push(now);
        Ok(())
    }

    fn name(&self) -> &str {
        "rate_limit"
    }
}

// ---------------------------------------------------------------------------
// DeduplicationStage
// ---------------------------------------------------------------------------

/// Prevents duplicate work orders from executing within a configurable window.
///
/// Duplicates are detected by hashing the canonical JSON representation of the
/// work order.  If the same hash appears within `window`, the stage rejects
/// the order.
pub struct DeduplicationStage {
    window: Duration,
    seen: Arc<Mutex<HashMap<String, Instant>>>,
}

impl DeduplicationStage {
    /// Create a new deduplication stage with the given time window.
    #[must_use]
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            seen: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Derive a deduplication key from a work order.
    ///
    /// Uses the canonical JSON serialization of the order so that structurally
    /// identical orders (possibly with different `id`s) are still detected.
    fn dedup_key(order: &WorkOrder) -> String {
        // Hash task + workspace root + config as a lightweight fingerprint.
        format!(
            "{}:{}:{}",
            order.task,
            order.workspace.root,
            serde_json::to_string(&order.config).unwrap_or_default()
        )
    }
}

#[async_trait]
impl PipelineStage for DeduplicationStage {
    async fn process(&self, order: &mut WorkOrder) -> Result<()> {
        let key = Self::dedup_key(order);
        let now = Instant::now();
        let mut seen = self.seen.lock().await;
        // Evict expired entries.
        seen.retain(|_, ts| now.duration_since(*ts) < self.window);
        if seen.contains_key(&key) {
            anyhow::bail!("duplicate work order detected within deduplication window");
        }
        seen.insert(key, now);
        Ok(())
    }

    fn name(&self) -> &str {
        "deduplication"
    }
}

// ---------------------------------------------------------------------------
// LoggingStage
// ---------------------------------------------------------------------------

/// Logs work order entry with a configurable prefix.
///
/// Records the work order `id` and `task` at `info` level before passing
/// through.
pub struct LoggingStage {
    prefix: String,
}

impl LoggingStage {
    /// Create a new logging stage with the given log-line prefix.
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
        }
    }
}

#[async_trait]
impl PipelineStage for LoggingStage {
    async fn process(&self, order: &mut WorkOrder) -> Result<()> {
        info!(
            target: "abp.pipeline",
            prefix = %self.prefix,
            id = %order.id,
            task = %order.task,
            "{}: processing work order id={} task={}",
            self.prefix,
            order.id,
            order.task,
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "logging"
    }
}

// ---------------------------------------------------------------------------
// MetricsStage
// ---------------------------------------------------------------------------

/// Tracks execution metrics (timing, counts, outcomes).
///
/// Because this stage only measures its own `process` call it must wrap
/// another stage to be useful — or, more commonly, it is placed in a
/// [`StagePipeline`] where each stage is timed externally.  On its own the
/// stage always succeeds and records a success.
pub struct MetricsStage {
    stats: Arc<Mutex<StageStats>>,
}

impl MetricsStage {
    /// Create a new metrics stage with zeroed counters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stats: Arc::new(Mutex::new(StageStats::default())),
        }
    }

    /// Return a snapshot of the current statistics.
    pub async fn stats(&self) -> StageStats {
        self.stats.lock().await.clone()
    }

    /// Record an external observation (used by [`StagePipeline`]).
    pub(crate) async fn record(&self, duration_ms: u64, success: bool) {
        let mut s = self.stats.lock().await;
        s.invocations += 1;
        s.total_duration_ms += duration_ms;
        if success {
            s.successes += 1;
        } else {
            s.failures += 1;
        }
    }
}

impl Default for MetricsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PipelineStage for MetricsStage {
    async fn process(&self, _order: &mut WorkOrder) -> Result<()> {
        let start = Instant::now();
        // The stage itself is a passthrough — metrics are recorded.
        let duration_ms = start.elapsed().as_millis() as u64;
        self.record(duration_ms, true).await;
        Ok(())
    }

    fn name(&self) -> &str {
        "metrics"
    }
}

// ---------------------------------------------------------------------------
// PipelineBuilder / StagePipeline
// ---------------------------------------------------------------------------

/// Ergonomic builder for a [`StagePipeline`].
///
/// ```
/// use abp_runtime::stages::PipelineBuilder;
/// use abp_runtime::pipeline::ValidationStage;
///
/// let pipeline = PipelineBuilder::new()
///     .add_stage(Box::new(ValidationStage))
///     .build();
/// assert_eq!(pipeline.stage_names().len(), 1);
/// ```
pub struct PipelineBuilder {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl PipelineBuilder {
    /// Create an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Append a boxed stage to the pipeline.
    #[must_use]
    pub fn add_stage(mut self, stage: Box<dyn PipelineStage>) -> Self {
        self.stages.push(stage);
        self
    }

    /// Return the number of stages added so far.
    #[must_use]
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Consume the builder and produce a [`StagePipeline`].
    #[must_use]
    pub fn build(self) -> StagePipeline {
        StagePipeline {
            stages: self.stages,
        }
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// An ordered chain of [`PipelineStage`]s that reports per-stage results.
///
/// Unlike the core [`Pipeline`](crate::pipeline::Pipeline), `StagePipeline`
/// records a [`StageResult`] for every stage (including timing) and does
/// **not** short-circuit on failure — all stages run unconditionally so that
/// callers receive a complete diagnostic picture.
pub struct StagePipeline {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl StagePipeline {
    /// Execute all stages against the given work order, returning a result
    /// vector with one entry per stage.
    pub async fn execute(&self, wo: &mut WorkOrder) -> Vec<StageResult> {
        let mut results = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            let start = Instant::now();
            let outcome = stage.process(wo).await;
            let duration_ms = start.elapsed().as_millis() as u64;
            let (passed, message) = match outcome {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e.to_string())),
            };
            debug!(
                target: "abp.pipeline",
                stage = %stage.name(),
                passed,
                duration_ms,
                "stage result"
            );
            results.push(StageResult {
                stage_name: stage.name().to_string(),
                passed,
                duration_ms,
                message,
            });
        }
        results
    }

    /// Return the names of all stages in insertion order.
    #[must_use]
    pub fn stage_names(&self) -> Vec<&str> {
        self.stages.iter().map(|s| s.name()).collect()
    }
}
