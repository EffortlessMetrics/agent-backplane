// SPDX-License-Identifier: MIT OR Apache-2.0
//! abp-integrations
#![deny(unsafe_code)]
#![warn(missing_docs)]
//!
//! Backends are how Agent Backplane talks to the outside world.
//!
//! In v0.1 we ship a `mock` backend and a generic `sidecar` backend.
//! Real SDK mappings live in separate crates/repos and register through the
//! same trait.

pub mod projection;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, ExecutionMode, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport, WorkOrder,
};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::debug;
use uuid::Uuid;

/// Trait that all backends must implement to participate in the ABP runtime.
///
/// Backends stream [`AgentEvent`]s into the provided channel and return
/// a [`Receipt`] when the run completes.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Return the backend's identity metadata.
    fn identity(&self) -> BackendIdentity;

    /// Return the backend's capability manifest.
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

/// A backend for local development and unit tests.
#[derive(Debug, Clone)]
pub struct MockBackend;

#[async_trait]
impl Backend for MockBackend {
    fn identity(&self) -> BackendIdentity {
        BackendIdentity {
            id: "mock".to_string(),
            backend_version: Some("0.1".to_string()),
            adapter_version: Some("0.1".to_string()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        use abp_core::{Capability as C, SupportLevel as S};
        let mut m = CapabilityManifest::default();
        m.insert(C::Streaming, S::Native);
        m.insert(C::ToolRead, S::Emulated);
        m.insert(C::ToolWrite, S::Emulated);
        m.insert(C::ToolEdit, S::Emulated);
        m.insert(C::ToolBash, S::Emulated);
        m.insert(C::StructuredOutputJsonSchema, S::Emulated);
        m
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        ensure_capability_requirements(&work_order.requirements, &self.capabilities())
            .context("capability requirements not satisfied")?;

        let started = Utc::now();
        let mut trace = Vec::new();
        emit_event(
            &mut trace,
            &events_tx,
            AgentEventKind::RunStarted {
                message: format!("mock backend starting: {}", work_order.task),
            },
        )
        .await;

        emit_event(
            &mut trace,
            &events_tx,
            AgentEventKind::AssistantMessage {
                text: "This is a mock backend. It does not call any real SDK.".into(),
            },
        )
        .await;

        emit_event(
            &mut trace,
            &events_tx,
            AgentEventKind::AssistantMessage {
                text: "Use --backend sidecar:<name> once you add a sidecar config.".into(),
            },
        )
        .await;

        emit_event(
            &mut trace,
            &events_tx,
            AgentEventKind::RunCompleted {
                message: "mock run complete".into(),
            },
        )
        .await;

        let finished = Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

        let mode = extract_execution_mode(&work_order);

        let receipt = Receipt {
            meta: RunMetadata {
                run_id,
                work_order_id: work_order.id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: started,
                finished_at: finished,
                duration_ms,
            },
            backend: self.identity(),
            capabilities: self.capabilities(),
            mode,
            usage_raw: json!({"note": "mock"}),
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
}

/// Generic sidecar backend.
///
/// A sidecar is an executable that speaks JSONL `abp-protocol` over stdio.
#[derive(Debug, Clone)]
pub struct SidecarBackend {
    /// Process specification for spawning the sidecar.
    pub spec: SidecarSpec,
}

impl SidecarBackend {
    /// Create a new [`SidecarBackend`] from a process specification.
    #[must_use]
    pub fn new(spec: SidecarSpec) -> Self {
        Self { spec }
    }
}

#[async_trait]
impl Backend for SidecarBackend {
    fn identity(&self) -> BackendIdentity {
        // Best-effort: sidecar reports identity during handshake.
        BackendIdentity {
            id: "sidecar".to_string(),
            backend_version: None,
            adapter_version: Some("0.1".to_string()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
        // Best-effort: handshake provides authoritative capabilities.
        CapabilityManifest::default()
    }

    async fn run(
        &self,
        run_id: Uuid,
        work_order: WorkOrder,
        events_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<Receipt> {
        let client = SidecarClient::spawn(self.spec.clone())
            .await
            .context("spawn sidecar")?;

        ensure_capability_requirements(&work_order.requirements, &client.hello.capabilities)
            .context("capability requirements not satisfied")?;

        debug!(target: "abp.sidecar", "connected to sidecar backend={}", client.hello.backend.id);

        let mut run = client
            .run(run_id.to_string(), work_order)
            .await
            .context("start run")?;

        while let Some(ev) = run.events.next().await {
            let _ = events_tx.send(ev).await;
        }

        let receipt = run
            .receipt
            .await
            .context("receive receipt")?
            .map_err(host_to_anyhow)?;

        let _ = run.wait.await;
        Ok(receipt)
    }
}

fn host_to_anyhow(e: HostError) -> anyhow::Error {
    anyhow::anyhow!(e)
}

/// Verify that a backend's capabilities satisfy all requirements.
///
/// Returns an error listing every unsatisfied requirement.
///
/// # Errors
///
/// Returns an error with details on each unsatisfied capability requirement.
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

async fn emit_event(
    trace: &mut Vec<AgentEvent>,
    events_tx: &mpsc::Sender<AgentEvent>,
    kind: AgentEventKind,
) {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    };
    trace.push(ev.clone());
    let _ = events_tx.send(ev).await;
}

/// Extract execution mode from WorkOrder config.vendor.abp.mode.
///
/// Returns `ExecutionMode::Mapped` (default) if not specified.
#[must_use]
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
///
/// # Errors
///
/// Currently always succeeds. Future versions may enforce additional constraints.
pub fn validate_passthrough_compatibility(_work_order: &WorkOrder) -> Result<()> {
    // In v0.1, we accept passthrough mode for any work order.
    // Future versions may enforce additional constraints.
    Ok(())
}
