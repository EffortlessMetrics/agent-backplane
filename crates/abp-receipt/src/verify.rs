// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt verification and batch auditing utilities.

use std::collections::{HashMap, HashSet};
use std::fmt;

use abp_core::{CONTRACT_VERSION, Outcome, Receipt};

/// Result of verifying a single [`Receipt`].
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Whether the stored hash matches the recomputed hash (or no hash is stored).
    pub hash_valid: bool,
    /// Whether the contract version matches [`CONTRACT_VERSION`].
    pub contract_valid: bool,
    /// Whether timestamps are reasonable (finished >= started, duration consistent).
    pub timestamps_valid: bool,
    /// Whether outcome fields are internally consistent.
    pub outcome_consistent: bool,
    /// Human-readable descriptions of any issues found.
    pub issues: Vec<String>,
}

impl VerificationResult {
    /// Returns `true` if all checks passed.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        self.hash_valid && self.contract_valid && self.timestamps_valid && self.outcome_consistent
    }
}

impl fmt::Display for VerificationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_verified() {
            write!(f, "verified (0 issues)")
        } else {
            write!(
                f,
                "failed ({} issues: {})",
                self.issues.len(),
                self.issues.join("; ")
            )
        }
    }
}

/// Verify a receipt for hash integrity, contract version, timestamps, and outcome consistency.
///
/// Uses [`crate::compute_hash`] internally, which sets `receipt_sha256` to `null` before
/// hashing (the canonical receipt hashing gotcha).
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::verify::verify_receipt;
///
/// let r = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .with_hash()
///     .unwrap();
/// let result = verify_receipt(&r);
/// assert!(result.is_verified());
/// ```
pub fn verify_receipt(receipt: &Receipt) -> VerificationResult {
    let mut issues = Vec::new();

    // 1. Hash check
    let hash_valid = check_hash(receipt, &mut issues);

    // 2. Contract version check
    let contract_valid = if receipt.meta.contract_version != CONTRACT_VERSION {
        issues.push(format!(
            "contract version mismatch: expected \"{CONTRACT_VERSION}\", got \"{}\"",
            receipt.meta.contract_version
        ));
        false
    } else {
        true
    };

    // 3. Timestamp checks
    let timestamps_valid = check_timestamps(receipt, &mut issues);

    // 4. Outcome consistency
    let outcome_consistent = check_outcome(receipt, &mut issues);

    VerificationResult {
        hash_valid,
        contract_valid,
        timestamps_valid,
        outcome_consistent,
        issues,
    }
}

fn check_hash(receipt: &Receipt, issues: &mut Vec<String>) -> bool {
    match &receipt.receipt_sha256 {
        None => true,
        Some(stored) => match crate::compute_hash(receipt) {
            Ok(recomputed) => {
                if *stored != recomputed {
                    issues.push("stored hash does not match recomputed hash".into());
                    false
                } else {
                    true
                }
            }
            Err(e) => {
                issues.push(format!("failed to recompute hash: {e}"));
                false
            }
        },
    }
}

fn check_timestamps(receipt: &Receipt, issues: &mut Vec<String>) -> bool {
    let mut valid = true;

    if receipt.meta.finished_at < receipt.meta.started_at {
        issues.push("finished_at is before started_at".into());
        valid = false;
    }

    let expected_ms = (receipt.meta.finished_at - receipt.meta.started_at)
        .num_milliseconds()
        .max(0) as u64;
    if receipt.meta.duration_ms != expected_ms {
        issues.push(format!(
            "duration_ms is {}, expected {} from timestamps",
            receipt.meta.duration_ms, expected_ms
        ));
        valid = false;
    }

    valid
}

fn check_outcome(receipt: &Receipt, issues: &mut Vec<String>) -> bool {
    let has_error_event = receipt
        .trace
        .iter()
        .any(|e| matches!(e.kind, abp_core::AgentEventKind::Error { .. }));

    let mut consistent = true;

    // If outcome is Failed, we expect at least one error event in the trace
    // (unless trace is empty — some backends may not emit events).
    if receipt.outcome == Outcome::Failed && !receipt.trace.is_empty() && !has_error_event {
        issues.push("outcome is Failed but trace contains no error events".into());
        consistent = false;
    }

    // If outcome is Complete but trace contains error events, that's suspicious.
    if receipt.outcome == Outcome::Complete && has_error_event {
        issues.push("outcome is Complete but trace contains error events".into());
        consistent = false;
    }

    consistent
}

/// A single issue found during batch auditing.
#[derive(Debug, Clone)]
pub struct AuditIssue {
    /// Index of the receipt in the batch (if applicable).
    pub receipt_index: Option<usize>,
    /// The run ID of the receipt (if applicable).
    pub run_id: Option<String>,
    /// Description of the issue.
    pub description: String,
}

impl fmt::Display for AuditIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.receipt_index, &self.run_id) {
            (Some(idx), Some(id)) => write!(f, "[#{idx} run={id}] {}", self.description),
            (Some(idx), None) => write!(f, "[#{idx}] {}", self.description),
            (None, Some(id)) => write!(f, "[run={id}] {}", self.description),
            (None, None) => write!(f, "{}", self.description),
        }
    }
}

