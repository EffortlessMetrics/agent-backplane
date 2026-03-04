// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compliance checking for receipts.

use abp_core::{CONTRACT_VERSION, Receipt};
use chrono::{Duration, Utc};

/// Severity level for compliance findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    /// Informational.
    Info,
    /// Something suspicious but not necessarily wrong.
    Warning,
    /// A compliance violation.
    Error,
}

/// A single compliance finding.
#[derive(Debug, Clone)]
pub struct ComplianceFinding {
    /// Which field or aspect is affected.
    pub field: String,
    /// Severity of the finding.
    pub severity: Severity,
    /// Human-readable description.
    pub message: String,
}

/// Report produced by [`ComplianceCheck::check`].
#[derive(Debug, Clone)]
pub struct ComplianceReport {
    /// All findings from the compliance check.
    pub findings: Vec<ComplianceFinding>,
}

impl ComplianceReport {
    /// Returns `true` if there are no errors.
    #[must_use]
    pub fn is_compliant(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// Returns only the error-level findings.
    #[must_use]
    pub fn errors(&self) -> Vec<&ComplianceFinding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .collect()
    }

    /// Returns only the warning-level findings.
    #[must_use]
    pub fn warnings(&self) -> Vec<&ComplianceFinding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .collect()
    }

    /// Total number of findings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// Returns `true` if there are no findings at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Configurable compliance checker for receipts.
///
/// # Examples
///
/// ```
/// use abp_receipt::compliance::ComplianceCheck;
/// use abp_receipt::{ReceiptBuilder, Outcome};
///
/// let receipt = ReceiptBuilder::new("mock")
///     .outcome(Outcome::Complete)
///     .with_hash()
///     .unwrap();
/// let report = ComplianceCheck::new().check(&receipt);
/// assert!(report.is_compliant());
/// ```
#[derive(Debug, Clone)]
pub struct ComplianceCheck {
    max_duration_ms: u64,
    max_age_days: i64,
}

impl Default for ComplianceCheck {
    fn default() -> Self {
        Self {
            max_duration_ms: 3_600_000,
            max_age_days: 365,
        }
    }
}

impl ComplianceCheck {
    /// Create a compliance checker with default thresholds.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum allowed duration in milliseconds.
    #[must_use]
    pub fn max_duration_ms(mut self, ms: u64) -> Self {
        self.max_duration_ms = ms;
        self
    }

    /// Set the maximum allowed age in days.
    #[must_use]
    pub fn max_age_days(mut self, days: i64) -> Self {
        self.max_age_days = days;
        self
    }

    /// Run all compliance checks on a receipt.
    #[must_use]
    pub fn check(&self, receipt: &Receipt) -> ComplianceReport {
        let mut findings = Vec::new();
        self.check_required_fields(receipt, &mut findings);
        self.check_hash(receipt, &mut findings);
        self.check_contract_version(receipt, &mut findings);
        self.check_timestamps(receipt, &mut findings);
        self.check_duration(receipt, &mut findings);
        self.check_age(receipt, &mut findings);
        ComplianceReport { findings }
    }

    /// Run compliance checks on a batch.
    #[must_use]
    pub fn check_batch(&self, receipts: &[Receipt]) -> Vec<ComplianceReport> {
        receipts.iter().map(|r| self.check(r)).collect()
    }

    fn check_required_fields(&self, receipt: &Receipt, findings: &mut Vec<ComplianceFinding>) {
        if receipt.backend.id.is_empty() {
            findings.push(ComplianceFinding {
                field: "backend.id".into(),
                severity: Severity::Error,
                message: "backend identifier must not be empty".into(),
            });
        }
        if receipt.meta.contract_version.is_empty() {
            findings.push(ComplianceFinding {
                field: "meta.contract_version".into(),
                severity: Severity::Error,
                message: "contract version must not be empty".into(),
            });
        }
        if receipt.meta.run_id.is_nil() {
            findings.push(ComplianceFinding {
                field: "meta.run_id".into(),
                severity: Severity::Warning,
                message: "run_id is nil UUID".into(),
            });
        }
    }

    fn check_hash(&self, receipt: &Receipt, findings: &mut Vec<ComplianceFinding>) {
        match &receipt.receipt_sha256 {
            None => {
                findings.push(ComplianceFinding {
                    field: "receipt_sha256".into(),
                    severity: Severity::Warning,
                    message: "no hash present -- receipt integrity cannot be verified".into(),
                });
            }
            Some(stored) => match crate::compute_hash(receipt) {
                Ok(recomputed) => {
                    if *stored != recomputed {
                        findings.push(ComplianceFinding {
                            field: "receipt_sha256".into(),
                            severity: Severity::Error,
                            message: "stored hash does not match recomputed hash".into(),
                        });
                    }
                }
                Err(e) => {
                    findings.push(ComplianceFinding {
                        field: "receipt_sha256".into(),
                        severity: Severity::Error,
                        message: format!("failed to recompute hash: {e}"),
                    });
                }
            },
        }
    }

    fn check_contract_version(&self, receipt: &Receipt, findings: &mut Vec<ComplianceFinding>) {
        if receipt.meta.contract_version != CONTRACT_VERSION {
            findings.push(ComplianceFinding {
                field: "meta.contract_version".into(),
                severity: Severity::Error,
                message: format!(
                    "expected \"{CONTRACT_VERSION}\", got \"{}\"",
                    receipt.meta.contract_version
                ),
            });
        }
    }

    fn check_timestamps(&self, receipt: &Receipt, findings: &mut Vec<ComplianceFinding>) {
        if receipt.meta.finished_at < receipt.meta.started_at {
            findings.push(ComplianceFinding {
                field: "meta.finished_at".into(),
                severity: Severity::Error,
                message: "finished_at is before started_at".into(),
            });
        }
        let expected_ms = (receipt.meta.finished_at - receipt.meta.started_at)
            .num_milliseconds()
            .max(0) as u64;
        if receipt.meta.duration_ms != expected_ms {
            findings.push(ComplianceFinding {
                field: "meta.duration_ms".into(),
                severity: Severity::Error,
                message: format!(
                    "duration_ms is {}, expected {} from timestamps",
                    receipt.meta.duration_ms, expected_ms
                ),
            });
        }
        if receipt.meta.started_at > Utc::now() {
            findings.push(ComplianceFinding {
                field: "meta.started_at".into(),
                severity: Severity::Warning,
                message: "started_at is in the future".into(),
            });
        }
    }

    fn check_duration(&self, receipt: &Receipt, findings: &mut Vec<ComplianceFinding>) {
        if receipt.meta.duration_ms > self.max_duration_ms {
            findings.push(ComplianceFinding {
                field: "meta.duration_ms".into(),
                severity: Severity::Warning,
                message: format!(
                    "duration {}ms exceeds threshold {}ms",
                    receipt.meta.duration_ms, self.max_duration_ms
                ),
            });
        }
    }

    fn check_age(&self, receipt: &Receipt, findings: &mut Vec<ComplianceFinding>) {
        let age = Utc::now() - receipt.meta.started_at;
        if age > Duration::days(self.max_age_days) {
            findings.push(ComplianceFinding {
                field: "meta.started_at".into(),
                severity: Severity::Warning,
                message: format!(
                    "receipt is older than {} days",
                    self.max_age_days
                ),
            });
        }
    }
}
