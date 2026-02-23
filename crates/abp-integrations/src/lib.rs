//! abp-integrations
//!
//! Backends are how Agent Backplane talks to the outside world.
//!
//! In v0.1 we ship a `mock` backend and a generic `sidecar` backend.
//! Real SDK mappings live in separate crates/repos and register through the
//! same trait.

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CapabilityManifest, Outcome, Receipt, RunMetadata,
    UsageNormalized, VerificationReport, WorkOrder, CONTRACT_VERSION,
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
        let started = Utc::now();
        let mut trace = Vec::new();

        let mut emit = |kind: AgentEventKind| async {
            let ev = AgentEvent { ts: Utc::now(), kind };
            trace.push(ev.clone());
            let _ = events_tx.send(ev).await;
        };

        emit(AgentEventKind::RunStarted {
            message: format!("mock backend starting: {}", work_order.task),
        })
        .await;

        emit(AgentEventKind::AssistantMessage {
            text: "This is a mock backend. It does not call any real SDK.".into(),
        })
        .await;

        emit(AgentEventKind::AssistantMessage {
            text: "Use --backend sidecar:<name> once you add a sidecar config.".into(),
        })
        .await;

        emit(AgentEventKind::RunCompleted {
            message: "mock run complete".into(),
        })
        .await;

        let finished = Utc::now();
        let duration_ms = (finished - started)
            .to_std()
            .unwrap_or_default()
            .as_millis() as u64;

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
    pub spec: SidecarSpec,
}

impl SidecarBackend {
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
