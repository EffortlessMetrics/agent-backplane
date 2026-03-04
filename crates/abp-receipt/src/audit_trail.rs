// SPDX-License-Identifier: MIT OR Apache-2.0

//! Audit trail for receipt lifecycle events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An action that can be recorded in the audit trail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// Receipt was created.
    Created,
    /// Hash was computed and attached.
    Hashed,
    /// Receipt was verified (with result).
    Verified {
        /// Whether verification succeeded.
        success: bool,
    },
    /// Receipt was archived / persisted.
    Archived,
    /// Receipt was exported to a format.
    Exported {
        /// The export format used.
        format: String,
    },
    /// Custom action.
    Custom {
        /// Description of the action.
        description: String,
    },
}

/// A single entry in the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp when this action occurred.
    pub timestamp: DateTime<Utc>,
    /// The run ID of the receipt this entry pertains to.
    pub run_id: Uuid,
    /// Who or what performed the action.
    pub actor: String,
    /// The action that was performed.
    pub action: AuditAction,
    /// Optional additional details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl AuditEntry {
    /// Create a new audit entry with the current timestamp.
    #[must_use]
    pub fn new(run_id: Uuid, actor: impl Into<String>, action: AuditAction) -> Self {
        Self {
            timestamp: Utc::now(),
            run_id,
            actor: actor.into(),
            action,
            details: None,
        }
    }

    /// Attach optional details to this entry.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// An ordered collection of audit entries tracking receipt lifecycle events.
///
/// # Examples
///
/// ```
/// use abp_receipt::audit_trail::{AuditTrail, AuditAction};
/// use uuid::Uuid;
///
/// let mut trail = AuditTrail::new();
/// let run_id = Uuid::new_v4();
/// trail.record(run_id, "system", AuditAction::Created);
/// trail.record(run_id, "system", AuditAction::Hashed);
/// assert_eq!(trail.len(), 2);
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditTrail {
    entries: Vec<AuditEntry>,
}

impl AuditTrail {
    /// Create an empty audit trail.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an action in the audit trail.
    pub fn record(
        &mut self,
        run_id: Uuid,
        actor: impl Into<String>,
        action: AuditAction,
    ) -> &AuditEntry {
        self.entries
            .push(AuditEntry::new(run_id, actor, action));
        self.entries.last().unwrap()
    }

    /// Record an action with additional details.
    pub fn record_with_details(
        &mut self,
        run_id: Uuid,
        actor: impl Into<String>,
        action: AuditAction,
        details: impl Into<String>,
    ) -> &AuditEntry {
        self.entries
            .push(AuditEntry::new(run_id, actor, action).with_details(details));
        self.entries.last().unwrap()
    }

    /// Return all entries in the trail.
    #[must_use]
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// Return entries for a specific run ID.
    #[must_use]
    pub fn entries_for_run(&self, run_id: Uuid) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.run_id == run_id).collect()
    }

    /// Return entries performed by a specific actor.
    #[must_use]
    pub fn entries_by_actor(&self, actor: &str) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.actor == actor)
            .collect()
    }

    /// Return the number of entries in the trail.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the trail is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Serialize the entire trail to JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a trail from JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
