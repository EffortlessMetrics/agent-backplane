// SPDX-License-Identifier: MIT OR Apache-2.0
//! Audit trail for policy decisions.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::PolicyEngine;

/// Outcome of a single policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PolicyDecision {
    /// The action was permitted.
    Allow,
    /// The action was denied for the given reason.
    Deny {
        /// Human-readable explanation of why the action was denied.
        reason: String,
    },
    /// The action was allowed but flagged with a warning.
    Warn {
        /// Human-readable warning message.
        reason: String,
    },
}

/// A single recorded policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When the evaluation occurred.
    pub timestamp: DateTime<Utc>,
    /// The kind of action checked (e.g. `"tool"`, `"read"`, `"write"`).
    pub action: String,
    /// The resource name or path that was evaluated.
    pub resource: String,
    /// The resulting policy decision.
    pub decision: PolicyDecision,
}

/// Aggregate counts of policy decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSummary {
    /// Number of decisions that were allowed.
    pub allowed: usize,
    /// Number of decisions that were denied.
    pub denied: usize,
    /// Number of decisions that produced warnings.
    pub warned: usize,
}

/// Wraps a [`PolicyEngine`] and records every decision for later inspection.
pub struct PolicyAuditor {
    engine: PolicyEngine,
    log: Vec<AuditEntry>,
}

impl PolicyAuditor {
    /// Create a new auditor around the given engine.
    #[must_use]
    pub fn new(engine: PolicyEngine) -> Self {
        Self {
            engine,
            log: Vec::new(),
        }
    }

    /// Check whether a tool is permitted, recording the decision.
    pub fn check_tool(&mut self, name: &str) -> PolicyDecision {
        let d = self.engine.can_use_tool(name);
        let decision = if d.allowed {
            PolicyDecision::Allow
        } else {
            PolicyDecision::Deny {
                reason: d.reason.unwrap_or_default(),
            }
        };
        self.record("tool", name, decision.clone());
        decision
    }

    /// Check whether reading a path is permitted, recording the decision.
    pub fn check_read(&mut self, path: &str) -> PolicyDecision {
        let d = self.engine.can_read_path(Path::new(path));
        let decision = if d.allowed {
            PolicyDecision::Allow
        } else {
            PolicyDecision::Deny {
                reason: d.reason.unwrap_or_default(),
            }
        };
        self.record("read", path, decision.clone());
        decision
    }

    /// Check whether writing a path is permitted, recording the decision.
    pub fn check_write(&mut self, path: &str) -> PolicyDecision {
        let d = self.engine.can_write_path(Path::new(path));
        let decision = if d.allowed {
            PolicyDecision::Allow
        } else {
            PolicyDecision::Deny {
                reason: d.reason.unwrap_or_default(),
            }
        };
        self.record("write", path, decision.clone());
        decision
    }

    /// All recorded entries in chronological order.
    #[must_use]
    pub fn entries(&self) -> &[AuditEntry] {
        &self.log
    }

    /// Number of denied decisions so far.
    #[must_use]
    pub fn denied_count(&self) -> usize {
        self.log
            .iter()
            .filter(|e| matches!(e.decision, PolicyDecision::Deny { .. }))
            .count()
    }

    /// Number of allowed decisions so far.
    #[must_use]
    pub fn allowed_count(&self) -> usize {
        self.log
            .iter()
            .filter(|e| matches!(e.decision, PolicyDecision::Allow))
            .count()
    }

    /// Produce an aggregate summary of all recorded decisions.
    #[must_use]
    pub fn summary(&self) -> AuditSummary {
        let mut s = AuditSummary {
            allowed: 0,
            denied: 0,
            warned: 0,
        };
        for e in &self.log {
            match e.decision {
                PolicyDecision::Allow => s.allowed += 1,
                PolicyDecision::Deny { .. } => s.denied += 1,
                PolicyDecision::Warn { .. } => s.warned += 1,
            }
        }
        s
    }

    fn record(&mut self, action: &str, resource: &str, decision: PolicyDecision) {
        self.log.push(AuditEntry {
            timestamp: Utc::now(),
            action: action.to_string(),
            resource: resource.to_string(),
            decision,
        });
    }
}

// ---------------------------------------------------------------------------
// AuditAction / AuditLog — extended audit trail
// ---------------------------------------------------------------------------

/// Categorised action stored in an [`AuditLog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// A tool invocation was allowed.
    ToolAllowed,
    /// A tool invocation was denied.
    ToolDenied,
    /// A file read was allowed.
    ReadAllowed,
    /// A file read was denied.
    ReadDenied,
    /// A file write was allowed.
    WriteAllowed,
    /// A file write was denied.
    WriteDenied,
    /// The action was throttled by a rate-limit policy.
    RateLimited,
}

impl AuditAction {
    /// Returns `true` for any denial variant.
    #[must_use]
    pub fn is_denied(&self) -> bool {
        matches!(
            self,
            Self::ToolDenied | Self::ReadDenied | Self::WriteDenied | Self::RateLimited
        )
    }
}

/// A single record in an [`AuditLog`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// ISO-8601 timestamp string.
    pub timestamp: String,
    /// The categorised action.
    pub action: AuditAction,
    /// The resource (tool name or path) involved.
    pub resource: String,
    /// Name of the policy that produced this record, if known.
    pub policy_name: Option<String>,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

/// Append-only log of [`AuditRecord`]s.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditLog {
    records: Vec<AuditRecord>,
}

impl AuditLog {
    /// Create an empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a record to the log.
    pub fn record(
        &mut self,
        action: AuditAction,
        resource: impl Into<String>,
        policy_name: Option<&str>,
        reason: Option<&str>,
    ) {
        self.records.push(AuditRecord {
            timestamp: Utc::now().to_rfc3339(),
            action,
            resource: resource.into(),
            policy_name: policy_name.map(String::from),
            reason: reason.map(String::from),
        });
    }

    /// All recorded entries in chronological order.
    #[must_use]
    pub fn entries(&self) -> &[AuditRecord] {
        &self.records
    }

    /// Filter entries by a specific [`AuditAction`].
    #[must_use]
    pub fn filter_by_action(&self, action: &AuditAction) -> Vec<&AuditRecord> {
        self.records
            .iter()
            .filter(|r| &r.action == action)
            .collect()
    }

    /// Number of records that represent a denial.
    #[must_use]
    pub fn denied_count(&self) -> usize {
        self.records.iter().filter(|r| r.action.is_denied()).count()
    }

    /// Total number of records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` when the log has no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
