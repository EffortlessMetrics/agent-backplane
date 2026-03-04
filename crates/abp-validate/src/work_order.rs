// SPDX-License-Identifier: MIT OR Apache-2.0
//! Work order validation.

use abp_core::WorkOrder;

use crate::{ValidationErrorKind, ValidationErrors, Validator};

/// Validates [`WorkOrder`] fields.
#[derive(Debug, Default)]
pub struct WorkOrderValidator;

impl Validator<WorkOrder> for WorkOrderValidator {
    fn validate(&self, wo: &WorkOrder) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        // task must be non-empty
        if wo.task.trim().is_empty() {
            errs.add(
                "task",
                ValidationErrorKind::Required,
                "task must not be empty",
            );
        }

        // workspace root must be non-empty
        if wo.workspace.root.trim().is_empty() {
            errs.add(
                "workspace.root",
                ValidationErrorKind::Required,
                "workspace root must not be empty",
            );
        }

        // max_budget_usd must be non-negative when present
        if let Some(budget) = wo.config.max_budget_usd {
            if budget < 0.0 {
                errs.add(
                    "config.max_budget_usd",
                    ValidationErrorKind::OutOfRange,
                    "max_budget_usd must not be negative",
                );
            }
            if budget.is_nan() {
                errs.add(
                    "config.max_budget_usd",
                    ValidationErrorKind::InvalidFormat,
                    "max_budget_usd must not be NaN",
                );
            }
        }

        // max_turns must be > 0 when present
        if let Some(0) = wo.config.max_turns {
            errs.add(
                "config.max_turns",
                ValidationErrorKind::OutOfRange,
                "max_turns must be greater than zero",
            );
        }

        // context snippet names should be non-empty
        for (i, snippet) in wo.context.snippets.iter().enumerate() {
            if snippet.name.trim().is_empty() {
                errs.add(
                    format!("context.snippets[{i}].name"),
                    ValidationErrorKind::Required,
                    "snippet name must not be empty",
                );
            }
        }

        // policy: a tool should not appear in both allowed and disallowed
        for tool in &wo.policy.allowed_tools {
            if wo.policy.disallowed_tools.contains(tool) {
                errs.add(
                    "policy",
                    ValidationErrorKind::InvalidReference,
                    format!("tool '{tool}' appears in both allowed and disallowed lists"),
                );
            }
        }

        errs.into_result()
    }
}
