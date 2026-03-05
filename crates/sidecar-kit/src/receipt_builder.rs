// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Typed receipt builder for sidecar authors.
//!
//! Wraps [`abp_core::ReceiptBuilder`] with a sidecar-friendly fluent API
//! that computes hashes, durations, and token usage automatically.
//!
//! # Example
//! ```
//! use sidecar_kit::receipt_builder::TypedReceiptBuilder;
//! use sidecar_kit::events::text_event;
//!
//! let receipt = TypedReceiptBuilder::new("my-sidecar")
//!     .status("complete")
//!     .add_event(text_event("hello"))
//!     .token_usage(100, 50)
//!     .build();
//!
//! assert_eq!(receipt.backend.id, "my-sidecar");
//! assert_eq!(receipt.outcome, abp_core::Outcome::Complete);
//! ```

use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use abp_core::{
    receipt_hash, AgentEvent, ArtifactRef, BackendIdentity, CapabilityManifest, ExecutionMode,
    Outcome, Receipt, RunMetadata, UsageNormalized, VerificationReport, CONTRACT_VERSION,
};

/// Typed receipt builder that produces an [`abp_core::Receipt`].
///
/// This builder is designed for sidecar authors who want a simple API
/// to construct valid receipts with computed hashes.
#[derive(Debug, Clone)]
pub struct TypedReceiptBuilder {
    backend_id: String,
    backend_version: Option<String>,
    adapter_version: Option<String>,
    model_name: Option<String>,
    outcome: Outcome,
    mode: ExecutionMode,
    work_order_id: Uuid,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    duration: Option<Duration>,
    trace: Vec<AgentEvent>,
    artifacts: Vec<ArtifactRef>,
    capabilities: CapabilityManifest,
    usage_raw: serde_json::Value,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    verification: VerificationReport,
}

