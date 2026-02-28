// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(unsafe_code)]

//! Comprehensive receipt and chain verification.

use std::collections::HashSet;

use uuid::Uuid;

use crate::{receipt_hash, AgentEventKind, Receipt, CONTRACT_VERSION};

/// Result of a single verification check.
#[derive(Debug, Clone)]
pub struct VerificationCheck {
    /// Human-readable name of the check.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Detail message explaining the result.
    pub detail: String,
}

/// Aggregated verification result for a single receipt.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    /// The run ID of the verified receipt.
    pub receipt_id: String,
    /// Individual check results.
    pub checks: Vec<VerificationCheck>,
    /// `true` only when every check passed.
    pub passed: bool,
}

/// Aggregated verification result for an ordered chain of receipts.
#[derive(Debug, Clone)]
pub struct ChainVerificationReport {
    /// Number of receipts in the chain.
    pub receipt_count: usize,
    /// `true` only when all individual and chain-level checks pass.
    pub all_valid: bool,
    /// Per-receipt verification reports.
    pub individual_reports: Vec<VerificationReport>,
    /// Chain-level checks (ordering, duplicates, version consistency).
    pub chain_checks: Vec<VerificationCheck>,
}

/// Verifies individual [`Receipt`]s for integrity and completeness.
#[derive(Debug, Clone, Default)]
pub struct ReceiptVerifier {
    _private: (),
}

impl ReceiptVerifier {
    /// Create a new verifier.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Run all verification checks against a single receipt.
    #[must_use]
    pub fn verify(&self, receipt: &Receipt) -> VerificationReport {
        let checks = vec![
            self.check_hash_integrity(receipt),
            self.check_contract_version(receipt),
            self.check_work_order_id(receipt),
            self.check_run_id(receipt),
            self.check_outcome(receipt),
            self.check_backend(receipt),
            self.check_timestamps(receipt),
            self.check_trace_order(receipt),
            self.check_trace_duplicate_ids(receipt),
        ];

        let passed = checks.iter().all(|c| c.passed);
        VerificationReport {
            receipt_id: receipt.meta.run_id.to_string(),
            checks,
            passed,
        }
    }

