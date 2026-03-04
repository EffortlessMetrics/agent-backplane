// SPDX-License-Identifier: MIT OR Apache-2.0

//! Receipt diffing — compare two receipts field-by-field.

use abp_core::Receipt;

/// A single field-level difference between two receipts.
#[derive(Debug, Clone)]
pub struct FieldDiff {
    /// Dot-separated path to the field (e.g. `"outcome"`, `"usage.input_tokens"`).
    pub field: String,
    /// Value from the left receipt (JSON string).
    pub left: String,
    /// Value from the right receipt (JSON string).
    pub right: String,
}

/// Result of diffing two receipts.
#[derive(Debug, Clone)]
pub struct ReceiptDiff {
    /// The run ID of the left receipt.
    pub left_id: String,
    /// The run ID of the right receipt.
    pub right_id: String,
    /// Individual field differences found.
    pub differences: Vec<FieldDiff>,
}

impl ReceiptDiff {
    /// Returns `true` if no differences were found.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two receipts and return their field-level differences.
///
/// Compares selected top-level and nested fields. The `receipt_sha256` field
/// is excluded since it is derived.
#[must_use]
pub fn diff_receipts(left: &Receipt, right: &Receipt) -> ReceiptDiff {
    let mut diffs = Vec::new();

    macro_rules! cmp {
        ($field:expr, $l:expr, $r:expr) => {
            let lv = format!("{:?}", $l);
            let rv = format!("{:?}", $r);
            if lv != rv {
                diffs.push(FieldDiff {
                    field: $field.to_string(),
                    left: lv,
                    right: rv,
                });
            }
        };
    }

    // Meta fields.
    cmp!("meta.run_id", left.meta.run_id, right.meta.run_id);
    cmp!(
        "meta.work_order_id",
        left.meta.work_order_id,
        right.meta.work_order_id
    );
    cmp!(
        "meta.contract_version",
        left.meta.contract_version,
        right.meta.contract_version
    );
    cmp!(
        "meta.started_at",
        left.meta.started_at,
        right.meta.started_at
    );
    cmp!(
        "meta.finished_at",
        left.meta.finished_at,
        right.meta.finished_at
    );
    cmp!(
        "meta.duration_ms",
        left.meta.duration_ms,
        right.meta.duration_ms
    );

    // Backend.
    cmp!("backend.id", left.backend.id, right.backend.id);
    cmp!(
        "backend.backend_version",
        left.backend.backend_version,
        right.backend.backend_version
    );
    cmp!(
        "backend.adapter_version",
        left.backend.adapter_version,
        right.backend.adapter_version
    );

    // Top-level scalars.
    cmp!("outcome", left.outcome, right.outcome);
    cmp!("mode", left.mode, right.mode);

    // Usage.
    cmp!(
        "usage.input_tokens",
        left.usage.input_tokens,
        right.usage.input_tokens
    );
    cmp!(
        "usage.output_tokens",
        left.usage.output_tokens,
        right.usage.output_tokens
    );
    cmp!(
        "usage.cache_read_tokens",
        left.usage.cache_read_tokens,
        right.usage.cache_read_tokens
    );
    cmp!(
        "usage.cache_write_tokens",
        left.usage.cache_write_tokens,
        right.usage.cache_write_tokens
    );
    cmp!(
        "usage.request_units",
        left.usage.request_units,
        right.usage.request_units
    );
    cmp!(
        "usage.estimated_cost_usd",
        left.usage.estimated_cost_usd,
        right.usage.estimated_cost_usd
    );

    // Verification.
    cmp!(
        "verification.harness_ok",
        left.verification.harness_ok,
        right.verification.harness_ok
    );
    cmp!(
        "verification.git_diff",
        left.verification.git_diff,
        right.verification.git_diff
    );
    cmp!(
        "verification.git_status",
        left.verification.git_status,
        right.verification.git_status
    );

    // Collections (compare lengths + content).
    cmp!("trace.len", left.trace.len(), right.trace.len());
    cmp!("artifacts.len", left.artifacts.len(), right.artifacts.len());
    cmp!("capabilities", left.capabilities, right.capabilities);

    ReceiptDiff {
        left_id: left.meta.run_id.to_string(),
        right_id: right.meta.run_id.to_string(),
        differences: diffs,
    }
}
