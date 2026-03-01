// SPDX-License-Identifier: MIT OR Apache-2.0
//! Extension traits for [`WorkOrder`], [`Receipt`], and [`AgentEvent`].

use crate::{AgentEvent, AgentEventKind, Capability, Outcome, Receipt, WorkOrder};
use std::collections::BTreeMap;

/// Convenience helpers for inspecting a [`WorkOrder`].
pub trait WorkOrderExt {
    /// Returns `true` if the work order requires `cap`.
    fn has_capability(&self, cap: &Capability) -> bool;

    /// Returns the remaining tool budget (`max_turns`), if one is set.
    ///
    /// Since a `WorkOrder` is a specification rather than live state, this
    /// simply returns the configured cap.
    fn tool_budget_remaining(&self) -> Option<u32>;

    /// Heuristic: returns `true` when the task text mentions code-related
    /// keywords (code, fix, implement, refactor).
    fn is_code_task(&self) -> bool;

    /// Returns the task description truncated to at most `max_len` characters.
    fn task_summary(&self, max_len: usize) -> String;

    /// Collects explicitly required capabilities and infers additional ones
    /// from the task text.
    fn required_capabilities(&self) -> Vec<Capability>;

    /// Looks up a key in `config.vendor`.
    fn vendor_config(&self, key: &str) -> Option<&serde_json::Value>;
}

impl WorkOrderExt for WorkOrder {
    fn has_capability(&self, cap: &Capability) -> bool {
        self.requirements
            .required
            .iter()
            .any(|r| &r.capability == cap)
    }

    fn tool_budget_remaining(&self) -> Option<u32> {
        self.config.max_turns
    }

    fn is_code_task(&self) -> bool {
        let lower = self.task.to_ascii_lowercase();
        ["code", "fix", "implement", "refactor"]
            .iter()
            .any(|kw| lower.contains(kw))
    }

    fn task_summary(&self, max_len: usize) -> String {
        if self.task.len() <= max_len {
            self.task.clone()
        } else {
            let mut end = max_len;
            // Avoid splitting a multi-byte character.
            while !self.task.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}…", &self.task[..end])
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        let mut caps: Vec<Capability> = self
            .requirements
            .required
            .iter()
            .map(|r| r.capability.clone())
            .collect();

        let lower = self.task.to_ascii_lowercase();
        if (lower.contains("edit") || lower.contains("refactor"))
            && !caps.contains(&Capability::ToolEdit)
        {
            caps.push(Capability::ToolEdit);
        }
        if (lower.contains("search") || lower.contains("grep"))
            && !caps.contains(&Capability::ToolGrep)
        {
            caps.push(Capability::ToolGrep);
        }
        if (lower.contains("bash") || lower.contains("shell") || lower.contains("command"))
            && !caps.contains(&Capability::ToolBash)
        {
            caps.push(Capability::ToolBash);
        }
        caps
    }

    fn vendor_config(&self, key: &str) -> Option<&serde_json::Value> {
        self.config.vendor.get(key)
    }
}

/// Convenience helpers for inspecting a [`Receipt`].
pub trait ReceiptExt {
    /// Returns `true` if the outcome is [`Outcome::Complete`].
    fn is_success(&self) -> bool;

    /// Returns `true` if the outcome is [`Outcome::Failed`].
    fn is_failure(&self) -> bool;

    /// Counts trace events grouped by their kind discriminator name.
    fn event_count_by_kind(&self) -> BTreeMap<String, usize>;

    /// Returns references to all `ToolCall` events in the trace.
    fn tool_calls(&self) -> Vec<&AgentEvent>;

    /// Returns references to all `AssistantMessage` events in the trace.
    fn assistant_messages(&self) -> Vec<&AgentEvent>;

    /// Returns the total number of `ToolCall` events.
    fn total_tool_calls(&self) -> usize;

    /// Returns `true` if the trace contains any `Error` events.
    fn has_errors(&self) -> bool;

    /// Wall-clock duration expressed in seconds.
    fn duration_secs(&self) -> f64;
}

impl ReceiptExt for Receipt {
    fn is_success(&self) -> bool {
        self.outcome == Outcome::Complete
    }

    fn is_failure(&self) -> bool {
        self.outcome == Outcome::Failed
    }

    fn event_count_by_kind(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for ev in &self.trace {
            let name = event_kind_name(&ev.kind);
            *counts.entry(name).or_insert(0) += 1;
        }
        counts
    }

    fn tool_calls(&self) -> Vec<&AgentEvent> {
        self.trace
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
            .collect()
    }

    fn assistant_messages(&self) -> Vec<&AgentEvent> {
        self.trace
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::AssistantMessage { .. }))
            .collect()
    }

    fn total_tool_calls(&self) -> usize {
        self.trace
            .iter()
            .filter(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
            .count()
    }

    fn has_errors(&self) -> bool {
        self.trace
            .iter()
            .any(|e| matches!(e.kind, AgentEventKind::Error { .. }))
    }

    fn duration_secs(&self) -> f64 {
        self.meta.duration_ms as f64 / 1000.0
    }
}

/// Convenience helpers for inspecting an [`AgentEvent`].
pub trait AgentEventExt {
    /// Returns `true` if this event is a `ToolCall`.
    fn is_tool_call(&self) -> bool;

    /// Returns `true` if this event is terminal (`RunCompleted`).
    fn is_terminal(&self) -> bool;

    /// Extracts the text payload from `AssistantDelta` or `AssistantMessage`.
    fn text_content(&self) -> Option<&str>;
}

impl AgentEventExt for AgentEvent {
    fn is_tool_call(&self) -> bool {
        matches!(self.kind, AgentEventKind::ToolCall { .. })
    }

    fn is_terminal(&self) -> bool {
        matches!(self.kind, AgentEventKind::RunCompleted { .. })
    }

    fn text_content(&self) -> Option<&str> {
        match &self.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
            _ => None,
        }
    }
}

/// Maps an [`AgentEventKind`] variant to its snake_case discriminator name.
fn event_kind_name(kind: &AgentEventKind) -> String {
    match kind {
        AgentEventKind::RunStarted { .. } => "run_started".into(),
        AgentEventKind::RunCompleted { .. } => "run_completed".into(),
        AgentEventKind::AssistantDelta { .. } => "assistant_delta".into(),
        AgentEventKind::AssistantMessage { .. } => "assistant_message".into(),
        AgentEventKind::ToolCall { .. } => "tool_call".into(),
        AgentEventKind::ToolResult { .. } => "tool_result".into(),
        AgentEventKind::FileChanged { .. } => "file_changed".into(),
        AgentEventKind::CommandExecuted { .. } => "command_executed".into(),
        AgentEventKind::Warning { .. } => "warning".into(),
        AgentEventKind::Error { .. } => "error".into(),
    }
}