    fn check_hash_integrity(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "hash_integrity".to_string();
        match &receipt.receipt_sha256 {
            None => VerificationCheck {
                name,
                passed: true,
                detail: "no hash present; skipped".into(),
            },
            Some(stored) => match receipt_hash(receipt) {
                Ok(recomputed) if *stored == recomputed => VerificationCheck {
                    name,
                    passed: true,
                    detail: "hash matches".into(),
                },
                Ok(recomputed) => VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("expected {recomputed}, got {stored}"),
                },
                Err(e) => VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("failed to recompute hash: {e}"),
                },
            },
        }
    }

    fn check_contract_version(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "contract_version".to_string();
        let ver = &receipt.meta.contract_version;
        if ver.is_empty() {
            return VerificationCheck {
                name,
                passed: false,
                detail: "contract version is empty".into(),
            };
        }
        // Expect format "abp/vX.Y"
        let valid_format = ver.starts_with("abp/v")
            && ver[5..].contains('.')
            && ver[5..].split('.').all(|p| !p.is_empty());
        if !valid_format {
            return VerificationCheck {
                name,
                passed: false,
                detail: format!("invalid format: \"{ver}\""),
            };
        }
        if *ver != CONTRACT_VERSION {
            return VerificationCheck {
                name,
                passed: true,
                detail: format!("valid format but differs from current ({CONTRACT_VERSION}): \"{ver}\""),
            };
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("matches current contract version ({CONTRACT_VERSION})"),
        }
    }

    fn check_work_order_id(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "work_order_id".to_string();
        if receipt.meta.work_order_id == Uuid::nil() {
            VerificationCheck {
                name,
                passed: false,
                detail: "work order ID is nil UUID".into(),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: format!("valid UUID: {}", receipt.meta.work_order_id),
            }
        }
    }

    fn check_run_id(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "run_id".to_string();
        if receipt.meta.run_id == Uuid::nil() {
            VerificationCheck {
                name,
                passed: false,
                detail: "run ID is nil UUID".into(),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: format!("valid UUID: {}", receipt.meta.run_id),
            }
        }
    }

    fn check_outcome(&self, receipt: &Receipt) -> VerificationCheck {
        // Outcome is an enum so it's always a recognized variant if deserialized.
        VerificationCheck {
            name: "outcome".to_string(),
            passed: true,
            detail: format!("recognized variant: {:?}", receipt.outcome),
        }
    }

    fn check_backend(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "backend".to_string();
        if receipt.backend.id.is_empty() {
            VerificationCheck {
                name,
                passed: false,
                detail: "backend ID is empty".into(),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: format!("backend present: \"{}\"", receipt.backend.id),
            }
        }
    }

    fn check_timestamps(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "timestamps".to_string();
        if receipt.meta.started_at > receipt.meta.finished_at {
            VerificationCheck {
                name,
                passed: false,
                detail: format!(
                    "started_at ({}) is after finished_at ({})",
                    receipt.meta.started_at, receipt.meta.finished_at
                ),
            }
        } else {
            VerificationCheck {
                name,
                passed: true,
                detail: "started_at <= finished_at".into(),
            }
        }
    }

    fn check_trace_order(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "trace_order".to_string();
        if receipt.trace.len() < 2 {
            return VerificationCheck {
                name,
                passed: true,
                detail: "fewer than 2 trace events; ordering trivially valid".into(),
            };
        }
        for i in 1..receipt.trace.len() {
            if receipt.trace[i].ts < receipt.trace[i - 1].ts {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!(
                        "event {} timestamp ({}) precedes event {} ({})",
                        i,
                        receipt.trace[i].ts,
                        i - 1,
                        receipt.trace[i - 1].ts,
                    ),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("{} trace events in order", receipt.trace.len()),
        }
    }

    fn check_trace_duplicate_ids(&self, receipt: &Receipt) -> VerificationCheck {
        let name = "trace_no_duplicate_ids".to_string();
        let mut seen = HashSet::new();
        for event in &receipt.trace {
            let id = match &event.kind {
                AgentEventKind::ToolCall { tool_use_id, .. } => tool_use_id.as_deref(),
                AgentEventKind::ToolResult { tool_use_id, .. } => tool_use_id.as_deref(),
                _ => None,
            };
            if let Some(id) = id
                && !seen.insert(id.to_string())
            {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("duplicate tool_use_id: \"{id}\""),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("no duplicate IDs among {} events", receipt.trace.len()),
        }
    }
}

/// Verifies an ordered chain of [`Receipt`]s for consistency.
pub struct ChainVerifier;

impl ChainVerifier {
    /// Verify an ordered slice of receipts as a chain.
    #[must_use]
    pub fn verify_chain(chain: &[Receipt]) -> ChainVerificationReport {
        let verifier = ReceiptVerifier::new();
        let individual_reports: Vec<VerificationReport> =
            chain.iter().map(|r| verifier.verify(r)).collect();

        let chain_checks = vec![
            Self::check_chain_order(chain),
            Self::check_no_duplicate_run_ids(chain),
            Self::check_consistent_version(chain),
        ];

        let all_individual_pass = individual_reports.iter().all(|r| r.passed);
        let all_chain_pass = chain_checks.iter().all(|c| c.passed);

        ChainVerificationReport {
            receipt_count: chain.len(),
            all_valid: all_individual_pass && all_chain_pass,
            individual_reports,
            chain_checks,
        }
    }

    fn check_chain_order(chain: &[Receipt]) -> VerificationCheck {
        let name = "chain_order".to_string();
        if chain.len() < 2 {
            return VerificationCheck {
                name,
                passed: true,
                detail: "fewer than 2 receipts; ordering trivially valid".into(),
            };
        }
        for i in 1..chain.len() {
            if chain[i].meta.started_at < chain[i - 1].meta.started_at {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!(
                        "receipt {} started_at ({}) precedes receipt {} ({})",
                        i,
                        chain[i].meta.started_at,
                        i - 1,
                        chain[i - 1].meta.started_at,
                    ),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("{} receipts in chronological order", chain.len()),
        }
    }

    fn check_no_duplicate_run_ids(chain: &[Receipt]) -> VerificationCheck {
        let name = "no_duplicate_run_ids".to_string();
        let mut seen = HashSet::new();
        for receipt in chain {
            let id = receipt.meta.run_id;
            if !seen.insert(id) {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!("duplicate run ID: {id}"),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("{} unique run IDs", seen.len()),
        }
    }

    fn check_consistent_version(chain: &[Receipt]) -> VerificationCheck {
        let name = "consistent_contract_version".to_string();
        if chain.is_empty() {
            return VerificationCheck {
                name,
                passed: true,
                detail: "empty chain".into(),
            };
        }
        let first = &chain[0].meta.contract_version;
        for (i, receipt) in chain.iter().enumerate().skip(1) {
            if receipt.meta.contract_version != *first {
                return VerificationCheck {
                    name,
                    passed: false,
                    detail: format!(
                        "receipt {} has version \"{}\" but receipt 0 has \"{first}\"",
                        i, receipt.meta.contract_version,
                    ),
                };
            }
        }
        VerificationCheck {
            name,
            passed: true,
            detail: format!("all receipts use version \"{first}\""),
        }
    }
}
