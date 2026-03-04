// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt metadata enrichment — compute and attach derived fields.

use abp_core::{AgentEventKind, Receipt};
use std::collections::BTreeMap;

/// Computed metadata that can be derived from a receipt's existing fields.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::enrich::ReceiptMetadata;
///
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// let meta = ReceiptMetadata::from_receipt(&r);
/// assert_eq!(meta.event_count, 0);
/// assert_eq!(meta.backend_id, "mock");
/// ```
#[derive(Debug, Clone)]
pub struct ReceiptMetadata {
    /// Backend identifier (copied for convenience).
    pub backend_id: String,
    /// Total number of trace events.
    pub event_count: usize,
    /// Number of error events in the trace.
    pub error_count: usize,
    /// Number of tool-use events in the trace.
    pub tool_use_count: usize,
    /// Number of assistant delta (text chunk) events.
    pub delta_count: usize,
    /// Total characters across all assistant delta events.
    pub total_delta_chars: usize,
    /// Number of artifact references.
    pub artifact_count: usize,
    /// Whether a hash is present.
    pub has_hash: bool,
    /// User-supplied tags (initially empty, populated via enrichment).
    pub tags: Vec<String>,
    /// User-supplied annotations (initially empty, populated via enrichment).
    pub annotations: BTreeMap<String, String>,
}

impl ReceiptMetadata {
    /// Derive metadata from an existing receipt.
    #[must_use]
    pub fn from_receipt(receipt: &Receipt) -> Self {
        let mut error_count = 0usize;
        let mut tool_use_count = 0usize;
        let mut delta_count = 0usize;
        let mut total_delta_chars = 0usize;

        for event in &receipt.trace {
            match &event.kind {
                AgentEventKind::Error { .. } => error_count += 1,
                AgentEventKind::ToolCall { .. } => tool_use_count += 1,
                AgentEventKind::AssistantDelta { text } => {
                    delta_count += 1;
                    total_delta_chars += text.len();
                }
                _ => {}
            }
        }

        Self {
            backend_id: receipt.backend.id.clone(),
            event_count: receipt.trace.len(),
            error_count,
            tool_use_count,
            delta_count,
            total_delta_chars,
            artifact_count: receipt.artifacts.len(),
            has_hash: receipt.receipt_sha256.is_some(),
            tags: Vec::new(),
            annotations: BTreeMap::new(),
        }
    }
}

/// Enriches receipts by computing and attaching derived metadata.
///
/// The enricher produces a [`ReceiptMetadata`] from the receipt's existing
/// fields and applies any configured tags and annotations.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::enrich::ReceiptEnricher;
///
/// let enricher = ReceiptEnricher::new()
///     .tag("production")
///     .annotate("team", "platform");
///
/// let r = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// let meta = enricher.enrich(&r);
/// assert!(meta.tags.contains(&"production".to_string()));
/// assert_eq!(meta.annotations.get("team").unwrap(), "platform");
/// ```
#[derive(Debug, Clone, Default)]
pub struct ReceiptEnricher {
    tags: Vec<String>,
    annotations: BTreeMap<String, String>,
}

impl ReceiptEnricher {
    /// Create a new enricher with no default tags or annotations.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tag that will be applied to all enriched receipts.
    #[must_use]
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Add an annotation that will be applied to all enriched receipts.
    #[must_use]
    pub fn annotate(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }

    /// Compute metadata for a receipt and apply configured tags/annotations.
    #[must_use]
    pub fn enrich(&self, receipt: &Receipt) -> ReceiptMetadata {
        let mut meta = ReceiptMetadata::from_receipt(receipt);
        meta.tags.extend(self.tags.iter().cloned());
        meta.annotations.extend(self.annotations.clone());
        meta
    }

    /// Enrich a batch of receipts.
    #[must_use]
    pub fn enrich_batch(&self, receipts: &[Receipt]) -> Vec<ReceiptMetadata> {
        receipts.iter().map(|r| self.enrich(r)).collect()
    }
}
