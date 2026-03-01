// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fluent builder for constructing [`Receipt`]s.

use abp_core::{
    AgentEvent, ArtifactRef, BackendIdentity, CONTRACT_VERSION, CapabilityManifest, ExecutionMode,
    Outcome, Receipt, RunMetadata, UsageNormalized, VerificationReport,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Fluent builder for constructing [`Receipt`]s ergonomically.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
///
/// let receipt = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .backend_version("1.0.0")
///     .build();
///
/// assert_eq!(receipt.backend.id, "mock");
/// assert_eq!(receipt.outcome, Outcome::Complete);
/// ```
#[derive(Debug)]
pub struct ReceiptBuilder {
    backend_id: String,
    backend_version: Option<String>,
    adapter_version: Option<String>,
    capabilities: CapabilityManifest,
    mode: ExecutionMode,
    outcome: Outcome,
    work_order_id: Uuid,
    run_id: Option<Uuid>,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    usage_raw: serde_json::Value,
    usage: UsageNormalized,
    trace: Vec<AgentEvent>,
    artifacts: Vec<ArtifactRef>,
    verification: VerificationReport,
}

impl ReceiptBuilder {
    /// Create a new builder with the given backend identifier.
    #[must_use]
    pub fn new(backend_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            backend_id: backend_id.into(),
            backend_version: None,
            adapter_version: None,
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
            outcome: Outcome::Complete,
            work_order_id: Uuid::nil(),
            run_id: None,
            started_at: now,
            finished_at: now,
            usage_raw: serde_json::json!({}),
            usage: UsageNormalized::default(),
            trace: vec![],
            artifacts: vec![],
            verification: VerificationReport::default(),
        }
    }

    /// Set the run outcome.
    #[must_use]
    pub fn outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// Set the backend identifier.
    #[must_use]
    pub fn backend_id(mut self, id: impl Into<String>) -> Self {
        self.backend_id = id.into();
        self
    }

    /// Set the backend runtime version.
    #[must_use]
    pub fn backend_version(mut self, version: impl Into<String>) -> Self {
        self.backend_version = Some(version.into());
        self
    }

    /// Set the adapter version.
    #[must_use]
    pub fn adapter_version(mut self, version: impl Into<String>) -> Self {
        self.adapter_version = Some(version.into());
        self
    }

    /// Set the run start timestamp.
    #[must_use]
    pub fn started_at(mut self, dt: DateTime<Utc>) -> Self {
        self.started_at = dt;
        self
    }

    /// Set the run finish timestamp.
    #[must_use]
    pub fn finished_at(mut self, dt: DateTime<Utc>) -> Self {
        self.finished_at = dt;
        self
    }

    /// Set the work order identifier.
    #[must_use]
    pub fn work_order_id(mut self, id: Uuid) -> Self {
        self.work_order_id = id;
        self
    }

    /// Set a specific run ID instead of generating one.
    #[must_use]
    pub fn run_id(mut self, id: Uuid) -> Self {
        self.run_id = Some(id);
        self
    }

    /// Set the capability manifest.
    #[must_use]
    pub fn capabilities(mut self, caps: CapabilityManifest) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set the execution mode.
    #[must_use]
    pub fn mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the raw vendor-specific usage payload.
    #[must_use]
    pub fn usage_raw(mut self, raw: serde_json::Value) -> Self {
        self.usage_raw = raw;
        self
    }

    /// Set the normalized usage counters.
    #[must_use]
    pub fn usage(mut self, usage: UsageNormalized) -> Self {
        self.usage = usage;
        self
    }

    /// Set the verification report.
    #[must_use]
    pub fn verification(mut self, verification: VerificationReport) -> Self {
        self.verification = verification;
        self
    }

    /// Append a trace event.
    #[must_use]
    pub fn add_trace_event(mut self, event: AgentEvent) -> Self {
        self.trace.push(event);
        self
    }

    /// Append an artifact reference.
    #[must_use]
    pub fn add_artifact(mut self, artifact: ArtifactRef) -> Self {
        self.artifacts.push(artifact);
        self
    }

    /// Consume the builder and produce a [`Receipt`] (no hash).
    #[must_use]
    pub fn build(self) -> Receipt {
        let duration_ms = (self.finished_at - self.started_at)
            .num_milliseconds()
            .max(0) as u64;

        Receipt {
            meta: RunMetadata {
                run_id: self.run_id.unwrap_or_else(Uuid::new_v4),
                work_order_id: self.work_order_id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: self.started_at,
                finished_at: self.finished_at,
                duration_ms,
            },
            backend: BackendIdentity {
                id: self.backend_id,
                backend_version: self.backend_version,
                adapter_version: self.adapter_version,
            },
            capabilities: self.capabilities,
            mode: self.mode,
            usage_raw: self.usage_raw,
            usage: self.usage,
            trace: self.trace,
            artifacts: self.artifacts,
            verification: self.verification,
            outcome: self.outcome,
            receipt_sha256: None,
        }
    }

    /// Build the receipt and compute its hash.
    ///
    /// # Errors
    ///
    /// Returns [`abp_core::ContractError::Json`] if serialization fails.
    pub fn with_hash(self) -> Result<Receipt, abp_core::ContractError> {
        let mut receipt = self.build();
        receipt.receipt_sha256 = Some(crate::compute_hash(&receipt)?);
        Ok(receipt)
    }
}
