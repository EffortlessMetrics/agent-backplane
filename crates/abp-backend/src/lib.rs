//! abp-backend
//!
//! Core backend traits and capability validation helpers.

use abp_core::{
    AgentEvent, BackendIdentity, CapabilityManifest, CapabilityRequirement, CapabilityRequirements,
    ExecutionMode, Receipt, WorkOrder,
};
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use uuid::Uuid;

#[async_trait]
pub trait Backend: Send + Sync {
    fn identity(&self) -> BackendIdentity;
    fn capabilities(&self) -> CapabilityManifest;

    /// Execute a work order.
    ///
    /// Backends are expected to stream events into `events_tx`.
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt>;
}

pub fn ensure_capability_requirements(
    requirements: &CapabilityRequirements,
    capabilities: &CapabilityManifest,
) -> Result<()> {
    let mut unsatisfied = Vec::new();

    for req in &requirements.required {
        if !capability_satisfies(req, capabilities) {
            unsatisfied.push(format_requirement(req, capabilities));
        }
    }

    if unsatisfied.is_empty() {
        return Ok(());
    }

    anyhow::bail!("unsatisfied requirements: {}", unsatisfied.join("; "));
}

fn capability_satisfies(req: &CapabilityRequirement, capabilities: &CapabilityManifest) -> bool {
    capabilities
        .get(&req.capability)
        .is_some_and(|level| level.satisfies(&req.min_support))
}

fn format_requirement(req: &CapabilityRequirement, capabilities: &CapabilityManifest) -> String {
    let actual = capabilities
        .get(&req.capability)
        .map(|v| format!("{v:?}"))
        .unwrap_or_else(|| "missing".to_string());
    format!(
        "{:?} requires {:?}, backend has {}",
        req.capability, req.min_support, actual
    )
}

/// Extract execution mode from WorkOrder config.vendor.abp.mode.
///
/// Returns `ExecutionMode::Mapped` (default) if not specified.
pub fn extract_execution_mode(work_order: &WorkOrder) -> ExecutionMode {
    let nested = work_order
        .config
        .vendor
        .get("abp")
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.get("mode"))
        .and_then(|m| serde_json::from_value(m.clone()).ok());

    if let Some(mode) = nested {
        return mode;
    }

    if let Some(mode) = work_order
        .config
        .vendor
        .get("abp.mode")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
    {
        return mode;
    }

    ExecutionMode::default()
}

/// Validate that passthrough mode is compatible with the backend.
///
/// Passthrough invariants:
/// - No request rewriting: SDK sees exactly what caller sent
/// - Stream equivalence: After removing ABP framing, stream is bitwise-equivalent
/// - Observer-only governance: Log/record but don't modify tool calls or outputs
pub fn validate_passthrough_compatibility(_work_order: &WorkOrder) -> Result<()> {
    // In v0.1, we accept passthrough mode for any work order.
    // Future versions may enforce additional constraints.
    Ok(())
}
