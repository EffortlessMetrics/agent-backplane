#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Shared backend abstractions and policy helpers.

use abp_core::{
    AgentEvent, CapabilityManifest, CapabilityRequirement, CapabilityRequirements, ExecutionMode,
    WorkOrder,
};
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use uuid::Uuid;

/// A backend that can execute work orders and stream agent events.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Returns the identity metadata for this backend.
    fn identity(&self) -> abp_core::BackendIdentity;
    /// Returns the capability manifest advertised by this backend.
    fn capabilities(&self) -> CapabilityManifest;

    /// Execute a work order.
    ///
    /// Backends are expected to stream events into `events_tx`.
    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<abp_core::Receipt>;
}

/// Checks that all required capabilities are satisfied by the given manifest.
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

/// Extracts the execution mode from a work order's vendor config, defaulting to `Mapped`.
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

/// Validates that a work order is compatible with passthrough execution mode.
pub fn validate_passthrough_compatibility(_work_order: &WorkOrder) -> Result<()> {
    Ok(())
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
