// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lifecycle hooks for runtime extensibility.
//!
//! Register [`LifecycleHook`] implementations with a [`HookRegistry`] to
//! observe and react to work-order lifecycle events (start, event, complete,
//! error) without modifying the core runtime loop.

use abp_core::{AgentEvent, Receipt, WorkOrder};
use std::sync::Arc;

use crate::RuntimeError;
use crate::telemetry::RunMetrics;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Extension point called at well-defined moments in a work-order's lifecycle.
///
/// All methods have default no-op implementations so hooks only need to
/// override the callbacks they care about.
pub trait LifecycleHook {
    /// Called before backend execution begins.
    ///
    /// # Errors
    ///
    /// Returning an error signals that the hook considers the run invalid;
    /// the registry collects all such results for the caller to inspect.
    fn on_run_start(
        &self,
        _work_order: &WorkOrder,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Called for every [`AgentEvent`] emitted during the run.
    ///
    /// # Errors
    ///
    /// An error here is informational — the runtime does not abort the run.
    fn on_event(
        &self,
        _event: &AgentEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Called after the backend returns a [`Receipt`].
    ///
    /// # Errors
    ///
    /// An error here is informational.
    fn on_run_complete(
        &self,
        _receipt: &Receipt,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Called when the runtime encounters a [`RuntimeError`].
    fn on_error(&self, _error: &RuntimeError) {}

    /// Human-readable name for this hook (used in logging / diagnostics).
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Ordered collection of [`LifecycleHook`]s that fires them in registration order.
pub struct HookRegistry {
    hooks: Vec<Box<dyn LifecycleHook + Send + Sync>>,
}

impl HookRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Append a hook. Hooks fire in the order they are registered.
    pub fn register(&mut self, hook: Box<dyn LifecycleHook + Send + Sync>) {
        self.hooks.push(hook);
    }

    /// Fire [`LifecycleHook::on_run_start`] on every registered hook.
    pub fn fire_run_start(
        &self,
        wo: &WorkOrder,
    ) -> Vec<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        self.hooks.iter().map(|h| h.on_run_start(wo)).collect()
    }

    /// Fire [`LifecycleHook::on_event`] on every registered hook.
    pub fn fire_event(
        &self,
        event: &AgentEvent,
    ) -> Vec<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        self.hooks.iter().map(|h| h.on_event(event)).collect()
    }

    /// Fire [`LifecycleHook::on_run_complete`] on every registered hook.
    pub fn fire_run_complete(
        &self,
        receipt: &Receipt,
    ) -> Vec<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        self.hooks
            .iter()
            .map(|h| h.on_run_complete(receipt))
            .collect()
    }

    /// Fire [`LifecycleHook::on_error`] on every registered hook.
    pub fn fire_error(&self, error: &RuntimeError) {
        for h in &self.hooks {
            h.on_error(error);
        }
    }

    /// Number of registered hooks.
    #[must_use]
    pub fn hook_count(&self) -> usize {
        self.hooks.len()
    }

    /// Names of all registered hooks, in registration order.
    #[must_use]
    pub fn hook_names(&self) -> Vec<&str> {
        self.hooks.iter().map(|h| h.name()).collect()
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Built-in: LoggingHook
// ---------------------------------------------------------------------------

/// Logs lifecycle transitions via the `tracing` crate.
pub struct LoggingHook;

impl LifecycleHook for LoggingHook {
    fn on_run_start(
        &self,
        work_order: &WorkOrder,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!(
            target: "abp.hooks",
            work_order_id = %work_order.id,
            task = %work_order.task,
            "run starting"
        );
        Ok(())
    }

    fn on_event(&self, event: &AgentEvent) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::debug!(target: "abp.hooks", ?event, "agent event");
        Ok(())
    }

    fn on_run_complete(
        &self,
        receipt: &Receipt,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!(
            target: "abp.hooks",
            run_id = %receipt.meta.run_id,
            outcome = ?receipt.outcome,
            duration_ms = receipt.meta.duration_ms,
            "run complete"
        );
        Ok(())
    }

    fn on_error(&self, error: &RuntimeError) {
        tracing::error!(target: "abp.hooks", %error, "runtime error");
    }

    fn name(&self) -> &str {
        "logging"
    }
}

// ---------------------------------------------------------------------------
// Built-in: MetricsHook
// ---------------------------------------------------------------------------

/// Updates a shared [`RunMetrics`] collector on lifecycle events.
pub struct MetricsHook {
    metrics: Arc<RunMetrics>,
}

impl MetricsHook {
    /// Create a new metrics hook backed by the given collector.
    #[must_use]
    pub fn new(metrics: Arc<RunMetrics>) -> Self {
        Self { metrics }
    }

    /// Return a reference to the underlying metrics.
    #[must_use]
    pub fn metrics(&self) -> &RunMetrics {
        &self.metrics
    }
}

impl LifecycleHook for MetricsHook {
    fn on_run_complete(
        &self,
        receipt: &Receipt,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let success = matches!(
            receipt.outcome,
            abp_core::Outcome::Complete | abp_core::Outcome::Partial
        );
        let event_count = receipt.trace.len() as u64;
        self.metrics
            .record_run(receipt.meta.duration_ms, success, event_count);
        Ok(())
    }

    fn name(&self) -> &str {
        "metrics"
    }
}

// ---------------------------------------------------------------------------
// Built-in: ValidationHook
// ---------------------------------------------------------------------------

/// Validates a [`WorkOrder`] before the run starts.
///
/// Current checks:
/// - `task` must not be empty
/// - `workspace.root` must not be empty
pub struct ValidationHook;

impl LifecycleHook for ValidationHook {
    fn on_run_start(
        &self,
        work_order: &WorkOrder,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if work_order.task.trim().is_empty() {
            return Err("work order task must not be empty".into());
        }
        if work_order.workspace.root.trim().is_empty() {
            return Err("work order workspace root must not be empty".into());
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "validation"
    }
}
