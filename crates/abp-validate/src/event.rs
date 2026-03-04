// SPDX-License-Identifier: MIT OR Apache-2.0
//! Agent event sequence validation.

use abp_core::AgentEvent;

use crate::{ValidationErrorKind, ValidationErrors, Validator};

/// Validates a sequence of [`AgentEvent`]s.
///
/// Checks that timestamps are monotonically non-decreasing and that
/// the sequence contains valid bookend events.
#[derive(Debug, Default)]
pub struct EventValidator;

impl Validator<Vec<AgentEvent>> for EventValidator {
    fn validate(&self, events: &Vec<AgentEvent>) -> Result<(), ValidationErrors> {
        let mut errs = ValidationErrors::new();

        if events.is_empty() {
            // empty trace is valid (e.g. no-op run)
            return errs.into_result();
        }

        // timestamps must be monotonically non-decreasing
        for i in 1..events.len() {
            if events[i].ts < events[i - 1].ts {
                errs.add(
                    format!("events[{i}].ts"),
                    ValidationErrorKind::OutOfRange,
                    format!(
                        "timestamp at index {i} ({}) is before index {} ({})",
                        events[i].ts,
                        i - 1,
                        events[i - 1].ts,
                    ),
                );
            }
        }

        // first event should be RunStarted
        if !matches!(
            events.first().map(|e| &e.kind),
            Some(abp_core::AgentEventKind::RunStarted { .. })
        ) {
            errs.add(
                "events[0].kind",
                ValidationErrorKind::InvalidFormat,
                "first event should be run_started",
            );
        }

        // last event should be RunCompleted
        if events.len() > 1
            && !matches!(
                events.last().map(|e| &e.kind),
                Some(abp_core::AgentEventKind::RunCompleted { .. })
            )
        {
            errs.add(
                format!("events[{}].kind", events.len() - 1),
                ValidationErrorKind::InvalidFormat,
                "last event should be run_completed",
            );
        }

        // tool_result without a preceding tool_call with matching tool_name
        let mut pending_tool_calls: Vec<String> = Vec::new();
        for (i, event) in events.iter().enumerate() {
            match &event.kind {
                abp_core::AgentEventKind::ToolCall { tool_name, .. } => {
                    pending_tool_calls.push(tool_name.clone());
                }
                abp_core::AgentEventKind::ToolResult { tool_name, .. } => {
                    if let Some(pos) = pending_tool_calls.iter().position(|n| n == tool_name) {
                        pending_tool_calls.remove(pos);
                    } else {
                        errs.add(
                            format!("events[{i}].kind"),
                            ValidationErrorKind::InvalidReference,
                            format!("tool_result for '{tool_name}' has no preceding tool_call"),
                        );
                    }
                }
                _ => {}
            }
        }

        errs.into_result()
    }
}
