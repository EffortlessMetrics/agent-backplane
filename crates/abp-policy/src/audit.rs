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
    Allow,
    Deny { reason: String },
    Warn { reason: String },
}

/// A single recorded policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub resource: String,
    pub decision: PolicyDecision,
}

/// Aggregate counts of policy decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSummary {
    pub allowed: usize,
    pub denied: usize,
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
