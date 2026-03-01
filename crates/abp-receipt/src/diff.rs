// SPDX-License-Identifier: MIT OR Apache-2.0

//! Field-level diffing of two [`Receipt`]s.

use abp_core::Receipt;

/// A single field difference between two receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    /// Dot-separated field path (e.g. `"backend.id"`, `"outcome"`).
    pub field: String,
    /// Serialized old value.
    pub old: String,
    /// Serialized new value.
    pub new: String,
}

/// The result of comparing two receipts field by field.
#[derive(Debug, Clone)]
pub struct ReceiptDiff {
    /// Individual field differences. Empty if the receipts are equivalent.
    pub changes: Vec<FieldDiff>,
}

impl ReceiptDiff {
    /// Returns `true` if there are no differences.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Returns the number of differing fields.
    #[must_use]
    pub fn len(&self) -> usize {
        self.changes.len()
    }
}

/// Compare two receipts field by field and return all differences.
///
/// The comparison inspects the top-level semantic fields of the receipt.
/// `receipt_sha256` is intentionally excluded from the diff since it is
/// a derived value.
///
/// # Examples
///
/// ```
/// use abp_receipt::{diff_receipts, ReceiptBuilder, Outcome};
///
/// let a = ReceiptBuilder::new("mock").outcome(Outcome::Complete).build();
/// let mut b = a.clone();
/// b.outcome = Outcome::Failed;
///
/// let diff = diff_receipts(&a, &b);
/// assert!(!diff.is_empty());
/// assert!(diff.changes.iter().any(|d| d.field == "outcome"));
/// ```
pub fn diff_receipts(a: &Receipt, b: &Receipt) -> ReceiptDiff {
    let mut changes = Vec::new();

    // run_id
    if a.meta.run_id != b.meta.run_id {
        changes.push(FieldDiff {
            field: "meta.run_id".into(),
            old: a.meta.run_id.to_string(),
            new: b.meta.run_id.to_string(),
        });
    }

    // work_order_id
    if a.meta.work_order_id != b.meta.work_order_id {
        changes.push(FieldDiff {
            field: "meta.work_order_id".into(),
            old: a.meta.work_order_id.to_string(),
            new: b.meta.work_order_id.to_string(),
        });
    }

    // contract_version
    if a.meta.contract_version != b.meta.contract_version {
        changes.push(FieldDiff {
            field: "meta.contract_version".into(),
            old: a.meta.contract_version.clone(),
            new: b.meta.contract_version.clone(),
        });
    }

    // started_at
    if a.meta.started_at != b.meta.started_at {
        changes.push(FieldDiff {
            field: "meta.started_at".into(),
            old: a.meta.started_at.to_rfc3339(),
            new: b.meta.started_at.to_rfc3339(),
        });
    }

    // finished_at
    if a.meta.finished_at != b.meta.finished_at {
        changes.push(FieldDiff {
            field: "meta.finished_at".into(),
            old: a.meta.finished_at.to_rfc3339(),
            new: b.meta.finished_at.to_rfc3339(),
        });
    }

    // duration_ms
    if a.meta.duration_ms != b.meta.duration_ms {
        changes.push(FieldDiff {
            field: "meta.duration_ms".into(),
            old: a.meta.duration_ms.to_string(),
            new: b.meta.duration_ms.to_string(),
        });
    }

    // backend.id
    if a.backend.id != b.backend.id {
        changes.push(FieldDiff {
            field: "backend.id".into(),
            old: a.backend.id.clone(),
            new: b.backend.id.clone(),
        });
    }

    // backend.backend_version
    if a.backend.backend_version != b.backend.backend_version {
        changes.push(FieldDiff {
            field: "backend.backend_version".into(),
            old: format!("{:?}", a.backend.backend_version),
            new: format!("{:?}", b.backend.backend_version),
        });
    }

    // backend.adapter_version
    if a.backend.adapter_version != b.backend.adapter_version {
        changes.push(FieldDiff {
            field: "backend.adapter_version".into(),
            old: format!("{:?}", a.backend.adapter_version),
            new: format!("{:?}", b.backend.adapter_version),
        });
    }

    // outcome
    if a.outcome != b.outcome {
        changes.push(FieldDiff {
            field: "outcome".into(),
            old: format!("{:?}", a.outcome),
            new: format!("{:?}", b.outcome),
        });
    }

    // mode
    diff_json_field(&mut changes, "mode", &a.mode, &b.mode);

    // usage_raw
    if a.usage_raw != b.usage_raw {
        changes.push(FieldDiff {
            field: "usage_raw".into(),
            old: a.usage_raw.to_string(),
            new: b.usage_raw.to_string(),
        });
    }

    // usage (compare as JSON for simplicity)
    diff_json_field(&mut changes, "usage", &a.usage, &b.usage);

    // trace length
    if a.trace.len() != b.trace.len() {
        changes.push(FieldDiff {
            field: "trace.len".into(),
            old: a.trace.len().to_string(),
            new: b.trace.len().to_string(),
        });
    }

    // artifacts length
    if a.artifacts.len() != b.artifacts.len() {
        changes.push(FieldDiff {
            field: "artifacts.len".into(),
            old: a.artifacts.len().to_string(),
            new: b.artifacts.len().to_string(),
        });
    }

    // verification
    diff_json_field(
        &mut changes,
        "verification",
        &a.verification,
        &b.verification,
    );

    ReceiptDiff { changes }
}

fn diff_json_field<T: serde::Serialize>(changes: &mut Vec<FieldDiff>, name: &str, a: &T, b: &T) {
    let ja = serde_json::to_string(a).unwrap_or_default();
    let jb = serde_json::to_string(b).unwrap_or_default();
    if ja != jb {
        changes.push(FieldDiff {
            field: name.into(),
            old: ja,
            new: jb,
        });
    }
}
