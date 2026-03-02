// SPDX-License-Identifier: MIT OR Apache-2.0
//! Convenience builders for constructing JSONL event and receipt payloads.
//!
//! Sidecar authors need to emit [`Frame::Event`](crate::Frame) frames whose
//! `event` field is a JSON object matching the ABP `AgentEvent` shape.
//! These helpers produce valid [`serde_json::Value`] payloads without
//! requiring a direct dependency on `abp-core`.
#![deny(unsafe_code)]

use chrono::Utc;
use serde_json::{Value, json};

use crate::Frame;

// ── Event helpers ───────────────────────────────────────────────────

/// Create an `assistant_delta` event value.
#[must_use]
pub fn event_text_delta(text: &str) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "assistant_delta",
        "text": text,
    })
}

/// Create an `assistant_message` event value.
#[must_use]
pub fn event_text_message(text: &str) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "assistant_message",
        "text": text,
    })
}

/// Create a `tool_call` event value.
#[must_use]
pub fn event_tool_call(tool_name: &str, tool_use_id: Option<&str>, input: Value) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "tool_call",
        "tool_name": tool_name,
        "tool_use_id": tool_use_id,
        "parent_tool_use_id": null,
        "input": input,
    })
}

/// Create a `tool_result` event value.
#[must_use]
pub fn event_tool_result(
    tool_name: &str,
    tool_use_id: Option<&str>,
    output: Value,
    is_error: bool,
) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "tool_result",
        "tool_name": tool_name,
        "tool_use_id": tool_use_id,
        "output": output,
        "is_error": is_error,
    })
}

/// Create an `error` event value.
#[must_use]
pub fn event_error(message: &str) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "error",
        "message": message,
    })
}

/// Create a `warning` event value.
#[must_use]
pub fn event_warning(message: &str) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "warning",
        "message": message,
    })
}

/// Create a `run_started` event value.
#[must_use]
pub fn event_run_started(message: &str) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "run_started",
        "message": message,
    })
}

/// Create a `run_completed` event value.
#[must_use]
pub fn event_run_completed(message: &str) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "run_completed",
        "message": message,
    })
}

/// Create a `file_changed` event value.
#[must_use]
pub fn event_file_changed(path: &str, summary: &str) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "file_changed",
        "path": path,
        "summary": summary,
    })
}

/// Create a `command_executed` event value.
#[must_use]
pub fn event_command_executed(
    command: &str,
    exit_code: Option<i32>,
    output_preview: Option<&str>,
) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "type": "command_executed",
        "command": command,
        "exit_code": exit_code,
        "output_preview": output_preview,
    })
}

// ── Frame helpers ───────────────────────────────────────────────────

/// Build a [`Frame::Event`] wrapping the given event value.
#[must_use]
pub fn event_frame(ref_id: &str, event: Value) -> Frame {
    Frame::Event {
        ref_id: ref_id.to_string(),
        event,
    }
}

/// Build a [`Frame::Fatal`] from a message string.
#[must_use]
pub fn fatal_frame(ref_id: Option<&str>, error: &str) -> Frame {
    Frame::Fatal {
        ref_id: ref_id.map(str::to_string),
        error: error.to_string(),
    }
}

/// Build a [`Frame::Hello`] with sensible defaults.
#[must_use]
pub fn hello_frame(backend_name: &str) -> Frame {
    Frame::Hello {
        contract_version: "abp/v0.1".to_string(),
        backend: json!({ "id": backend_name }),
        capabilities: json!({}),
        mode: Value::Null,
    }
}

// ── ReceiptBuilder ──────────────────────────────────────────────────

/// Incremental builder for constructing a receipt [`Value`].
///
/// Produces a JSON object matching the ABP `Receipt` shape without
/// requiring `abp-core` as a dependency.
#[derive(Debug, Clone)]
pub struct ReceiptBuilder {
    run_id: String,
    backend_id: String,
    outcome: String,
    events: Vec<Value>,
    artifacts: Vec<Value>,
    usage_raw: Value,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl ReceiptBuilder {
    /// Start building a receipt for the given `run_id`.
    #[must_use]
    pub fn new(run_id: &str, backend_id: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            backend_id: backend_id.to_string(),
            outcome: "complete".to_string(),
            events: Vec::new(),
            artifacts: Vec::new(),
            usage_raw: Value::Null,
            input_tokens: None,
            output_tokens: None,
        }
    }

    /// Set the outcome to `"failed"`.
    #[must_use]
    pub fn failed(mut self) -> Self {
        self.outcome = "failed".to_string();
        self
    }

    /// Set the outcome to `"partial"`.
    #[must_use]
    pub fn partial(mut self) -> Self {
        self.outcome = "partial".to_string();
        self
    }

    /// Append a trace event.
    #[must_use]
    pub fn event(mut self, event: Value) -> Self {
        self.events.push(event);
        self
    }

    /// Append an artifact reference.
    #[must_use]
    pub fn artifact(mut self, kind: &str, path: &str) -> Self {
        self.artifacts.push(json!({ "kind": kind, "path": path }));
        self
    }

    /// Set the raw usage payload.
    #[must_use]
    pub fn usage_raw(mut self, usage: Value) -> Self {
        self.usage_raw = usage;
        self
    }

    /// Set normalized input token count.
    #[must_use]
    pub fn input_tokens(mut self, n: u64) -> Self {
        self.input_tokens = Some(n);
        self
    }

    /// Set normalized output token count.
    #[must_use]
    pub fn output_tokens(mut self, n: u64) -> Self {
        self.output_tokens = Some(n);
        self
    }

    /// Consume the builder and produce a receipt [`Value`].
    #[must_use]
    pub fn build(self) -> Value {
        let now = Utc::now().to_rfc3339();
        json!({
            "meta": {
                "run_id": self.run_id,
                "work_order_id": self.run_id,
                "contract_version": "abp/v0.1",
                "started_at": now,
                "finished_at": now,
                "duration_ms": 0,
            },
            "backend": {
                "id": self.backend_id,
                "backend_version": null,
                "adapter_version": null,
            },
            "capabilities": {},
            "mode": "mapped",
            "usage_raw": self.usage_raw,
            "usage": {
                "input_tokens": self.input_tokens,
                "output_tokens": self.output_tokens,
                "cache_read_tokens": null,
                "cache_write_tokens": null,
                "request_units": null,
                "estimated_cost_usd": null,
            },
            "trace": self.events,
            "artifacts": self.artifacts,
            "verification": {
                "git_diff": null,
                "git_status": null,
                "harness_ok": false,
            },
            "outcome": self.outcome,
            "receipt_sha256": null,
        })
    }
}