/// Report produced by [`ReceiptAuditor::audit_batch`].
#[derive(Debug, Clone)]
pub struct AuditReport {
    /// Total number of receipts audited.
    pub total: usize,
    /// Number of individually valid receipts.
    pub valid: usize,
    /// Number of individually invalid receipts.
    pub invalid: usize,
    /// Hex hashes that appeared more than once.
    pub duplicate_hashes: Vec<String>,
    /// All issues found across the batch.
    pub issues: Vec<AuditIssue>,
}

impl AuditReport {
    /// Returns `true` if the entire batch is clean.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.invalid == 0 && self.duplicate_hashes.is_empty() && self.issues.is_empty()
    }
}

impl fmt::Display for AuditReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AuditReport {{ total: {}, valid: {}, invalid: {}, duplicates: {}, issues: {} }}",
            self.total,
            self.valid,
            self.invalid,
            self.duplicate_hashes.len(),
            self.issues.len(),
        )
    }
}

/// Accumulates receipts for batch verification and anomaly detection.
///
/// # Examples
///
/// ```
/// use abp_receipt::{ReceiptBuilder, Outcome};
/// use abp_receipt::verify::ReceiptAuditor;
///
/// let auditor = ReceiptAuditor::new();
/// let r = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .with_hash()
///     .unwrap();
/// let report = auditor.audit_batch(&[r]);
/// assert!(report.is_clean());
/// ```
#[derive(Debug, Clone, Default)]
pub struct ReceiptAuditor {
    _priv: (),
}

impl ReceiptAuditor {
    /// Create a new auditor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Audit a batch of receipts for individual validity and cross-receipt anomalies.
    ///
    /// Checks:
    /// - Each receipt individually via [`verify_receipt`]
    /// - Duplicate hashes across the batch
    /// - Timeline consistency (no overlapping runs with the same backend)
    #[must_use]
    pub fn audit_batch(&self, receipts: &[Receipt]) -> AuditReport {
        let mut issues = Vec::new();
        let mut valid = 0usize;
        let mut invalid = 0usize;

        // Track hashes for duplicate detection.
        let mut hash_counts: HashMap<String, Vec<usize>> = HashMap::new();
        // Track run IDs for uniqueness.
        let mut seen_run_ids: HashSet<String> = HashSet::new();

        for (i, receipt) in receipts.iter().enumerate() {
            let result = verify_receipt(receipt);
            if result.is_verified() {
                valid += 1;
            } else {
                invalid += 1;
                for issue_desc in &result.issues {
                    issues.push(AuditIssue {
                        receipt_index: Some(i),
                        run_id: Some(receipt.meta.run_id.to_string()),
                        description: issue_desc.clone(),
                    });
                }
            }

            // Collect hashes for duplicate detection.
            if let Some(ref hash) = receipt.receipt_sha256 {
                hash_counts.entry(hash.clone()).or_default().push(i);
            }

            // Check for duplicate run IDs.
            let run_id_str = receipt.meta.run_id.to_string();
            if !seen_run_ids.insert(run_id_str.clone()) {
                issues.push(AuditIssue {
                    receipt_index: Some(i),
                    run_id: Some(run_id_str),
                    description: "duplicate run_id in batch".into(),
                });
            }
        }

        // Check for timeline consistency: receipts sorted by started_at
        // should not have overlapping intervals for the same backend.
        self.check_timeline_consistency(receipts, &mut issues);

        // Collect duplicate hashes.
        let duplicate_hashes: Vec<String> = hash_counts
            .into_iter()
            .filter(|(_, indices)| indices.len() > 1)
            .map(|(hash, indices)| {
                issues.push(AuditIssue {
                    receipt_index: None,
                    run_id: None,
                    description: format!("duplicate hash {hash} found at indices {indices:?}"),
                });
                hash
            })
            .collect();

        AuditReport {
            total: receipts.len(),
            valid,
            invalid,
            duplicate_hashes,
            issues,
        }
    }

    fn check_timeline_consistency(&self, receipts: &[Receipt], issues: &mut Vec<AuditIssue>) {
        // Group by backend and check for time-ordering anomalies.
        let mut by_backend: HashMap<&str, Vec<(usize, &Receipt)>> = HashMap::new();
        for (i, r) in receipts.iter().enumerate() {
            by_backend.entry(&r.backend.id).or_default().push((i, r));
        }

        for (backend_id, mut runs) in by_backend {
            runs.sort_by_key(|(_, r)| r.meta.started_at);
            for window in runs.windows(2) {
                let (_, prev) = &window[0];
                let (idx, curr) = &window[1];
                if curr.meta.started_at < prev.meta.finished_at {
                    issues.push(AuditIssue {
                        receipt_index: Some(*idx),
                        run_id: Some(curr.meta.run_id.to_string()),
                        description: format!(
                            "overlapping timeline with previous run on backend \"{backend_id}\""
                        ),
                    });
                }
            }
        }
    }
}
