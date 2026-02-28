//! Local development backend with deterministic events.

use abp_backend::{Backend, ensure_capability_requirements};
use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, CONTRACT_VERSION, CapabilityManifest, Outcome,
    Receipt, RunMetadata, UsageNormalized, VerificationReport, WorkOrder,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

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

async fn emit_event(
    trace: &mut Vec<AgentEvent>,
    tx: &mpsc::Sender<AgentEvent>,
    kind: AgentEventKind,
) {
    let ev = AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    };
    trace.push(ev.clone());
    let _ = tx.send(ev).await;
}

fn extract_execution_mode(work_order: &WorkOrder) -> abp_core::ExecutionMode {
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

    abp_core::ExecutionMode::default()
}
