// SPDX-License-Identifier: MIT OR Apache-2.0
//! Work order parsing utilities for sidecar authors.
//!
//! When a sidecar receives a [`Frame::Run`](crate::Frame) the `work_order`
//! field is an opaque [`serde_json::Value`]. This module provides
//! [`WorkOrderView`] — a lightweight lens over that value — so sidecar
//! authors can extract common fields without pulling in `abp-core`.

use serde_json::Value;

/// A read-only view into a work-order JSON value.
///
/// Provides accessor methods for the most commonly used fields defined
/// by the ABP `WorkOrder` contract.
///
/// # Example
/// ```
/// use serde_json::json;
/// use sidecar_kit::work_order::WorkOrderView;
///
/// let raw = json!({
///     "id": "abc-123",
///     "task": "fix the bug",
///     "config": { "model": { "model_id": "gpt-4" } }
/// });
/// let view = WorkOrderView::new(&raw);
/// assert_eq!(view.id(), Some("abc-123"));
/// assert_eq!(view.task(), Some("fix the bug"));
/// assert_eq!(view.model_id(), Some("gpt-4"));
/// ```
#[derive(Debug, Clone)]
pub struct WorkOrderView<'a> {
    value: &'a Value,
}

impl<'a> WorkOrderView<'a> {
    /// Wrap a raw JSON value.
    #[must_use]
    pub fn new(value: &'a Value) -> Self {
        Self { value }
    }

    /// The work order ID (`id` field).
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.value.get("id").and_then(Value::as_str)
    }

    /// The task description (`task` field).
    #[must_use]
    pub fn task(&self) -> Option<&str> {
        self.value.get("task").and_then(Value::as_str)
    }

    /// The execution lane (`lane` field).
    #[must_use]
    pub fn lane(&self) -> Option<&str> {
        self.value.get("lane").and_then(Value::as_str)
    }

    /// The workspace root path (`workspace.root` field).
    #[must_use]
    pub fn workspace_root(&self) -> Option<&str> {
        self.value
            .get("workspace")
            .and_then(|w| w.get("root"))
            .and_then(Value::as_str)
    }

    /// The model identifier (`config.model.model_id` field).
    #[must_use]
    pub fn model_id(&self) -> Option<&str> {
        self.value
            .get("config")
            .and_then(|c| c.get("model"))
            .and_then(|m| m.get("model_id"))
            .and_then(Value::as_str)
    }

    /// The max-turns budget (`config.budget.max_turns` field).
    #[must_use]
    pub fn max_turns(&self) -> Option<u64> {
        self.value
            .get("config")
            .and_then(|c| c.get("budget"))
            .and_then(|b| b.get("max_turns"))
            .and_then(Value::as_u64)
    }

    /// The max-tokens budget (`config.budget.max_tokens` field).
    #[must_use]
    pub fn max_tokens(&self) -> Option<u64> {
        self.value
            .get("config")
            .and_then(|c| c.get("budget"))
            .and_then(|b| b.get("max_tokens"))
            .and_then(Value::as_u64)
    }

    /// The system prompt, if any (`config.model.system_prompt` field).
    #[must_use]
    pub fn system_prompt(&self) -> Option<&str> {
        self.value
            .get("config")
            .and_then(|c| c.get("model"))
            .and_then(|m| m.get("system_prompt"))
            .and_then(Value::as_str)
    }

    /// The policy profile value (`policy` field).
    #[must_use]
    pub fn policy(&self) -> Option<&Value> {
        self.value.get("policy")
    }

    /// The context packet value (`context` field).
    #[must_use]
    pub fn context(&self) -> Option<&Value> {
        self.value.get("context")
    }

    /// Access the entire raw value.
    #[must_use]
    pub fn raw(&self) -> &Value {
        self.value
    }

    /// Look up an arbitrary dotted path (e.g. `"config.vendor.abp.mode"`).
    #[must_use]
    pub fn get_path(&self, dotted: &str) -> Option<&Value> {
        let mut current = self.value;
        for segment in dotted.split('.') {
            current = current.get(segment)?;
        }
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_work_order() -> Value {
        json!({
            "id": "wo-001",
            "task": "refactor auth module",
            "lane": "patch_first",
            "workspace": {
                "root": "/tmp/workspace",
                "mode": "copy"
            },
            "context": {
                "files": []
            },
            "policy": {
                "tools": { "allow": ["*"] }
            },
            "config": {
                "model": {
                    "model_id": "claude-sonnet-4-20250514",
                    "system_prompt": "You are a helpful assistant."
                },
                "budget": {
                    "max_turns": 10,
                    "max_tokens": 100000
                },
                "vendor": {
                    "abp": { "mode": "mapped" }
                }
            }
        })
    }

    #[test]
    fn extracts_top_level_fields() {
        let wo = sample_work_order();
        let view = WorkOrderView::new(&wo);
        assert_eq!(view.id(), Some("wo-001"));
        assert_eq!(view.task(), Some("refactor auth module"));
        assert_eq!(view.lane(), Some("patch_first"));
    }

    #[test]
    fn extracts_workspace_root() {
        let wo = sample_work_order();
        let view = WorkOrderView::new(&wo);
        assert_eq!(view.workspace_root(), Some("/tmp/workspace"));
    }

    #[test]
    fn extracts_model_config() {
        let wo = sample_work_order();
        let view = WorkOrderView::new(&wo);
        assert_eq!(view.model_id(), Some("claude-sonnet-4-20250514"));
        assert_eq!(view.system_prompt(), Some("You are a helpful assistant."));
    }

    #[test]
    fn extracts_budget() {
        let wo = sample_work_order();
        let view = WorkOrderView::new(&wo);
        assert_eq!(view.max_turns(), Some(10));
        assert_eq!(view.max_tokens(), Some(100000));
    }

    #[test]
    fn get_path_navigates_nested() {
        let wo = sample_work_order();
        let view = WorkOrderView::new(&wo);
        let mode = view.get_path("config.vendor.abp.mode");
        assert_eq!(mode.and_then(Value::as_str), Some("mapped"));
    }

    #[test]
    fn missing_fields_return_none() {
        let wo = json!({});
        let view = WorkOrderView::new(&wo);
        assert_eq!(view.id(), None);
        assert_eq!(view.task(), None);
        assert_eq!(view.model_id(), None);
        assert_eq!(view.max_turns(), None);
    }
}
