//! Sidecar backend implementation for JSONL protocol adapters.

use abp_backend_core::{Backend, ensure_capability_requirements};
use abp_core::{AgentEvent, BackendIdentity, CapabilityManifest, Receipt, WorkOrder};
use abp_host::{HostError, SidecarClient, SidecarSpec};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::debug;
use uuid::Uuid;

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
        BackendIdentity {
            id: "sidecar".to_string(),
            backend_version: None,
            adapter_version: Some("0.1".to_string()),
        }
    }

    fn capabilities(&self) -> CapabilityManifest {
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