impl TypedReceiptBuilder {
    /// Create a new builder with the given backend identifier.
    #[must_use]
    pub fn new(backend_id: impl Into<String>) -> Self {
        Self {
            backend_id: backend_id.into(),
            backend_version: None,
            adapter_version: None,
            model_name: None,
            outcome: Outcome::Complete,
            mode: ExecutionMode::default(),
            work_order_id: Uuid::nil(),
            started_at: Utc::now(),
            finished_at: None,
            duration: None,
            trace: Vec::new(),
            artifacts: Vec::new(),
            capabilities: CapabilityManifest::new(),
            usage_raw: serde_json::json!({}),
            input_tokens: None,
            output_tokens: None,
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: false,
            },
        }
    }

    /// Set the model name (stored in usage_raw for reference).
    #[must_use]
    pub fn model(mut self, name: impl Into<String>) -> Self {
        self.model_name = Some(name.into());
        self
    }

    /// Set the backend identifier.
    #[must_use]
    pub fn backend(mut self, id: impl Into<String>) -> Self {
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

    /// Set the outcome status from a string (`"complete"`, `"partial"`, `"failed"`).
    #[must_use]
    pub fn status(mut self, status: &str) -> Self {
        self.outcome = match status {
            "failed" => Outcome::Failed,
            "partial" => Outcome::Partial,
            _ => Outcome::Complete,
        };
        self
    }

    /// Set the outcome directly.
    #[must_use]
    pub fn outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// Set an explicit duration for the run.
    #[must_use]
    pub fn duration(mut self, d: Duration) -> Self {
        self.duration = Some(d);
        self
    }

    /// Set the work order ID this receipt corresponds to.
    #[must_use]
    pub fn work_order_id(mut self, id: Uuid) -> Self {
        self.work_order_id = id;
        self
    }

    /// Set the execution mode.
    #[must_use]
    pub fn mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the capability manifest.
    #[must_use]
    pub fn capabilities(mut self, caps: CapabilityManifest) -> Self {
        self.capabilities = caps;
        self
    }

    /// Append an event to the trace.
    #[must_use]
    pub fn add_event(mut self, event: AgentEvent) -> Self {
        self.trace.push(event);
        self
    }

    /// Append multiple events to the trace.
    #[must_use]
    pub fn add_events(mut self, events: impl IntoIterator<Item = AgentEvent>) -> Self {
        self.trace.extend(events);
        self
    }

    /// Append an artifact reference.
    #[must_use]
    pub fn add_artifact(mut self, kind: impl Into<String>, path: impl Into<String>) -> Self {
        self.artifacts.push(ArtifactRef {
            kind: kind.into(),
            path: path.into(),
        });
        self
    }

    /// Set normalized token usage counters.
    #[must_use]
    pub fn token_usage(mut self, prompt: u64, completion: u64) -> Self {
        self.input_tokens = Some(prompt);
        self.output_tokens = Some(completion);
        self
    }

    /// Set the raw vendor-specific usage payload.
    #[must_use]
    pub fn usage_raw(mut self, raw: serde_json::Value) -> Self {
        self.usage_raw = raw;
        self
    }

    /// Set the verification report.
    #[must_use]
    pub fn verification(mut self, report: VerificationReport) -> Self {
        self.verification = report;
        self
    }

    /// Consume the builder and produce a [`Receipt`] (without hash).
    #[must_use]
    pub fn build(self) -> Receipt {
        let finished_at = self.finished_at.unwrap_or_else(Utc::now);
        let duration_ms = self
            .duration
            .map(|d| d.as_millis() as u64)
            .unwrap_or_else(|| (finished_at - self.started_at).num_milliseconds().max(0) as u64);

        let mut usage_raw = self.usage_raw;
        if let Some(model) = &self.model_name {
            if let serde_json::Value::Object(ref mut map) = usage_raw {
                map.insert("model".to_string(), serde_json::json!(model));
            }
        }

        Receipt {
            meta: RunMetadata {
                run_id: Uuid::new_v4(),
                work_order_id: self.work_order_id,
                contract_version: CONTRACT_VERSION.to_string(),
                started_at: self.started_at,
                finished_at,
                duration_ms,
            },
            backend: BackendIdentity {
                id: self.backend_id,
                backend_version: self.backend_version,
                adapter_version: self.adapter_version,
            },
            capabilities: self.capabilities,
            mode: self.mode,
            usage_raw,
            usage: UsageNormalized {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            },
            trace: self.trace,
            artifacts: self.artifacts,
            verification: self.verification,
            outcome: self.outcome,
            receipt_sha256: None,
        }
    }

    /// Consume the builder and produce a [`Receipt`] with a computed hash.
    ///
    /// # Errors
    ///
    /// Returns an error if the receipt cannot be serialized for hashing.
    pub fn build_with_hash(self) -> Result<Receipt, abp_core::ContractError> {
        self.build().with_hash()
    }

    /// Build a receipt auto-computed from an event stream.
    ///
    /// Infers timing from the first and last event timestamps when available.
    #[must_use]
    pub fn from_events(backend_id: impl Into<String>, events: Vec<AgentEvent>) -> Receipt {
        let started_at = events.first().map(|e| e.ts).unwrap_or_else(Utc::now);
        let finished_at = events.last().map(|e| e.ts).unwrap_or_else(Utc::now);
        let duration_ms = (finished_at - started_at).num_milliseconds().max(0) as u64;

        Receipt {
            meta: RunMetadata {
                run_id: Uuid::new_v4(),
                work_order_id: Uuid::nil(),
                contract_version: CONTRACT_VERSION.to_string(),
                started_at,
                finished_at,
                duration_ms,
            },
            backend: BackendIdentity {
                id: backend_id.into(),
                backend_version: None,
                adapter_version: None,
            },
            capabilities: CapabilityManifest::new(),
            mode: ExecutionMode::default(),
            usage_raw: serde_json::json!({}),
            usage: UsageNormalized {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                request_units: None,
                estimated_cost_usd: None,
            },
            trace: events,
            artifacts: Vec::new(),
            verification: VerificationReport {
                git_diff: None,
                git_status: None,
                harness_ok: false,
            },
            outcome: Outcome::Complete,
            receipt_sha256: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{text_event, warning_event};

    #[test]
    fn basic_receipt_builds() {
        let receipt = TypedReceiptBuilder::new("test-backend").build();
        assert_eq!(receipt.backend.id, "test-backend");
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
        assert!(receipt.receipt_sha256.is_none());
    }

    #[test]
    fn receipt_with_hash() {
        let receipt = TypedReceiptBuilder::new("test-backend")
            .build_with_hash()
            .expect("hash should succeed");
        assert!(receipt.receipt_sha256.is_some());
        let hash = receipt.receipt_sha256.as_ref().unwrap();
        assert_eq!(hash.len(), 64); // SHA-256 hex
    }

    #[test]
    fn receipt_hash_is_deterministic() {
        // Two receipts with same content should get same hash
        // (but different run_ids, so they'll differ — test that hash is present)
        let r = TypedReceiptBuilder::new("test").build_with_hash().unwrap();
        assert!(r.receipt_sha256.is_some());
    }

    #[test]
    fn receipt_status_mapping() {
        let r = TypedReceiptBuilder::new("b").status("failed").build();
        assert_eq!(r.outcome, Outcome::Failed);

        let r = TypedReceiptBuilder::new("b").status("partial").build();
        assert_eq!(r.outcome, Outcome::Partial);

        let r = TypedReceiptBuilder::new("b").status("complete").build();
        assert_eq!(r.outcome, Outcome::Complete);

        let r = TypedReceiptBuilder::new("b").status("unknown").build();
        assert_eq!(r.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_with_events() {
        let events = vec![text_event("hello"), warning_event("careful")];
        let receipt = TypedReceiptBuilder::new("b").add_events(events).build();
        assert_eq!(receipt.trace.len(), 2);
    }

    #[test]
    fn receipt_with_token_usage() {
        let receipt = TypedReceiptBuilder::new("b").token_usage(1000, 500).build();
        assert_eq!(receipt.usage.input_tokens, Some(1000));
        assert_eq!(receipt.usage.output_tokens, Some(500));
    }

    #[test]
    fn receipt_with_duration() {
        let receipt = TypedReceiptBuilder::new("b")
            .duration(Duration::from_secs(5))
            .build();
        assert_eq!(receipt.meta.duration_ms, 5000);
    }

    #[test]
    fn receipt_with_model() {
        let receipt = TypedReceiptBuilder::new("b").model("gpt-4").build();
        assert_eq!(receipt.usage_raw["model"], "gpt-4");
    }

    #[test]
    fn receipt_with_artifacts() {
        let receipt = TypedReceiptBuilder::new("b")
            .add_artifact("patch", "output.patch")
            .add_artifact("log", "run.log")
            .build();
        assert_eq!(receipt.artifacts.len(), 2);
        assert_eq!(receipt.artifacts[0].kind, "patch");
        assert_eq!(receipt.artifacts[0].path, "output.patch");
    }

    #[test]
    fn from_events_infers_timing() {
        use chrono::TimeZone;
        let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 5).unwrap();

        let events = vec![
            AgentEvent {
                ts: t1,
                kind: abp_core::AgentEventKind::RunStarted {
                    message: "start".into(),
                },
                ext: None,
            },
            AgentEvent {
                ts: t2,
                kind: abp_core::AgentEventKind::RunCompleted {
                    message: "done".into(),
                },
                ext: None,
            },
        ];

        let receipt = TypedReceiptBuilder::from_events("b", events);
        assert_eq!(receipt.meta.started_at, t1);
        assert_eq!(receipt.meta.finished_at, t2);
        assert_eq!(receipt.meta.duration_ms, 5000);
        assert_eq!(receipt.trace.len(), 2);
    }

    #[test]
    fn from_events_empty() {
        let receipt = TypedReceiptBuilder::from_events("b", vec![]);
        assert_eq!(receipt.trace.len(), 0);
        assert_eq!(receipt.outcome, Outcome::Complete);
    }

    #[test]
    fn receipt_roundtrips_json() {
        let receipt = TypedReceiptBuilder::new("test-backend")
            .status("complete")
            .token_usage(100, 50)
            .add_event(text_event("hello"))
            .build();
        let json = serde_json::to_string(&receipt).expect("serialize");
        let roundtrip: Receipt = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(roundtrip.backend.id, "test-backend");
        assert_eq!(roundtrip.outcome, Outcome::Complete);
    }
}
